use crate::{canonical_json_fingerprint, HarnessStore, StoreError, StoreResult};
use firm_core::agentfirm_api::{ActorKind, ActorRef, CanonicalMessageDelivery, Message};
use firm_core::collaboration::{
    CancellationDecisionKind, CancellationRequestState, CrossNodeDeliveryProjection,
    DelegationCancellationDecision, DelegationCancellationRequest, DelegationDecision,
    DelegationDecisionKind, DelegationInboundMode, DelegationInboundPolicy,
    DelegationInboundPolicySnapshot, DelegationState, DelegationTerminalOutcome,
    FabricEffectCertainty, FabricError, FabricErrorCode, RemoteFactPublication, RemoteWorkRef,
    RoutedBusinessKind, RoutedBusinessOperation, TargetPlacementRef, WorkDelegationV1,
    COLLABORATION_STORE_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const COLLABORATION_OPERATIONS_LEDGER: &str = "agentfirm_collaboration_operations.jsonl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationOperation {
    pub store_version: String,
    pub company_id: String,
    pub command_name: String,
    pub authenticated_actor: ActorRef,
    pub idempotency_key: String,
    pub request_fingerprint: String,
    pub aggregate_kind: String,
    pub aggregate_id: String,
    pub resulting_revision: u64,
    pub resulting_projection: Value,
    pub immutable_side_records: Vec<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationMutationContext {
    pub company_id: String,
    pub authenticated_actor: ActorRef,
    pub command_name: String,
    pub idempotency_key: String,
    pub expected_revision: u64,
    pub occurred_at: String,
}

/// Server-resolved authority facts. Public callers never construct this from
/// request headers or bodies; the application boundary resolves it from the
/// authenticated session and canonical Team/Work projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCollaborationAuthority {
    pub source_host: ActorRef,
    pub source_work_owner: ActorRef,
    pub target_host: ActorRef,
    pub target_placement: TargetPlacementRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeDelegationRequest {
    pub delegation_id: String,
    pub source_work_ref: RemoteWorkRef,
    pub source_owner_ref: ActorRef,
    pub target_placement: TargetPlacementRef,
    pub requested_outcome: String,
    pub outcome_class: String,
    pub acceptance_contract: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollaborationMutationResult<T> {
    pub projection: T,
    pub operation: CollaborationOperation,
    pub replayed: bool,
}

fn collaboration_error(
    code: FabricErrorCode,
    message: impl Into<String>,
    resource_kind: &str,
    resource_id: &str,
    current_revision: Option<u64>,
) -> StoreError {
    StoreError::Conflict(
        serde_json::to_string(&FabricError {
            code,
            message: message.into(),
            retryable: false,
            effect_certainty: FabricEffectCertainty::None,
            resource_kind: resource_kind.into(),
            resource_id: resource_id.into(),
            current_revision,
        })
        .unwrap_or_else(|_| "collaboration mutation rejected".into()),
    )
}

fn require_non_empty(value: &str, field: &str) -> StoreResult<()> {
    if value.trim().is_empty() {
        return Err(collaboration_error(
            FabricErrorCode::ProtocolMismatch,
            format!("{field} must not be empty"),
            "request",
            field,
            None,
        ));
    }
    Ok(())
}

fn policy_snapshot(
    policy: &DelegationInboundPolicy,
) -> StoreResult<DelegationInboundPolicySnapshot> {
    let value = serde_json::json!({
        "policy_id": policy.id,
        "policy_revision": policy.revision,
        "mode": policy.mode,
        "allowed_outcome_classes": policy.allowed_outcome_classes,
        "max_active_delegations": policy.max_active_delegations,
    });
    Ok(DelegationInboundPolicySnapshot {
        policy_id: policy.id.clone(),
        policy_revision: policy.revision,
        policy_digest: canonical_json_fingerprint(&value),
        mode: policy.mode,
        allowed_outcome_classes: policy.allowed_outcome_classes.clone(),
        max_active_delegations: policy.max_active_delegations,
    })
}

fn exact_actor(actual: &ActorRef, expected: &ActorRef) -> bool {
    actual == expected && !actual.id.trim().is_empty()
}

impl HarnessStore {
    fn collaboration_operations_unlocked(&self) -> StoreResult<Vec<CollaborationOperation>> {
        let path = self.root().join(COLLABORATION_OPERATIONS_LEDGER);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(path)?;
        let durable_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let mut operations = Vec::new();
        for row in bytes[..durable_len].split(|byte| *byte == b'\n') {
            if row.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            operations.push(serde_json::from_slice(row)?);
        }
        Ok(operations)
    }

    fn write_collaboration_operations_atomic_unlocked(
        &self,
        operations: &[CollaborationOperation],
    ) -> StoreResult<()> {
        let path = self.root().join(COLLABORATION_OPERATIONS_LEDGER);
        let next = self
            .root()
            .join("agentfirm_collaboration_operations.jsonl.next");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&next)?;
        for operation in operations {
            serde_json::to_writer(&mut file, operation)?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&next, &path)?;
        std::fs::File::open(self.root())?.sync_all()?;
        Ok(())
    }

    pub fn collaboration_operations(&self) -> StoreResult<Vec<CollaborationOperation>> {
        self.collaboration_operations_unlocked()
    }

    fn latest_collaboration_projection_unlocked<T: serde::de::DeserializeOwned>(
        &self,
        company_id: &str,
        aggregate_kind: &str,
        aggregate_id: &str,
    ) -> StoreResult<Option<T>> {
        self.collaboration_operations_unlocked()?
            .into_iter()
            .filter(|operation| {
                operation.company_id == company_id
                    && operation.aggregate_kind == aggregate_kind
                    && operation.aggregate_id == aggregate_id
            })
            .max_by_key(|operation| operation.resulting_revision)
            .map(|operation| serde_json::from_value(operation.resulting_projection))
            .transpose()
            .map_err(StoreError::from)
    }

    fn latest_collaboration_delegations_unlocked(
        &self,
        company_id: &str,
    ) -> StoreResult<BTreeMap<String, WorkDelegationV1>> {
        let mut latest = BTreeMap::new();
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id == company_id
                && operation.aggregate_kind == "work_delegation_v1"
            {
                let delegation: WorkDelegationV1 =
                    serde_json::from_value(operation.resulting_projection)?;
                latest.insert(delegation.id.clone(), delegation);
            }
        }
        Ok(latest)
    }

    pub fn collaboration_delegations(
        &self,
        company_id: &str,
    ) -> StoreResult<Vec<WorkDelegationV1>> {
        Ok(self
            .latest_collaboration_delegations_unlocked(company_id)?
            .into_values()
            .collect())
    }

    pub fn put_collaboration_inbound_policy(
        &self,
        context: &CollaborationMutationContext,
        policy: &DelegationInboundPolicy,
        resolved_target_host: &ActorRef,
    ) -> StoreResult<CollaborationMutationResult<DelegationInboundPolicy>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if policy.company_id != context.company_id
            || policy.created_by_target_host != *resolved_target_host
            || !exact_actor(&context.authenticated_actor, resolved_target_host)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the server-resolved target Host may author inbound policy",
                "delegation_inbound_policy",
                &policy.id,
                None,
            ));
        }
        if policy.revision != context.expected_revision + 1
            || policy.max_active_delegations == 0
            || policy.allowed_outcome_classes.is_empty()
            || policy.target_team_id == policy.source_team_id
        {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "inbound policy revision, scope, outcome classes, or active limit is invalid",
                "delegation_inbound_policy",
                &policy.id,
                Some(context.expected_revision),
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "delegation_inbound_policy",
            &policy.id,
            serde_json::to_value(policy)?,
            policy,
            Vec::new(),
        )
    }

    fn commit_collaboration_projection_unlocked<T>(
        &self,
        context: &CollaborationMutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        request_payload: Value,
        resulting_projection: &T,
        immutable_side_records: Vec<Value>,
    ) -> StoreResult<CollaborationMutationResult<T>>
    where
        T: Clone + Serialize + serde::de::DeserializeOwned,
    {
        let fingerprint = canonical_json_fingerprint(&request_payload);
        let mut operations = self.collaboration_operations_unlocked()?;
        if let Some(existing) = operations.iter().find(|operation| {
            operation.company_id == context.company_id
                && operation.authenticated_actor == context.authenticated_actor
                && operation.command_name == context.command_name
                && operation.idempotency_key == context.idempotency_key
        }) {
            if existing.request_fingerprint != fingerprint
                || existing.aggregate_kind != aggregate_kind
                || existing.aggregate_id != aggregate_id
            {
                return Err(collaboration_error(
                    FabricErrorCode::IdempotencyConflict,
                    "idempotency key was reused for a different collaboration mutation",
                    aggregate_kind,
                    aggregate_id,
                    Some(existing.resulting_revision),
                ));
            }
            // Rewriting the complete durable frames also removes a possible
            // non-newline torn tail left by a crash. Exact replay is therefore
            // both effect-idempotent and a bounded recovery path.
            self.write_collaboration_operations_atomic_unlocked(&operations)?;
            return Ok(CollaborationMutationResult {
                projection: serde_json::from_value(existing.resulting_projection.clone())?,
                operation: existing.clone(),
                replayed: true,
            });
        }
        let current_revision = operations
            .iter()
            .filter(|operation| {
                operation.company_id == context.company_id
                    && operation.aggregate_kind == aggregate_kind
                    && operation.aggregate_id == aggregate_id
            })
            .map(|operation| operation.resulting_revision)
            .max()
            .unwrap_or(0);
        if current_revision != context.expected_revision {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                format!(
                    "expected revision {}, current revision is {current_revision}",
                    context.expected_revision
                ),
                aggregate_kind,
                aggregate_id,
                Some(current_revision),
            ));
        }
        let operation = CollaborationOperation {
            store_version: COLLABORATION_STORE_VERSION.into(),
            company_id: context.company_id.clone(),
            command_name: context.command_name.clone(),
            authenticated_actor: context.authenticated_actor.clone(),
            idempotency_key: context.idempotency_key.clone(),
            request_fingerprint: fingerprint,
            aggregate_kind: aggregate_kind.into(),
            aggregate_id: aggregate_id.into(),
            resulting_revision: current_revision + 1,
            resulting_projection: serde_json::to_value(resulting_projection)?,
            immutable_side_records,
            created_at: context.occurred_at.clone(),
        };
        operations.push(operation.clone());
        self.write_collaboration_operations_atomic_unlocked(&operations)?;
        Ok(CollaborationMutationResult {
            projection: resulting_projection.clone(),
            operation,
            replayed: false,
        })
    }

    pub fn propose_collaboration_delegation(
        &self,
        context: &CollaborationMutationContext,
        request: &ProposeDelegationRequest,
        authority: &ResolvedCollaborationAuthority,
        policy: &DelegationInboundPolicy,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        for (value, field) in [
            (&context.company_id, "company_id"),
            (&context.idempotency_key, "idempotency_key"),
            (&request.delegation_id, "delegation_id"),
            (&request.requested_outcome, "requested_outcome"),
            (&request.acceptance_contract, "acceptance_contract"),
        ] {
            require_non_empty(value, field)?;
        }
        if context.expected_revision != 0 {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "delegation propose must start at revision zero",
                "work_delegation_v1",
                &request.delegation_id,
                Some(context.expected_revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.source_host)
            && !exact_actor(&context.authenticated_actor, &authority.source_work_owner)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host or source Work owner may propose",
                "work_delegation_v1",
                &request.delegation_id,
                None,
            ));
        }
        if request.source_owner_ref != authority.source_work_owner
            || request.source_work_ref.team_id.is_empty()
            || request.source_work_ref.team_id == request.target_placement.team_id
            || request.target_placement != authority.target_placement
            || request.source_work_ref.node_id.is_empty()
        {
            return Err(collaboration_error(
                FabricErrorCode::TargetTeamPlacementChanged,
                "source/target authority or exact target placement does not match",
                "work_delegation_v1",
                &request.delegation_id,
                None,
            ));
        }
        if policy.company_id != context.company_id
            || policy.source_team_id != request.source_work_ref.team_id
            || policy.target_team_id != request.target_placement.team_id
            || policy.created_by_target_host != authority.target_host
            || policy.revoked_at.is_some()
            || !policy
                .allowed_outcome_classes
                .iter()
                .any(|class| class == &request.outcome_class)
        {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "target-owned inbound policy does not authorize this delegation",
                "delegation_inbound_policy",
                &policy.id,
                Some(policy.revision),
            ));
        }
        let canonical_policy = self
            .latest_collaboration_projection_unlocked::<DelegationInboundPolicy>(
                &context.company_id,
                "delegation_inbound_policy",
                &policy.id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::DelegationPolicyRejected,
                    "target inbound policy is not present in the canonical collaboration Store",
                    "delegation_inbound_policy",
                    &policy.id,
                    None,
                )
            })?;
        if canonical_json_fingerprint(&serde_json::to_value(&canonical_policy)?)
            != canonical_json_fingerprint(&serde_json::to_value(policy)?)
        {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "caller policy does not match the exact canonical target-owned revision",
                "delegation_inbound_policy",
                &policy.id,
                Some(canonical_policy.revision),
            ));
        }
        let active_count = self
            .latest_collaboration_delegations_unlocked(&context.company_id)?
            .values()
            .filter(|delegation| {
                delegation.source_team_id == request.source_work_ref.team_id
                    && delegation.target_placement.team_id == request.target_placement.team_id
                    && delegation.state != DelegationState::Terminal
            })
            .count() as u64;
        if active_count >= policy.max_active_delegations {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "target inbound policy active delegation limit is reached",
                "delegation_inbound_policy",
                &policy.id,
                Some(policy.revision),
            ));
        }
        let snapshot = policy_snapshot(policy)?;
        let state = match policy.mode {
            DelegationInboundMode::HostApprovalRequired => DelegationState::AwaitingTargetDecision,
            DelegationInboundMode::AutoAccept => DelegationState::ProvisioningTargetWork,
        };
        let delegation = WorkDelegationV1 {
            id: request.delegation_id.clone(),
            company_id: context.company_id.clone(),
            source_work_ref: request.source_work_ref.clone(),
            source_owner_ref: request.source_owner_ref.clone(),
            source_team_id: request.source_work_ref.team_id.clone(),
            source_node_id: request.source_work_ref.node_id.clone(),
            target_placement: request.target_placement.clone(),
            requested_outcome: request.requested_outcome.clone(),
            outcome_class: request.outcome_class.clone(),
            acceptance_contract: request.acceptance_contract.clone(),
            inbound_policy_snapshot: snapshot,
            target_work_ref: None,
            state,
            terminal_outcome: None,
            revision: 1,
            operation_id: request.operation_id.clone(),
            idempotency_key: context.idempotency_key.clone(),
            created_by: context.authenticated_actor.clone(),
            created_at: context.occurred_at.clone(),
            updated_at: context.occurred_at.clone(),
        };
        let payload = serde_json::json!({
            "request": request,
            "resolved_source_host": authority.source_host,
            "resolved_source_work_owner": authority.source_work_owner,
            "resolved_target_host": authority.target_host,
            "policy_snapshot": delegation.inbound_policy_snapshot,
        });
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            &request.delegation_id,
            payload,
            &delegation,
            Vec::new(),
        )
    }

    pub fn decide_collaboration_delegation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        decision: &DelegationDecision,
        authority: &ResolvedCollaborationAuthority,
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || decision.expected_delegation_revision != delegation.revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "delegation decision revision is stale",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if delegation.state != DelegationState::AwaitingTargetDecision {
            return Err(collaboration_error(
                FabricErrorCode::DelegationTerminal,
                "delegation is not awaiting a target decision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.target_host)
            || decision.decided_by_target_host != authority.target_host
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host may decide an inbound delegation",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if observed_target_placement != &delegation.target_placement
            || observed_target_placement != &authority.target_placement
        {
            return Err(collaboration_error(
                FabricErrorCode::TargetTeamPlacementChanged,
                "target Team placement generation changed before decision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        match decision.decision {
            DelegationDecisionKind::Accept => {
                delegation.state = DelegationState::ProvisioningTargetWork;
            }
            DelegationDecisionKind::Reject => {
                delegation.state = DelegationState::Terminal;
                delegation.terminal_outcome = Some(DelegationTerminalOutcome::Rejected);
            }
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "decision": decision,
                "observed_target_placement": observed_target_placement,
            }),
            &delegation,
            vec![serde_json::to_value(decision)?],
        )
    }

    pub fn cancel_delegation_before_accept(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        reason: &str,
        authority: &ResolvedCollaborationAuthority,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        require_non_empty(reason, "reason")?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || delegation.state != DelegationState::AwaitingTargetDecision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancel-before-accept requires the exact awaiting decision revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.source_host) {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host may cancel before target acceptance",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.state = DelegationState::Terminal;
        delegation.terminal_outcome = Some(DelegationTerminalOutcome::Cancelled);
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({"reason": reason}),
            &delegation,
            Vec::new(),
        )
    }

    pub fn target_work_create_operation(
        &self,
        company_id: &str,
        delegation_id: &str,
        actor: &ActorRef,
        created_at: &str,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if delegation.state != DelegationState::ProvisioningTargetWork {
            return Err(collaboration_error(
                FabricErrorCode::DelegationTerminal,
                "delegation is not ready to provision target Work",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "requested_outcome": delegation.requested_outcome,
            "acceptance_contract": delegation.acceptance_contract,
            "source_work_ref": delegation.source_work_ref,
            "target_placement": delegation.target_placement,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-target-work-{}", delegation.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: company_id.into(),
            kind: RoutedBusinessKind::TargetWorkCreate,
            authenticated_actor: actor.clone(),
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: delegation.revision,
            idempotency_key: format!("target-work-create:{}", delegation.id),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: "collaboration.target_work_create".into(),
            ordering_key: format!("delegation:{}", delegation.id),
            created_at: created_at.into(),
        })
    }

    pub fn apply_target_work_created(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        target_work_ref: &RemoteWorkRef,
        observed_target_placement: &TargetPlacementRef,
        routed_operation_id: &str,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || delegation.state != DelegationState::ProvisioningTargetWork
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "target Work result does not match current provisioning revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if observed_target_placement != &delegation.target_placement
            || target_work_ref.node_id != delegation.target_placement.node_id
            || target_work_ref.team_id != delegation.target_placement.team_id
            || target_work_ref.team_revision != delegation.target_placement.team_revision
            || target_work_ref.placement_generation
                != delegation.target_placement.placement_generation
            || target_work_ref.work_id == delegation.source_work_ref.work_id
        {
            return Err(collaboration_error(
                FabricErrorCode::TargetTeamPlacementChanged,
                "target Work result is outside the frozen placement",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.target_work_ref = Some(target_work_ref.clone());
        delegation.state = DelegationState::Active;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "target_work_ref": target_work_ref,
                "observed_target_placement": observed_target_placement,
                "routed_operation_id": routed_operation_id,
            }),
            &delegation,
            Vec::new(),
        )
    }

    pub fn request_delegation_cancellation(
        &self,
        context: &CollaborationMutationContext,
        request: &DelegationCancellationRequest,
        authority: &ResolvedCollaborationAuthority,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &request.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    &request.delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || request.expected_delegation_revision != delegation.revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancellation request revision is stale",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.source_host)
            || request.requested_by != authority.source_host
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host may request active cancellation",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        if !matches!(
            delegation.state,
            DelegationState::Active | DelegationState::ResultAvailable
        ) {
            return Err(collaboration_error(
                FabricErrorCode::DelegationTerminal,
                "delegation is not active",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        let mut frozen_request = request.clone();
        frozen_request.state = CancellationRequestState::Pending;
        frozen_request.revision = 1;
        frozen_request.updated_at = context.occurred_at.clone();
        delegation.state = DelegationState::CancellationRequested;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            &delegation.id,
            serde_json::to_value(&frozen_request)?,
            &delegation,
            vec![serde_json::to_value(&frozen_request)?],
        )
    }

    pub fn decide_delegation_cancellation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        request_id: &str,
        decision: &DelegationCancellationDecision,
        authority: &ResolvedCollaborationAuthority,
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || delegation.state != DelegationState::CancellationRequested
            || decision.cancellation_request_id != request_id
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancellation decision does not match the pending request",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.target_host)
            || decision.decided_by_target_host != authority.target_host
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host may decide cancellation",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if observed_target_placement != &delegation.target_placement
            || observed_target_placement != &authority.target_placement
        {
            return Err(collaboration_error(
                FabricErrorCode::TargetTeamPlacementChanged,
                "target placement changed before cancellation decision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        require_non_empty(&decision.native_work_event_ref, "native_work_event_ref")?;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        match decision.decision {
            CancellationDecisionKind::Accept => {
                delegation.state = DelegationState::Terminal;
                delegation.terminal_outcome = Some(DelegationTerminalOutcome::Cancelled);
            }
            CancellationDecisionKind::Reject => {
                delegation.state = DelegationState::Active;
            }
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "request_id": request_id,
                "decision": decision,
                "observed_target_placement": observed_target_placement,
            }),
            &delegation,
            vec![serde_json::to_value(decision)?],
        )
    }

    pub fn publish_remote_fact(
        &self,
        context: &CollaborationMutationContext,
        publication: &RemoteFactPublication,
        authorized_target_actors: &[ActorRef],
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<RemoteFactPublication>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &publication.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    &publication.delegation_id,
                    None,
                )
            })?;
        if !authorized_target_actors
            .iter()
            .any(|actor| exact_actor(&context.authenticated_actor, actor))
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "remote publication requires an exact target Work actor",
                "remote_fact_publication",
                &publication.id,
                None,
            ));
        }
        if !matches!(
            delegation.state,
            DelegationState::Active
                | DelegationState::ResultAvailable
                | DelegationState::CancellationRequested
        ) || publication.company_id != context.company_id
            || publication.origin_node_id != delegation.target_placement.node_id
            || publication.origin_team_id != delegation.target_placement.team_id
            || publication.fact_work_ref.team_id != delegation.target_placement.team_id
            || publication.fact_work_ref.node_id != delegation.target_placement.node_id
            || publication.fact_work_ref.placement_generation
                != delegation.target_placement.placement_generation
            || publication.delegation_source_work_ref != delegation.source_work_ref
            || observed_target_placement != &delegation.target_placement
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "remote fact is outside the exact Delegation/Work/placement scope",
                "remote_fact_publication",
                &publication.id,
                None,
            ));
        }
        let digest = canonical_json_fingerprint(&publication.snapshot.canonical_redacted_fact);
        if publication.snapshot.publication_id != publication.id
            || publication.snapshot.canonical_digest != digest
            || publication.fact_digest != digest
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationDigestMismatch,
                "remote fact canonical digest does not match the redacted snapshot",
                "remote_fact_publication",
                &publication.id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "remote_fact_publication",
            &publication.id,
            serde_json::to_value(publication)?,
            publication,
            Vec::new(),
        )
    }

    pub fn mark_delegation_result_available(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        publication_id: &str,
        operational_decision: &firm_core::collaboration::WorkOperationalDecisionRef,
        authority: &ResolvedCollaborationAuthority,
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || delegation.state != DelegationState::Active
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "result publication does not match the current active Delegation revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.target_host)
            || observed_target_placement != &delegation.target_placement
            || observed_target_placement != &authority.target_placement
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host on the frozen placement may publish an accepted result",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let publication = self
            .latest_collaboration_projection_unlocked::<RemoteFactPublication>(
                &context.company_id,
                "remote_fact_publication",
                publication_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::PublicationScopeMismatch,
                    "accepted result references a missing immutable publication",
                    "remote_fact_publication",
                    publication_id,
                    None,
                )
            })?;
        let target_work = delegation.target_work_ref.as_ref().ok_or_else(|| {
            collaboration_error(
                FabricErrorCode::TargetWorkCreateFailed,
                "active Delegation has no exact target Work ref",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            )
        })?;
        if publication.delegation_id != delegation.id
            || publication.fact_work_ref.work_id != target_work.work_id
            || operational_decision.work_ref.work_id != target_work.work_id
            || operational_decision.work_ref.work_revision
                != publication.fact_work_ref.work_revision
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "publication and WorkOperationalDecision do not bind the same target Submitted Work revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.state = DelegationState::ResultAvailable;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "publication_id": publication_id,
                "operational_decision": operational_decision,
                "observed_target_placement": observed_target_placement,
            }),
            &delegation,
            vec![serde_json::to_value(operational_decision)?],
        )
    }

    pub fn complete_delegation_after_source_integration(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        integrated_source_work_ref: &RemoteWorkRef,
        source_integration_event_ref: &str,
        authority: &ResolvedCollaborationAuthority,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        require_non_empty(source_integration_event_ref, "source_integration_event_ref")?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || delegation.state != DelegationState::ResultAvailable
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "source integration requires the exact result-available revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.source_host) {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host may close the collaboration relationship after integration",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if integrated_source_work_ref.execution_space_id
            != delegation.source_work_ref.execution_space_id
            || integrated_source_work_ref.node_id != delegation.source_work_ref.node_id
            || integrated_source_work_ref.team_id != delegation.source_work_ref.team_id
            || integrated_source_work_ref.work_id != delegation.source_work_ref.work_id
            || integrated_source_work_ref.work_revision < delegation.source_work_ref.work_revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "source integration evidence does not bind the original source Work lineage",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.state = DelegationState::Terminal;
        delegation.terminal_outcome = Some(DelegationTerminalOutcome::Completed);
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "integrated_source_work_ref": integrated_source_work_ref,
                "source_integration_event_ref": source_integration_event_ref,
            }),
            &delegation,
            Vec::new(),
        )
    }
}

/// Build Company-visible, read-only projections from target-owned canonical
/// deliveries. Exactly one row per recipient is required; partial success is
/// represented by independent states and never collapsed to Message-level
/// delivered truth.
pub fn project_cross_node_deliveries(
    message: &Message,
    deliveries: &[CanonicalMessageDelivery],
    routed_operation_id: &str,
    target_gateway_generation: Option<u64>,
    target_observed_sequence: u64,
    observed_at: &str,
) -> StoreResult<Vec<CrossNodeDeliveryProjection>> {
    let expected = message
        .recipients
        .iter()
        .filter(|recipient| {
            recipient.kind == firm_core::agentfirm_api::MessageRecipientKind::AgentIdentity
        })
        .map(|recipient| recipient.id.clone())
        .collect::<BTreeSet<_>>();
    let actual = deliveries
        .iter()
        .map(|delivery| delivery.recipient_identity_id.clone())
        .collect::<BTreeSet<_>>();
    if expected != actual || actual.len() != deliveries.len() {
        return Err(collaboration_error(
            FabricErrorCode::MessageRecipientUnauthorized,
            "per-recipient delivery set is missing, duplicated, or outside the immutable Message",
            "message",
            &message.id,
            None,
        ));
    }
    deliveries
        .iter()
        .map(|delivery| {
            if delivery.message_id != message.id {
                return Err(collaboration_error(
                    FabricErrorCode::MessageRecipientUnauthorized,
                    "delivery references a different immutable Message",
                    "canonical_message_delivery",
                    &delivery.id,
                    Some(delivery.version),
                ));
            }
            Ok(CrossNodeDeliveryProjection {
                delivery_id: delivery.id.clone(),
                message_id: delivery.message_id.clone(),
                recipient_actor_ref: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: delivery.recipient_identity_id.clone(),
                },
                recipient_session_id: delivery.recipient_session_id.clone(),
                recipient_runtime_generation: delivery.recipient_session_generation,
                target_node_id: delivery.target_node_id.clone(),
                target_gateway_generation,
                routed_operation_id: routed_operation_id.into(),
                state: delivery.status,
                attempt_refs: if delivery.attempt == 0 {
                    Vec::new()
                } else {
                    vec![format!(
                        "delivery-attempt:{}:{}",
                        delivery.id, delivery.attempt
                    )]
                },
                receipt_refs: delivery.provider_receipt_id.clone().into_iter().collect(),
                target_observed_sequence,
                observed_at: observed_at.into(),
            })
        })
        .collect()
}
