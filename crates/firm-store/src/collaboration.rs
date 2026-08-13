use crate::{canonical_json_fingerprint, HarnessStore, StoreError, StoreResult};
use firm_core::agentfirm_api::{ActorKind, ActorRef, CanonicalMessageDelivery, Message};
use firm_core::collaboration::{
    ArtifactImport, CancellationDecisionKind, CancellationRequestState,
    CrossNodeDeliveryProjection, DelegationCancellationDecision, DelegationCancellationRequest,
    DelegationDecision, DelegationDecisionKind, DelegationInboundMode, DelegationInboundPolicy,
    DelegationInboundPolicySnapshot, DelegationState, DelegationTerminalOutcome,
    FabricEffectCertainty, FabricError, FabricErrorCode, ImmutableMessageTransferPayload,
    RemoteFactPublication, RemoteMessageReplica, RemoteMessageTransferState, RemoteWorkRef,
    RoutedBusinessKind, RoutedBusinessOperation, RoutedBusinessReceipt,
    SourceRemoteMessageTransfer, SourceWorkAttestation, TargetPlacementRef, WorkDelegationV1,
    COLLABORATION_STORE_VERSION,
};
use firm_fabric::{ArtifactCapability, ArtifactCapabilityPurpose, RemoteArtifactManifest};
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
    pub store_sequence: u64,
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
    pub source_work_application_service: ActorRef,
    pub source_gateway_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeDelegationRequest {
    pub delegation_id: String,
    pub source_work_attestation_id: String,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationDelegationFilter {
    pub source_team_id: Option<String>,
    pub target_team_id: Option<String>,
    pub node_id: Option<String>,
    pub state: Option<DelegationState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationCursor {
    pub as_of_store_sequence: u64,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationPage<T> {
    pub items: Vec<T>,
    pub as_of_store_sequence: u64,
    pub next_cursor: Option<CollaborationCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationScopedCursor {
    pub company_id: String,
    pub actor_digest: String,
    pub filter_digest: String,
    pub as_of_store_sequence: u64,
    pub raw_offset: usize,
    pub visible_progress: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationScopedPage<T> {
    pub items: Vec<T>,
    pub as_of_store_sequence: u64,
    pub next_cursor: Option<CollaborationScopedCursor>,
}

pub trait CollaborationFabricPort {
    fn dispatch(
        &self,
        operation: &RoutedBusinessOperation,
    ) -> Result<RoutedBusinessReceipt, FabricError>;
}

/// Target-node persistence seam for a remote replica of the existing Wave 4C
/// Message. The route journal cannot create delivery authority; callers must
/// receive a successfully persisted replica before invoking canonical delivery.
pub trait RemoteMessageReplicaPort {
    fn fetch_message_object(&self, message_object_ref: &str) -> Result<Vec<u8>, FabricError>;

    fn persist_remote_replica(
        &self,
        replica: &RemoteMessageReplica,
    ) -> Result<RemoteMessageReplica, FabricError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessageReplicaExpectation {
    pub source_execution_space_id: String,
    pub message_id: String,
    pub schema_version: u64,
    pub content_fingerprint: String,
    pub body_digest: String,
    pub persisted_at: String,
}

fn direct_fabric_error(
    code: FabricErrorCode,
    message: impl Into<String>,
    resource_kind: &str,
    resource_id: &str,
) -> FabricError {
    FabricError {
        code,
        message: message.into(),
        retryable: false,
        effect_certainty: FabricEffectCertainty::None,
        resource_kind: resource_kind.into(),
        resource_id: resource_id.into(),
        current_revision: None,
    }
}

fn immutable_message_fingerprint(message: &Message) -> String {
    canonical_json_fingerprint(&serde_json::json!({
        "sender_actor_ref": message.sender_actor_ref,
        "sender_agent_id": message.sender_agent_id,
        "sender_session_id": message.sender_session_id,
        "address_kind": message.address_kind,
        "target_ref": message.target_ref,
        "recipients": message.recipients,
        "team_id": message.team_id,
        "team_run_id": message.team_run_id,
        "work_id": message.work_id,
        "collaboration_scope": message.collaboration_scope,
        "kind": message.kind,
        "body": message.body,
        "body_digest": message.body_digest,
        "correlation_id": message.correlation_id,
        "causation_id": message.causation_id,
        "response_intent": message.response_intent,
        "evidence_refs": message.evidence_refs,
        "schema_version": message.schema_version,
        "idempotency_key": message.idempotency_key,
    }))
}

pub fn validate_message_collaboration_scope(message: &Message) -> StoreResult<()> {
    let Some(scope) = message.collaboration_scope.as_ref() else {
        return Ok(());
    };
    let source_work_valid = scope.source_work_ref.as_ref().is_none_or(|source| {
        source.execution_space_id == message.source_execution_space_id
            && source.node_id == message.source_node_id
            && source.team_id == scope.source_team_id
            && source.placement_generation == 1
            && message.work_id.as_ref() == Some(&source.work_id)
    });
    let target_work_valid = scope.target_work_ref.as_ref().is_none_or(|target| {
        target.team_id == scope.target_team_id && target.placement_generation == 1
    });
    if scope.source_team_id.trim().is_empty()
        || scope.target_team_id.trim().is_empty()
        || scope.source_team_id == scope.target_team_id
        || message.team_id.as_ref() != Some(&scope.source_team_id)
        || !source_work_valid
        || !target_work_valid
        || (scope.expected_delegation_revision.is_some() && scope.delegation_id.is_none())
    {
        return Err(collaboration_error(
            FabricErrorCode::MessageRecipientUnauthorized,
            "Message CollaborationScope is outside the exact source Team/Work and target Team authority",
            "message",
            &message.id,
            None,
        ));
    }
    Ok(())
}

/// Resolve, authenticate and durably persist one immutable Message replica on
/// the target Node. A digest-only transfer is impossible by construction.
pub fn persist_verified_remote_message_replica<P: RemoteMessageReplicaPort>(
    port: &P,
    payload: &ImmutableMessageTransferPayload,
    expected: &RemoteMessageReplicaExpectation,
) -> Result<RemoteMessageReplica, FabricError> {
    let bytes = match payload {
        ImmutableMessageTransferPayload::CanonicalBytes {
            canonical_message_bytes,
        } => canonical_message_bytes.clone(),
        ImmutableMessageTransferPayload::MessageObjectRef {
            message_object_ref,
            authenticated_content_digest,
        } => {
            let bytes = port.fetch_message_object(message_object_ref)?;
            let value = serde_json::from_slice::<Value>(&bytes).map_err(|_| {
                direct_fabric_error(
                    FabricErrorCode::MessageReplicaMismatch,
                    "message object is not a valid immutable Message payload",
                    "message_object_ref",
                    message_object_ref,
                )
            })?;
            if canonical_json_fingerprint(&value) != *authenticated_content_digest {
                return Err(direct_fabric_error(
                    FabricErrorCode::MessageReplicaMismatch,
                    "message object content-addressed digest does not match",
                    "message_object_ref",
                    message_object_ref,
                ));
            }
            bytes
        }
    };
    let message = serde_json::from_slice::<Message>(&bytes).map_err(|_| {
        direct_fabric_error(
            FabricErrorCode::MessageReplicaMismatch,
            "cross-node payload is not the canonical Wave 4C Message schema",
            "message",
            &expected.message_id,
        )
    })?;
    let body_digest = format!(
        "sha256:{}",
        firm_fabric::sha256_hex(message.body.as_bytes())
    );
    if message.source_execution_space_id != expected.source_execution_space_id
        || message.id != expected.message_id
        || message.schema_version != expected.schema_version
        || message.content_fingerprint != expected.content_fingerprint
        || message.body_digest != expected.body_digest
        || message.content_fingerprint != immutable_message_fingerprint(&message)
        || message.body_digest != body_digest
    {
        return Err(direct_fabric_error(
            FabricErrorCode::MessageReplicaMismatch,
            "immutable Message identity, schema, body digest, or content fingerprint changed in transit",
            "message",
            &expected.message_id,
        ));
    }
    let replica = RemoteMessageReplica {
        source_execution_space_id: message.source_execution_space_id,
        message_id: message.id,
        schema_version: message.schema_version,
        content_fingerprint: message.content_fingerprint,
        body_digest: message.body_digest,
        canonical_message_bytes: bytes,
        persisted_at: expected.persisted_at.clone(),
    };
    port.persist_remote_replica(&replica)
}

/// Build the non-authoritative source outbox row allowed while the Control
/// Plane is offline. The Message already exists; this function cannot create a
/// Delegation, Decision, publication, or replacement Message identity.
pub fn queue_remote_message_transfer(
    message: &Message,
    target_placement: &TargetPlacementRef,
    payload: ImmutableMessageTransferPayload,
    queued_at: &str,
) -> Result<SourceRemoteMessageTransfer, FabricError> {
    let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
        direct_fabric_error(
            FabricErrorCode::MessageRecipientUnauthorized,
            "cross-node transfer requires the immutable Message CollaborationScope",
            "message",
            &message.id,
        )
    })?;
    if target_placement.placement_generation != 1
        || scope.target_team_id != target_placement.team_id
        || scope.source_team_id == scope.target_team_id
        || message.source_node_id == target_placement.node_id
        || message.content_fingerprint != immutable_message_fingerprint(message)
    {
        return Err(direct_fabric_error(
            FabricErrorCode::TargetTeamPlacementChanged,
            "remote Message transfer does not match immutable v1 Team placement and authored scope",
            "message",
            &message.id,
        ));
    }
    Ok(SourceRemoteMessageTransfer {
        id: format!("remote-message-transfer:{}", message.id),
        source_execution_space_id: message.source_execution_space_id.clone(),
        source_node_id: message.source_node_id.clone(),
        source_node_daemon_generation: message.source_authority_generation,
        message_id: message.id.clone(),
        message_schema_version: message.schema_version,
        content_fingerprint: message.content_fingerprint.clone(),
        body_digest: message.body_digest.clone(),
        target_placement: target_placement.clone(),
        payload,
        state: RemoteMessageTransferState::QueuedForControlPlane,
        queued_at: queued_at.into(),
    })
}

/// Transport-neutral business orchestration. The fabric may route/reconcile
/// the effect, but only this service folds an applied receipt into the central
/// Delegation relationship. Unknown/failed effects never fabricate Work or
/// Delegation outcomes.
pub struct CollaborationApplicationService<'a, F> {
    store: &'a HarnessStore,
    fabric: &'a F,
    control_plane_actor: &'a ActorRef,
}

impl<'a, F: CollaborationFabricPort> CollaborationApplicationService<'a, F> {
    pub fn new(store: &'a HarnessStore, fabric: &'a F, control_plane_actor: &'a ActorRef) -> Self {
        Self {
            store,
            fabric,
            control_plane_actor,
        }
    }

    pub fn provision_target_work(
        &self,
        fold_context: &CollaborationMutationContext,
        delegation_id: &str,
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        let routed = self.store.target_work_create_operation(
            &fold_context.company_id,
            delegation_id,
            &fold_context.occurred_at,
        )?;
        let receipt = self.fabric.dispatch(&routed).map_err(|error| {
            StoreError::Conflict(
                serde_json::to_string(&error)
                    .unwrap_or_else(|_| "routed collaboration operation failed".into()),
            )
        })?;
        if receipt.operation_id != routed.id
            || receipt.kind != routed.kind
            || receipt.target_node_id != routed.target_placement.node_id
            || receipt.target_placement_generation != routed.target_placement.placement_generation
            || receipt.effect_certainty != FabricEffectCertainty::Applied
            || receipt.result_digest != canonical_json_fingerprint(&receipt.result)
        {
            return Err(collaboration_error(
                FabricErrorCode::RecoveryRequired,
                "fabric receipt is unknown, forged, or outside the exact routed operation",
                "routed_operation",
                &routed.id,
                Some(routed.expected_revision),
            ));
        }
        let target_work_ref = serde_json::from_value::<RemoteWorkRef>(
            receipt
                .result
                .get("target_work_ref")
                .cloned()
                .ok_or_else(|| {
                    collaboration_error(
                        FabricErrorCode::TargetWorkCreateFailed,
                        "applied target Work receipt lacks target_work_ref",
                        "routed_operation",
                        &routed.id,
                        Some(routed.expected_revision),
                    )
                })?,
        )?;
        self.store.apply_target_work_created(
            fold_context,
            delegation_id,
            &target_work_ref,
            observed_target_placement,
            &receipt.operation_id,
            self.control_plane_actor,
        )
    }
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

fn source_work_attestation_digest(attestation: &SourceWorkAttestation) -> StoreResult<String> {
    Ok(canonical_json_fingerprint(&serde_json::json!({
        "id": attestation.id,
        "company_id": attestation.company_id,
        "source_work_ref": attestation.source_work_ref,
        "source_owner_ref": attestation.source_owner_ref,
        "source_host_ref": attestation.source_host_ref,
        "work_application_service_ref": attestation.work_application_service_ref,
        "source_gateway_generation": attestation.source_gateway_generation,
        "issued_at": attestation.issued_at,
    })))
}

fn exact_actor(actual: &ActorRef, expected: &ActorRef) -> bool {
    actual == expected && !actual.id.trim().is_empty()
}

impl HarnessStore {
    pub fn list_collaboration_delegations_for_actor(
        &self,
        company_id: &str,
        actor: &ActorRef,
        filter: &CollaborationDelegationFilter,
        cursor: Option<CollaborationScopedCursor>,
        limit: usize,
    ) -> StoreResult<CollaborationScopedPage<WorkDelegationV1>> {
        if limit == 0 || limit > 500 {
            return Err(StoreError::Conflict(
                "COLLABORATION_CURSOR_INVALID: limit must be between 1 and 500".into(),
            ));
        }
        let operations = self.collaboration_operations_unlocked()?;
        let latest_sequence = operations
            .iter()
            .map(|row| row.store_sequence)
            .max()
            .unwrap_or(0);
        let actor_digest = canonical_json_fingerprint(&serde_json::to_value(actor)?);
        let filter_digest = canonical_json_fingerprint(&serde_json::to_value(filter)?);
        let as_of = cursor
            .as_ref()
            .map(|value| value.as_of_store_sequence)
            .unwrap_or(latest_sequence);
        if as_of > latest_sequence
            || cursor.as_ref().is_some_and(|value| {
                value.company_id != company_id
                    || value.actor_digest != actor_digest
                    || value.filter_digest != filter_digest
            })
        {
            return Err(StoreError::Conflict(
                "COLLABORATION_CURSOR_SCOPE_MISMATCH: cursor actor, Company, filter or snapshot is invalid".into(),
            ));
        }
        let mut attestations = BTreeMap::<String, SourceWorkAttestation>::new();
        let mut latest = BTreeMap::<String, WorkDelegationV1>::new();
        for operation in operations
            .into_iter()
            .filter(|row| row.company_id == company_id && row.store_sequence <= as_of)
        {
            if operation.aggregate_kind == "source_work_attestation" {
                let value: SourceWorkAttestation =
                    serde_json::from_value(operation.resulting_projection)?;
                attestations.insert(value.id.clone(), value);
            } else if operation.aggregate_kind == "work_delegation_v1" {
                let value: WorkDelegationV1 =
                    serde_json::from_value(operation.resulting_projection)?;
                latest.insert(value.id.clone(), value);
            }
        }
        let rows = latest.into_values().collect::<Vec<_>>();
        let mut raw_offset = cursor.as_ref().map(|value| value.raw_offset).unwrap_or(0);
        let mut visible_progress = cursor
            .as_ref()
            .map(|value| value.visible_progress)
            .unwrap_or(0);
        if raw_offset > rows.len() {
            return Err(StoreError::Conflict(
                "COLLABORATION_CURSOR_INVALID: raw offset is outside the frozen snapshot".into(),
            ));
        }
        let mut items = Vec::new();
        // Bound raw work independently of visible results. A page containing
        // only rows hidden from this actor is still a valid advancing page;
        // clients follow its opaque cursor until visible rows or EOF. This
        // prevents a hostile Company history from turning one scoped request
        // into an unbounded scan without letting hidden rows consume the
        // caller's visible item limit.
        let raw_scan_budget = limit.saturating_mul(4).max(limit);
        let raw_scan_end = raw_offset.saturating_add(raw_scan_budget).min(rows.len());
        while raw_offset < raw_scan_end && items.len() < limit {
            let delegation = &rows[raw_offset];
            raw_offset += 1;
            let source_host = attestations
                .get(&delegation.source_work_attestation_id)
                .map(|value| &value.source_host_ref);
            let visible = actor == &delegation.source_owner_ref
                || source_host == Some(actor)
                || actor == &delegation.target_host_ref;
            let matches = filter
                .source_team_id
                .as_ref()
                .is_none_or(|value| value == &delegation.source_team_id)
                && filter
                    .target_team_id
                    .as_ref()
                    .is_none_or(|value| value == &delegation.target_placement.team_id)
                && filter.node_id.as_ref().is_none_or(|value| {
                    value == &delegation.source_node_id
                        || value == &delegation.target_placement.node_id
                })
                && filter.state.is_none_or(|value| value == delegation.state);
            if visible && matches {
                items.push(delegation.clone());
                visible_progress += 1;
            }
        }
        Ok(CollaborationScopedPage {
            items,
            as_of_store_sequence: as_of,
            next_cursor: (raw_offset < rows.len()).then_some(CollaborationScopedCursor {
                company_id: company_id.into(),
                actor_digest,
                filter_digest,
                as_of_store_sequence: as_of,
                raw_offset,
                visible_progress,
            }),
        })
    }
    pub fn collaboration_artifact_import(
        &self,
        company_id: &str,
        artifact_id: &str,
    ) -> StoreResult<Option<ArtifactImport>> {
        self.latest_collaboration_projection_unlocked(company_id, "artifact_import", artifact_id)
    }

    pub fn read_collaboration_artifact_import_bytes(
        &self,
        company_id: &str,
        artifact_id: &str,
    ) -> StoreResult<Vec<u8>> {
        let import = self
            .collaboration_artifact_import(company_id, artifact_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!("ARTIFACT_IMPORT_NOT_FOUND: {artifact_id}"))
            })?;
        let path = self
            .root()
            .join("collaboration-artifact-imports")
            .join(&import.artifact_digest);
        let bytes = std::fs::read(path)?;
        if bytes.len() as u64 != import.size_bytes
            || firm_fabric::sha256_hex(&bytes) != import.artifact_digest
        {
            return Err(StoreError::Conflict(
                "ARTIFACT_IMPORT_TAMPERED: imported bytes disagree with the canonical import"
                    .into(),
            ));
        }
        Ok(bytes)
    }

    pub fn persist_collaboration_artifact_import(
        &self,
        context: &CollaborationMutationContext,
        import: &ArtifactImport,
        bytes: &[u8],
    ) -> StoreResult<CollaborationMutationResult<ArtifactImport>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &import.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "artifact import requires the exact current Delegation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        let attestation = self
            .latest_collaboration_projection_unlocked::<SourceWorkAttestation>(
                &context.company_id,
                "source_work_attestation",
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "artifact import requires the exact source Work attestation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        if import.company_id != context.company_id
            || import.revision != 1
            || context.authenticated_actor.kind != ActorKind::Service
            || context.authenticated_actor.id != import.source_node_daemon_id
            || import.source_node_id != delegation.source_node_id
            || import.source_node_daemon_id.trim().is_empty()
            || import.source_node_daemon_generation == 0
            || import.source_team_id != delegation.source_team_id
            || import.source_host_ref != attestation.source_host_ref
            || import.source_work_ref != delegation.source_work_ref
            || import.size_bytes != bytes.len() as u64
            || import.artifact_digest != firm_fabric::sha256_hex(bytes)
        {
            return Err(collaboration_error(
                FabricErrorCode::ArtifactScopeUnauthorized,
                "artifact import bytes or source authority disagree with the current Delegation",
                "artifact_import",
                &import.artifact_id,
                None,
            ));
        }
        if let Some(existing) = self.latest_collaboration_projection_unlocked::<ArtifactImport>(
            &context.company_id,
            "artifact_import",
            &import.artifact_id,
        )? {
            if existing != *import {
                return Err(collaboration_error(
                    FabricErrorCode::IdempotencyConflict,
                    "artifact import replay changed immutable bytes or authority",
                    "artifact_import",
                    &import.artifact_id,
                    Some(existing.revision),
                ));
            }
            return self.commit_collaboration_projection_unlocked(
                context,
                "artifact_import",
                &import.artifact_id,
                serde_json::to_value(&existing)?,
                &existing,
                Vec::new(),
            );
        }
        let directory = self.root().join("collaboration-artifact-imports");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(&import.artifact_digest);
        let next = directory.join(format!("{}.next", import.artifact_digest));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&next)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&next, &path)?;
        std::fs::File::open(&directory)?.sync_all()?;
        self.commit_collaboration_projection_unlocked(
            context,
            "artifact_import",
            &import.artifact_id,
            serde_json::to_value(import)?,
            import,
            Vec::new(),
        )
    }

    /// Fold a target Node's terminal import into central relationship state.
    /// Artifact bytes remain solely in the source Execution Space.
    pub fn record_collaboration_artifact_import(
        &self,
        context: &CollaborationMutationContext,
        import: &ArtifactImport,
        routed_operation_id: &str,
        resolved_control_plane_actor: &ActorRef,
    ) -> StoreResult<CollaborationMutationResult<ArtifactImport>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &import.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "artifact import result references no central Delegation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        let attestation = self
            .latest_collaboration_projection_unlocked::<SourceWorkAttestation>(
                &context.company_id,
                "source_work_attestation",
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "artifact import result has no source Work attestation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        if !exact_actor(&context.authenticated_actor, resolved_control_plane_actor)
            || import.company_id != context.company_id
            || import.operation_id != routed_operation_id
            || import.source_node_id != delegation.source_node_id
            || import.source_team_id != delegation.source_team_id
            || import.source_host_ref != attestation.source_host_ref
            || import.source_work_ref != delegation.source_work_ref
            || import.source_node_daemon_id.trim().is_empty()
            || import.source_node_daemon_generation == 0
            || import.revision != 1
        {
            return Err(collaboration_error(
                FabricErrorCode::ArtifactScopeUnauthorized,
                "artifact import result changed Delegation, source Host/Work/Node, operation, or daemon generation",
                "artifact_import",
                &import.artifact_id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "artifact_import",
            &import.artifact_id,
            serde_json::to_value(import)?,
            import,
            Vec::new(),
        )
    }

    /// Hold the canonical collaboration writer lock across a caller-supplied
    /// authority check and its downstream durable commit. This is the only
    /// supported lock order for cross-store routing: collaboration first,
    /// Fabric second. The callback cannot mutate collaboration state.
    #[allow(clippy::result_large_err)]
    pub fn with_collaboration_authority_fence<T>(
        &self,
        validate: impl FnOnce(&Self) -> Result<(), firm_fabric::FabricError>,
        commit: impl FnOnce() -> Result<T, firm_fabric::FabricError>,
    ) -> Result<T, firm_fabric::FabricError> {
        self.init().map_err(|error| {
            firm_fabric::FabricError::none(
                firm_fabric::FabricErrorCode::StoreUnavailable,
                error.to_string(),
            )
        })?;
        let _lock = self.acquire_write_lock().map_err(|error| {
            firm_fabric::FabricError::none(
                firm_fabric::FabricErrorCode::StoreUnavailable,
                error.to_string(),
            )
        })?;
        validate(self)?;
        commit()
    }

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

    fn latest_cancellation_request_unlocked(
        &self,
        company_id: &str,
        delegation_id: &str,
        request_id: &str,
    ) -> StoreResult<Option<DelegationCancellationRequest>> {
        let mut latest = None;
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id != company_id
                || operation.aggregate_kind != "work_delegation_v1"
                || operation.aggregate_id != delegation_id
            {
                continue;
            }
            for record in operation.immutable_side_records {
                let Ok(request) = serde_json::from_value::<DelegationCancellationRequest>(record)
                else {
                    continue;
                };
                if request.id == request_id
                    && latest
                        .as_ref()
                        .is_none_or(|current: &DelegationCancellationRequest| {
                            request.revision > current.revision
                        })
                {
                    latest = Some(request);
                }
            }
        }
        Ok(latest)
    }

    pub fn collaboration_cancellation_requests(
        &self,
        company_id: &str,
        delegation_id: &str,
    ) -> StoreResult<Vec<DelegationCancellationRequest>> {
        let mut latest = BTreeMap::<String, DelegationCancellationRequest>::new();
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id != company_id
                || operation.aggregate_kind != "work_delegation_v1"
                || operation.aggregate_id != delegation_id
            {
                continue;
            }
            for record in operation.immutable_side_records {
                let Ok(request) = serde_json::from_value::<DelegationCancellationRequest>(record)
                else {
                    continue;
                };
                if latest
                    .get(&request.id)
                    .is_none_or(|current| request.revision > current.revision)
                {
                    latest.insert(request.id.clone(), request);
                }
            }
        }
        Ok(latest.into_values().collect())
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

    pub fn collaboration_delegation(
        &self,
        company_id: &str,
        delegation_id: &str,
    ) -> StoreResult<Option<WorkDelegationV1>> {
        self.latest_collaboration_projection_unlocked(
            company_id,
            "work_delegation_v1",
            delegation_id,
        )
    }

    pub fn collaboration_source_work_attestation(
        &self,
        company_id: &str,
        attestation_id: &str,
    ) -> StoreResult<Option<SourceWorkAttestation>> {
        self.latest_collaboration_projection_unlocked(
            company_id,
            "source_work_attestation",
            attestation_id,
        )
    }

    pub fn collaboration_inbound_policy(
        &self,
        company_id: &str,
        policy_id: &str,
    ) -> StoreResult<Option<DelegationInboundPolicy>> {
        self.latest_collaboration_projection_unlocked(
            company_id,
            "delegation_inbound_policy",
            policy_id,
        )
    }

    pub fn collaboration_cancellation_request(
        &self,
        company_id: &str,
        delegation_id: &str,
        request_id: &str,
    ) -> StoreResult<Option<DelegationCancellationRequest>> {
        self.latest_cancellation_request_unlocked(company_id, delegation_id, request_id)
    }

    pub fn collaboration_publications(
        &self,
        company_id: &str,
        delegation_id: &str,
    ) -> StoreResult<Vec<RemoteFactPublication>> {
        let mut latest = BTreeMap::<String, RemoteFactPublication>::new();
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id != company_id
                || operation.aggregate_kind != "remote_fact_publication"
            {
                continue;
            }
            let Ok(publication) =
                serde_json::from_value::<RemoteFactPublication>(operation.resulting_projection)
            else {
                continue;
            };
            if publication.delegation_id != delegation_id {
                continue;
            }
            if latest
                .get(&publication.id)
                .is_none_or(|current| publication.fact_revision > current.fact_revision)
            {
                latest.insert(publication.id.clone(), publication);
            }
        }
        Ok(latest.into_values().collect())
    }

    /// Persist a server-authored proof of exact source Work authority. The
    /// source WorkApplicationService is the only writer; a Host request can
    /// subsequently reference only this immutable attestation ID.
    pub fn put_source_work_attestation(
        &self,
        context: &CollaborationMutationContext,
        attestation: &SourceWorkAttestation,
        resolved_work_application_service: &ActorRef,
        current_source_gateway_generation: u64,
    ) -> StoreResult<CollaborationMutationResult<SourceWorkAttestation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if context.expected_revision != 0
            || context.company_id != attestation.company_id
            || !exact_actor(
                &context.authenticated_actor,
                resolved_work_application_service,
            )
            || attestation.work_application_service_ref != *resolved_work_application_service
            || attestation.source_gateway_generation != current_source_gateway_generation
            || current_source_gateway_generation == 0
            || attestation.source_work_ref.placement_generation != 1
            || attestation.source_work_ref.team_id.is_empty()
            || attestation.source_work_ref.node_id.is_empty()
            || attestation.attestation_digest != source_work_attestation_digest(attestation)?
        {
            return Err(collaboration_error(
                FabricErrorCode::SourceWorkAttestationInvalid,
                "source Work attestation is not server-authored for the exact current Work, Team, and gateway generation",
                "source_work_attestation",
                &attestation.id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "source_work_attestation",
            &attestation.id,
            serde_json::to_value(attestation)?,
            attestation,
            Vec::new(),
        )
    }

    pub fn list_collaboration_delegations(
        &self,
        company_id: &str,
        filter: &CollaborationDelegationFilter,
        cursor: Option<CollaborationCursor>,
        limit: usize,
    ) -> StoreResult<CollaborationPage<WorkDelegationV1>> {
        if limit == 0 || limit > 500 {
            return Err(collaboration_error(
                FabricErrorCode::ProtocolMismatch,
                "collaboration list limit must be between 1 and 500",
                "collaboration_cursor",
                company_id,
                None,
            ));
        }
        let operations = self.collaboration_operations_unlocked()?;
        let latest_sequence = operations
            .iter()
            .map(|operation| operation.store_sequence)
            .max()
            .unwrap_or(0);
        let as_of = cursor
            .map(|value| value.as_of_store_sequence)
            .unwrap_or(latest_sequence);
        if as_of > latest_sequence {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "collaboration cursor points beyond the current Store sequence",
                "collaboration_cursor",
                company_id,
                Some(latest_sequence),
            ));
        }
        let mut latest = BTreeMap::<String, WorkDelegationV1>::new();
        for operation in operations.into_iter().filter(|operation| {
            operation.company_id == company_id
                && operation.aggregate_kind == "work_delegation_v1"
                && operation.store_sequence <= as_of
        }) {
            let delegation: WorkDelegationV1 =
                serde_json::from_value(operation.resulting_projection)?;
            latest.insert(delegation.id.clone(), delegation);
        }
        let filtered = latest
            .into_values()
            .filter(|delegation| {
                filter
                    .source_team_id
                    .as_ref()
                    .is_none_or(|team_id| &delegation.source_team_id == team_id)
                    && filter
                        .target_team_id
                        .as_ref()
                        .is_none_or(|team_id| &delegation.target_placement.team_id == team_id)
                    && filter.node_id.as_ref().is_none_or(|node_id| {
                        &delegation.source_node_id == node_id
                            || &delegation.target_placement.node_id == node_id
                    })
                    && filter.state.is_none_or(|state| delegation.state == state)
            })
            .collect::<Vec<_>>();
        let offset = cursor.map(|value| value.offset).unwrap_or(0);
        if offset > filtered.len() {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "collaboration cursor offset is outside the frozen snapshot",
                "collaboration_cursor",
                company_id,
                Some(as_of),
            ));
        }
        let items = filtered
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset + items.len();
        Ok(CollaborationPage {
            items,
            as_of_store_sequence: as_of,
            next_cursor: (next_offset < filtered.len()).then_some(CollaborationCursor {
                as_of_store_sequence: as_of,
                offset: next_offset,
            }),
        })
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
            store_sequence: operations
                .iter()
                .map(|operation| operation.store_sequence)
                .max()
                .unwrap_or(0)
                + 1,
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
            (
                &request.source_work_attestation_id,
                "source_work_attestation_id",
            ),
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
        let attestation = self
            .latest_collaboration_projection_unlocked::<SourceWorkAttestation>(
                &context.company_id,
                "source_work_attestation",
                &request.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "delegation proposal requires a canonical source Work attestation",
                    "source_work_attestation",
                    &request.source_work_attestation_id,
                    None,
                )
            })?;
        if attestation.attestation_digest != source_work_attestation_digest(&attestation)?
            || attestation.work_application_service_ref != authority.source_work_application_service
            || attestation.source_gateway_generation != authority.source_gateway_generation
            || attestation.source_host_ref != authority.source_host
            || attestation.source_owner_ref != authority.source_work_owner
        {
            return Err(collaboration_error(
                FabricErrorCode::SourceWorkAttestationInvalid,
                "canonical source Work attestation is stale or outside the server-resolved source authority",
                "source_work_attestation",
                &attestation.id,
                None,
            ));
        }
        if !exact_actor(&context.authenticated_actor, &attestation.source_host_ref)
            && !exact_actor(&context.authenticated_actor, &attestation.source_owner_ref)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host or source Work owner may propose",
                "work_delegation_v1",
                &request.delegation_id,
                None,
            ));
        }
        if attestation.source_work_ref.team_id == request.target_placement.team_id
            || request.target_placement != authority.target_placement
            || request.target_placement.placement_generation != 1
            || attestation.source_work_ref.placement_generation != 1
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
            || policy.source_team_id != attestation.source_work_ref.team_id
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
                delegation.source_team_id == attestation.source_work_ref.team_id
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
            source_work_attestation_id: attestation.id.clone(),
            source_work_ref: attestation.source_work_ref.clone(),
            source_owner_ref: attestation.source_owner_ref.clone(),
            source_team_id: attestation.source_work_ref.team_id.clone(),
            source_node_id: attestation.source_work_ref.node_id.clone(),
            target_placement: request.target_placement.clone(),
            target_host_ref: authority.target_host.clone(),
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
            authenticated_actor: delegation.target_host_ref,
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

    /// Build the source Node-authored proposal envelope from the immutable
    /// WorkApplicationService attestation. The public request contributes only
    /// desired outcome and target identity; it cannot select Work/owner facts.
    pub fn delegation_propose_operation(
        &self,
        context: &CollaborationMutationContext,
        request: &ProposeDelegationRequest,
        policy_id: &str,
    ) -> StoreResult<RoutedBusinessOperation> {
        let attestation = self
            .collaboration_source_work_attestation(
                &context.company_id,
                &request.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "delegation route requires the server-authored source Work attestation",
                    "source_work_attestation",
                    &request.source_work_attestation_id,
                    None,
                )
            })?;
        if context.expected_revision != 0
            || request.target_placement.placement_generation != 1
            || attestation.source_work_ref.node_id == request.target_placement.node_id
            || (!exact_actor(&context.authenticated_actor, &attestation.source_host_ref)
                && !exact_actor(&context.authenticated_actor, &attestation.source_owner_ref))
            || policy_id.trim().is_empty()
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "delegation route is outside exact source Work actor or v1 target placement",
                "work_delegation_v1",
                &request.delegation_id,
                None,
            ));
        }
        let payload = serde_json::json!({
            "request": request,
            "source_work_attestation": attestation,
            "policy_id": policy_id,
        });
        Ok(RoutedBusinessOperation {
            id: request.operation_id.clone(),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationPropose,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: attestation.source_work_ref.node_id,
            target_placement: request.target_placement.clone(),
            expected_revision: 0,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationPropose.required_capability(),
            ordering_key: format!("delegation:{}", request.delegation_id),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn delegation_decide_operation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        decision: &DelegationDecision,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation decision route requires the central relationship",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || decision.expected_delegation_revision != delegation.revision
            || decision.delegation_id != delegation.id
            || !exact_actor(&context.authenticated_actor, &delegation.target_host_ref)
            || decision.decided_by_target_host != delegation.target_host_ref
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "delegation decision route requires exact target Host and relationship revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "decision": decision,
            "target_placement": delegation.target_placement,
        });
        Ok(RoutedBusinessOperation {
            id: decision.id.clone(),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationDecide,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationDecide.required_capability(),
            ordering_key: format!("delegation:{delegation_id}"),
            created_at: context.occurred_at.clone(),
        })
    }

    /// Target WorkApplicationService publishes only a redacted immutable fact
    /// whose native Work identity is proven by the local target Store. The
    /// Company registry will independently re-check Delegation scope.
    pub fn remote_fact_publish_operation(
        &self,
        context: &CollaborationMutationContext,
        publication: &RemoteFactPublication,
        source_team_placement: &TargetPlacementRef,
        current_node_id: &str,
    ) -> StoreResult<RoutedBusinessOperation> {
        let work = self
            .latest_works()?
            .into_iter()
            .find(|work| work.id == publication.native_fact_work_ref.work_id)
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::PublicationScopeMismatch,
                    "remote publication references no target-owned native Work",
                    "remote_fact_publication",
                    &publication.id,
                    None,
                )
            })?;
        let team = self
            .teams()?
            .into_iter()
            .rev()
            .find(|team| team.id == publication.origin_team_id)
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::PublicationScopeMismatch,
                    "remote publication references no target-owned AgentTeam",
                    "remote_fact_publication",
                    &publication.id,
                    None,
                )
            })?;
        let actor_is_host = context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == team.host_agent_id;
        let actor_is_owner = context.authenticated_actor.kind == ActorKind::AgentMember
            && work.owner_member_id.as_deref() == Some(context.authenticated_actor.id.as_str());
        let accepted_result_revision =
            publication
                .operational_decision_ref
                .as_ref()
                .is_some_and(|decision| {
                    decision.work_ref == publication.native_fact_work_ref
                        && work.version == publication.native_fact_work_ref.work_revision + 1
                        && work.phase == firm_core::WorkPhase::Closed
                        && work.resolution == Some(firm_core::WorkResolution::Accepted)
                });
        let canonical_digest =
            canonical_json_fingerprint(&publication.snapshot.canonical_redacted_fact);
        if context.expected_revision == 0
            || publication.fact_revision == 0
            || publication.company_id != context.company_id
            || publication.origin_node_id != current_node_id
            || publication.native_fact_work_ref.node_id != current_node_id
            || publication.native_fact_work_ref.team_id != team.id
            || publication.native_fact_work_ref.work_id != publication.fact_work_ref.work_id
            || publication.native_fact_work_ref.node_id != publication.fact_work_ref.node_id
            || publication.native_fact_work_ref.team_id != publication.fact_work_ref.team_id
            || publication.native_fact_work_ref.placement_generation
                != publication.fact_work_ref.placement_generation
            || (publication.native_fact_work_ref.work_revision != work.version
                && !accepted_result_revision)
            || publication.fact_digest != canonical_digest
            || publication.snapshot.canonical_digest != canonical_digest
            || publication.snapshot.publication_id != publication.id
            || publication.created_by != context.authenticated_actor
            || (!actor_is_host && !actor_is_owner)
            || source_team_placement.team_id != publication.delegation_source_work_ref.team_id
            || source_team_placement.node_id != publication.delegation_source_work_ref.node_id
            || source_team_placement.placement_generation != 1
            || source_team_placement.node_id == current_node_id
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "remote publication is not server-bound to the exact local Work/actor or source Team placement",
                "remote_fact_publication",
                &publication.id,
                Some(work.version),
            ));
        }
        let payload = serde_json::json!({
            "publication": publication,
            "source_team_placement": source_team_placement,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-publication:{}", publication.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::RemoteFactPublish,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: current_node_id.into(),
            target_placement: source_team_placement.clone(),
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::RemoteFactPublish.required_capability(),
            ordering_key: format!("delegation:{}", publication.delegation_id),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn delegation_cancel_request_operation(
        &self,
        context: &CollaborationMutationContext,
        request: &DelegationCancellationRequest,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, &request.delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "cancellation route requires the central Delegation",
                    "work_delegation_v1",
                    &request.delegation_id,
                    None,
                )
            })?;
        let source_attestation = self
            .collaboration_source_work_attestation(
                &context.company_id,
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "cancellation route has no source Host attestation",
                    "source_work_attestation",
                    &delegation.source_work_attestation_id,
                    None,
                )
            })?;
        if delegation.state != DelegationState::CancellationRequested
            || delegation.revision != request.expected_delegation_revision.saturating_add(1)
            || context.expected_revision != delegation.revision
            || request.requested_by != context.authenticated_actor
            || !exact_actor(
                &context.authenticated_actor,
                &source_attestation.source_host_ref,
            )
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "cancellation request requires the exact source actor and Delegation revision",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "request": request,
            "target_placement": delegation.target_placement,
            "target_work_ref": delegation.target_work_ref,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-cancellation-request:{}", request.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationCancelRequest,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: request.expected_delegation_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationCancelRequest.required_capability(),
            ordering_key: format!("delegation:{}", request.delegation_id),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn delegation_cancel_decide_operation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        request_id: &str,
        decision: &DelegationCancellationDecision,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "cancellation decision route requires the central Delegation",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        let request = self
            .collaboration_cancellation_request(&context.company_id, delegation_id, request_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::CancellationDecisionRequired,
                    "cancellation decision references no pending request",
                    "delegation_cancellation_request",
                    request_id,
                    None,
                )
            })?;
        if delegation.revision != context.expected_revision
            || request.state != CancellationRequestState::Pending
            || decision.cancellation_request_id != request.id
            || decision.expected_request_revision != request.revision
            || decision.decided_by_target_host != delegation.target_host_ref
            || !exact_actor(&context.authenticated_actor, &delegation.target_host_ref)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "cancellation decision requires exact target Host, pending request, and revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "request_id": request_id,
            "decision": decision,
            "target_placement": delegation.target_placement,
            "target_work_ref": delegation.target_work_ref,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-cancellation-decision:{}", decision.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationCancelDecide,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationCancelDecide.required_capability(),
            ordering_key: format!("delegation:{delegation_id}"),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn artifact_grant_operation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        manifest: &RemoteArtifactManifest,
        capability: &ArtifactCapability,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "artifact grant requires the central Delegation",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        let source_attestation = self
            .collaboration_source_work_attestation(
                &context.company_id,
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "artifact grant has no source Host attestation",
                    "source_work_attestation",
                    &delegation.source_work_attestation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || !matches!(
                delegation.state,
                DelegationState::Active | DelegationState::ResultAvailable
            )
            || !exact_actor(&context.authenticated_actor, &delegation.target_host_ref)
            || manifest.company_id != context.company_id
            || manifest.source_node_id != delegation.target_placement.node_id
            || manifest.source_team_id.as_deref()
                != Some(delegation.target_placement.team_id.as_str())
            || manifest.source_work_id.as_deref()
                != delegation
                    .target_work_ref
                    .as_ref()
                    .map(|work| work.work_id.as_str())
            || manifest.completed_at_unix_ms.is_none()
            || manifest.deleted_at_unix_ms.is_some()
            || !manifest
                .authorized_readers
                .contains(&source_attestation.source_host_ref.id)
            || capability.purpose != ArtifactCapabilityPurpose::Download
            || capability.company_id != context.company_id
            || capability.artifact_id != manifest.id
            || capability.artifact_digest != manifest.sha256
            || capability.node_id != delegation.source_node_id
            || capability.issued_to != source_attestation.source_host_ref.id
        {
            return Err(collaboration_error(
                FabricErrorCode::ArtifactScopeUnauthorized,
                "artifact grant is not bound to the exact Delegation, complete manifest, source Host, and source Node",
                "remote_artifact_manifest",
                &manifest.id,
                Some(manifest.revision),
            ));
        }
        let source_placement = TargetPlacementRef {
            team_id: delegation.source_team_id.clone(),
            team_revision: delegation.source_work_ref.team_revision,
            node_id: delegation.source_node_id.clone(),
            placement_generation: delegation.source_work_ref.placement_generation,
        };
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "manifest": manifest,
            "read_capability": capability,
            "source_placement": source_placement,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-artifact-grant:{}:{}", delegation_id, manifest.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::ArtifactGrant,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.target_placement.node_id,
            target_placement: source_placement,
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::ArtifactGrant.required_capability(),
            ordering_key: format!("delegation:{delegation_id}"),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn apply_target_work_created(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        target_work_ref: &RemoteWorkRef,
        observed_target_placement: &TargetPlacementRef,
        routed_operation_id: &str,
        resolved_control_plane_actor: &ActorRef,
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
        if resolved_control_plane_actor.kind != ActorKind::Service
            || !exact_actor(&context.authenticated_actor, resolved_control_plane_actor)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the server-resolved Control Plane Service may fold an applied routed result",
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
        let mut frozen_request = request.clone();
        frozen_request.state = CancellationRequestState::Pending;
        frozen_request.revision = 1;
        frozen_request.updated_at = context.occurred_at.clone();
        let replay_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(&frozen_request)?);
        if let Some(existing) =
            self.collaboration_operations_unlocked()?
                .into_iter()
                .find(|operation| {
                    operation.company_id == context.company_id
                        && operation.authenticated_actor == context.authenticated_actor
                        && operation.command_name == context.command_name
                        && operation.idempotency_key == context.idempotency_key
                })
        {
            if existing.request_fingerprint != replay_fingerprint
                || existing.aggregate_kind != "work_delegation_v1"
                || existing.aggregate_id != request.delegation_id
            {
                return Err(collaboration_error(
                    FabricErrorCode::IdempotencyConflict,
                    "cancellation request idempotency key changed its fingerprint",
                    "work_delegation_v1",
                    &request.delegation_id,
                    Some(existing.resulting_revision),
                ));
            }
            return Ok(CollaborationMutationResult {
                projection: serde_json::from_value(existing.resulting_projection.clone())?,
                operation: existing,
                replayed: true,
            });
        }
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
        let mut pending_request = self
            .latest_cancellation_request_unlocked(&context.company_id, delegation_id, request_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::CancellationDecisionRequired,
                    "cancellation decision references no canonical pending request",
                    "delegation_cancellation_request",
                    request_id,
                    None,
                )
            })?;
        if pending_request.state != CancellationRequestState::Pending
            || decision.expected_request_revision != pending_request.revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancellation decision does not bind the exact pending request revision",
                "delegation_cancellation_request",
                request_id,
                Some(pending_request.revision),
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
                pending_request.state = CancellationRequestState::Accepted;
            }
            CancellationDecisionKind::Reject => {
                delegation.state = DelegationState::Active;
                pending_request.state = CancellationRequestState::Rejected;
            }
        }
        pending_request.target_host_decision_ref = Some(decision.id.clone());
        pending_request.revision += 1;
        pending_request.updated_at = context.occurred_at.clone();
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
            vec![
                serde_json::to_value(decision)?,
                serde_json::to_value(&pending_request)?,
            ],
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
            || delegation.target_work_ref.as_ref() != Some(&publication.fact_work_ref)
            || publication.native_fact_work_ref.work_id != publication.fact_work_ref.work_id
            || publication.native_fact_work_ref.team_id != publication.fact_work_ref.team_id
            || publication.native_fact_work_ref.node_id != publication.fact_work_ref.node_id
            || publication.native_fact_work_ref.placement_generation
                != publication.fact_work_ref.placement_generation
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

    /// Persist a target-Node-delivered, read-only copy of the central
    /// publication. This aggregate is intentionally a cache and is never
    /// consulted by Company collaboration mutations.
    pub fn persist_remote_fact_cache(
        &self,
        context: &CollaborationMutationContext,
        publication: &RemoteFactPublication,
        routed_operation_id: &str,
        target_placement: &TargetPlacementRef,
        current_node_id: &str,
    ) -> StoreResult<CollaborationMutationResult<RemoteFactPublication>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let canonical_digest =
            canonical_json_fingerprint(&publication.snapshot.canonical_redacted_fact);
        if context.expected_revision != 0
            || context.authenticated_actor.kind != ActorKind::Service
            || routed_operation_id.trim().is_empty()
            || publication.company_id != context.company_id
            || publication.fact_digest != canonical_digest
            || publication.snapshot.canonical_digest != canonical_digest
            || target_placement.node_id != current_node_id
            || target_placement.team_id != publication.delegation_source_work_ref.team_id
            || target_placement.node_id != publication.delegation_source_work_ref.node_id
            || target_placement.placement_generation != 1
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "remote fact cache is not bound to the exact central publication and target placement",
                "remote_fact_cache",
                &publication.id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "remote_fact_cache",
            &publication.id,
            serde_json::json!({
                "publication": publication,
                "routed_operation_id": routed_operation_id,
                "target_placement": target_placement,
            }),
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
        let request_payload = serde_json::json!({
            "publication_id": publication_id,
            "operational_decision": operational_decision,
            "observed_target_placement": observed_target_placement,
        });
        if let Some(existing) =
            self.collaboration_operations_unlocked()?
                .into_iter()
                .find(|operation| {
                    operation.company_id == context.company_id
                        && operation.authenticated_actor == context.authenticated_actor
                        && operation.command_name == context.command_name
                        && operation.idempotency_key == context.idempotency_key
                })
        {
            let projection =
                serde_json::from_value::<WorkDelegationV1>(existing.resulting_projection)?;
            return self.commit_collaboration_projection_unlocked(
                context,
                "work_delegation_v1",
                delegation_id,
                request_payload,
                &projection,
                vec![serde_json::to_value(operational_decision)?],
            );
        }
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
                != publication.native_fact_work_ref.work_revision
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
            request_payload,
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
    remote_replica: &RemoteMessageReplica,
    deliveries: &[CanonicalMessageDelivery],
    routed_operation_id: &str,
    target_gateway_generation: Option<u64>,
    target_observed_sequence: u64,
    observed_at: &str,
) -> StoreResult<Vec<CrossNodeDeliveryProjection>> {
    let persisted_message =
        serde_json::from_slice::<Message>(&remote_replica.canonical_message_bytes)
            .map_err(StoreError::from)?;
    if &persisted_message != message
        || remote_replica.source_execution_space_id != message.source_execution_space_id
        || remote_replica.message_id != message.id
        || remote_replica.schema_version != message.schema_version
        || remote_replica.content_fingerprint != message.content_fingerprint
        || remote_replica.body_digest != message.body_digest
    {
        return Err(collaboration_error(
            FabricErrorCode::MessageReplicaMismatch,
            "canonical deliveries require the exact target-persisted immutable Message replica",
            "message",
            &message.id,
            None,
        ));
    }
    let expected_direct = message
        .recipients
        .iter()
        .filter(|recipient| {
            recipient.kind == firm_core::agentfirm_api::MessageRecipientKind::AgentIdentity
        })
        .map(|recipient| recipient.id.clone())
        .collect::<BTreeSet<_>>();
    let has_team_recipient = message
        .recipients
        .iter()
        .any(|recipient| recipient.kind == firm_core::agentfirm_api::MessageRecipientKind::Team);
    let actual = deliveries
        .iter()
        .map(|delivery| delivery.recipient_identity_id.clone())
        .collect::<BTreeSet<_>>();
    let target_nodes = deliveries
        .iter()
        .map(|delivery| delivery.target_node_id.as_str())
        .collect::<BTreeSet<_>>();
    let recipient_set_valid = if has_team_recipient {
        !actual.is_empty() && expected_direct.is_subset(&actual)
    } else {
        expected_direct == actual
    };
    if !recipient_set_valid || actual.len() != deliveries.len() || target_nodes.len() != 1 {
        return Err(collaboration_error(
            FabricErrorCode::MessageRecipientUnauthorized,
            "per-recipient delivery batch is missing, duplicated, cross-node mixed, or outside the immutable Message/subscription expansion",
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
