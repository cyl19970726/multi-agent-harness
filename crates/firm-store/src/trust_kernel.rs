use crate::{HarnessStore, StoreError, StoreResult};
use firm_core::agentfirm_api::{
    ActorKind, AgentMember, AgentMemberOrganizationStatus, CanonicalMutationEvent,
    CanonicalOperation, DeliveryClaim, DeliveryReconcileOutcome, FailureAnalysis, GateEvaluation,
    GateRequirement, GateVerdict, GateWaiver, GateWaiverState, MemberCoordinationStatus, MemberRun,
    MemberRuntimeStatus, MemberWorkspaceBinding, MessageDelivery, MessageDeliveryStatus,
    MutationContext, ProviderReceipt, TeamMessage, TrustError, TrustErrorCode, WorkDelivery,
    WorkDeliveryStatus, WorkFinding, WorkModuleBinding, WorkReport, WorkReportKind,
    WorkspaceLifecycle, WorkspaceMode, WorkspaceOwnership, WorkspaceSafetyProof,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

const TRUST_OPERATIONS_LEDGER: &str = "agentfirm_trust_operations.jsonl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustOperationEnvelope {
    execution_space_id: String,
    authenticated_actor_kind: ActorKind,
    authenticated_actor_id: String,
    command_name: String,
    operation: CanonicalOperation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalMutationResult<T> {
    pub projection: T,
    pub event: CanonicalMutationEvent,
    pub replayed: bool,
}

fn trust_conflict(error: TrustError) -> StoreError {
    StoreError::Conflict(serde_json::to_string(&error).unwrap_or_else(|_| error.message.clone()))
}

fn trust_error(
    code: TrustErrorCode,
    message: impl Into<String>,
    resource_kind: &str,
    resource_id: &str,
    current_version: Option<u64>,
) -> StoreError {
    trust_conflict(TrustError {
        code,
        message: message.into(),
        retryable: false,
        resource_kind: resource_kind.to_string(),
        resource_id: resource_id.to_string(),
        current_version,
    })
}

fn required(value: &str, field: &str) -> StoreResult<()> {
    if value.trim().is_empty() {
        return Err(trust_error(
            TrustErrorCode::InvalidStateTransition,
            format!("{field} must not be empty"),
            "request",
            field,
            None,
        ));
    }
    Ok(())
}

fn now_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

pub fn canonical_json_fingerprint(value: &Value) -> String {
    let bytes = serde_json::to_vec(&canonicalize(value)).expect("canonical JSON serialization");
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn event_projection<T: for<'de> Deserialize<'de>>(
    envelope: &TrustOperationEnvelope,
) -> StoreResult<T> {
    serde_json::from_value(envelope.operation.resulting_projection.clone())
        .map_err(StoreError::from)
}

impl HarnessStore {
    fn trust_operation_envelopes_unlocked(&self) -> StoreResult<Vec<TrustOperationEnvelope>> {
        self.read_jsonl(TRUST_OPERATIONS_LEDGER)
    }

    pub fn canonical_operations(&self) -> StoreResult<Vec<CanonicalOperation>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .map(|envelope| envelope.operation)
            .collect())
    }

    fn latest_trust_envelopes_unlocked(
        &self,
        execution_space_id: &str,
        aggregate_kind: &str,
    ) -> StoreResult<BTreeMap<String, TrustOperationEnvelope>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id == execution_space_id
                && envelope.operation.event.aggregate_kind == aggregate_kind
            {
                latest.insert(envelope.operation.event.aggregate_id.clone(), envelope);
            }
        }
        Ok(latest)
    }

    fn commit_trust_projection<T: Serialize + for<'de> Deserialize<'de> + Clone>(
        &self,
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        transition: &str,
        request_payload: Value,
        resulting_projection: &T,
        immutable_side_records: Vec<Value>,
        initial_outbox_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<T>> {
        required(&context.execution_space_id, "execution_space_id")?;
        required(&context.authenticated_actor.id, "authenticated_actor.id")?;
        required(&context.command_name, "command_name")?;
        required(&context.idempotency_key, "idempotency_key")?;
        required(aggregate_kind, "aggregate_kind")?;
        required(aggregate_id, "aggregate_id")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = canonical_json_fingerprint(&request_payload);

        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used with a different canonical payload",
                    aggregate_kind,
                    aggregate_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            if replay.operation.event.aggregate_kind != aggregate_kind
                || replay.operation.event.aggregate_id != aggregate_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotent replay changed aggregate identity",
                    aggregate_kind,
                    aggregate_id,
                    None,
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }

        let latest = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == aggregate_kind
                    && envelope.operation.event.aggregate_id == aggregate_id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence);
        let current_version = latest
            .map(|envelope| envelope.operation.event.resulting_version)
            .unwrap_or(0);
        if context.expected_version != current_version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                format!(
                    "expected version {}, current version is {current_version}",
                    context.expected_version
                ),
                aggregate_kind,
                aggregate_id,
                Some(current_version),
            ));
        }
        let store_sequence = existing
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let resulting_version = current_version + 1;
        let event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: aggregate_kind.to_string(),
            aggregate_id: aggregate_id.to_string(),
            sequence: latest
                .map(|envelope| envelope.operation.event.sequence)
                .unwrap_or(0)
                + 1,
            store_sequence,
            transition: transition.to_string(),
            expected_version: current_version,
            resulting_version,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        let operation = CanonicalOperation {
            event: event.clone(),
            resulting_projection: serde_json::to_value(resulting_projection)?,
            immutable_side_records,
            initial_outbox_records,
        };
        self.append_jsonl_unlocked(
            TRUST_OPERATIONS_LEDGER,
            &TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: context.command_name.clone(),
                operation,
            },
        )?;
        Ok(CanonicalMutationResult {
            projection: resulting_projection.clone(),
            event,
            replayed: false,
        })
    }

    pub fn trust_agent_members(&self, execution_space_id: &str) -> StoreResult<Vec<AgentMember>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_member")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn create_trust_agent_member(
        &self,
        context: &MutationContext,
        mut member: AgentMember,
    ) -> StoreResult<CanonicalMutationResult<AgentMember>> {
        required(&member.id, "AgentMember.id")?;
        required(&member.name, "AgentMember.name")?;
        required(&member.role, "AgentMember.role")?;
        required(&member.workspace_policy, "AgentMember.workspace_policy")?;
        if member.version != 1 || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentMember create requires absent CAS and version 1",
                "agent_member",
                &member.id,
                Some(0),
            ));
        }
        if member.created_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "created_by must equal the authenticated actor",
                "agent_member",
                &member.id,
                None,
            ));
        }
        member.updated_at = member.created_at.clone();
        let payload = serde_json::to_value(&member)?;
        self.commit_trust_projection(
            context,
            "agent_member",
            &member.id,
            "created",
            payload,
            &member,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn transition_trust_agent_member(
        &self,
        context: &MutationContext,
        member_id: &str,
        next_status: AgentMemberOrganizationStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentMember>> {
        let mut current = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_member")?
            .remove(member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentMember not found",
                    "agent_member",
                    member_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentMember>(&envelope))?;
        let allowed = matches!(
            (current.organization_status, next_status),
            (
                AgentMemberOrganizationStatus::Active,
                AgentMemberOrganizationStatus::Paused
            ) | (
                AgentMemberOrganizationStatus::Paused,
                AgentMemberOrganizationStatus::Active
            ) | (
                AgentMemberOrganizationStatus::Active,
                AgentMemberOrganizationStatus::Retired
            ) | (
                AgentMemberOrganizationStatus::Paused,
                AgentMemberOrganizationStatus::Retired
            )
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentMember transition is not allowed",
                "agent_member",
                member_id,
                Some(current.version),
            ));
        }
        current.organization_status = next_status;
        current.version += 1;
        current.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "agent_member",
            member_id,
            match next_status {
                AgentMemberOrganizationStatus::Active => "resumed",
                AgentMemberOrganizationStatus::Paused => "paused",
                AgentMemberOrganizationStatus::Retired => "retired",
            },
            serde_json::json!({"status": next_status, "updated_at": updated_at}),
            &current,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_member_runs(&self, execution_space_id: &str) -> StoreResult<Vec<MemberRun>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn create_trust_member_run(
        &self,
        context: &MutationContext,
        run: MemberRun,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        required(&run.id, "MemberRun.id")?;
        required(&run.agent_member_id, "MemberRun.agent_member_id")?;
        required(&run.team_run_id, "MemberRun.team_run_id")?;
        if run.version != 1 || run.runtime_generation != 1 || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "MemberRun create requires absent CAS, version 1 and generation 1",
                "member_run",
                &run.id,
                Some(0),
            ));
        }
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == run.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun references a missing AgentMember",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        match member.organization_status {
            AgentMemberOrganizationStatus::Active => {}
            AgentMemberOrganizationStatus::Paused => {
                return Err(trust_error(
                    TrustErrorCode::AgentMemberPaused,
                    "paused AgentMember cannot start a MemberRun",
                    "agent_member",
                    &member.id,
                    Some(member.version),
                ))
            }
            AgentMemberOrganizationStatus::Retired => {
                return Err(trust_error(
                    TrustErrorCode::AgentMemberRetired,
                    "retired AgentMember cannot start a MemberRun",
                    "agent_member",
                    &member.id,
                    Some(member.version),
                ))
            }
        }
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|candidate| candidate.id == run.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun references a missing TeamRun",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        let team = self
            .latest_teams()?
            .remove(&team_run.agent_team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamRun references a missing AgentTeam",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        if team.host_agent_id != run.agent_member_id
            && !team.member_ids.contains(&run.agent_member_id)
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentMember does not belong to the Team",
                "member_run",
                &run.id,
                None,
            ));
        }
        self.commit_trust_projection(
            context,
            "member_run",
            &run.id,
            "created",
            serde_json::to_value(&run)?,
            &run,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn transition_trust_member_run(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        next: MemberCoordinationStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        let mut run = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "member_run")?
            .remove(member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun not found",
                    "member_run",
                    member_run_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberRun>(&envelope))?;
        let transition = match (run.coordination_status, next) {
            (MemberCoordinationStatus::Active, MemberCoordinationStatus::Closed) => "closed",
            (MemberCoordinationStatus::Closed, MemberCoordinationStatus::Active) => "reopened",
            (MemberCoordinationStatus::Active, MemberCoordinationStatus::Retired)
            | (MemberCoordinationStatus::Closed, MemberCoordinationStatus::Retired) => "retired",
            _ => {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun coordination transition is not allowed",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
        };
        if transition == "reopened" {
            let session = run.native_session.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::NativeSessionMissing,
                    "reopen requires a resumable NativeSessionRef",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                )
            })?;
            if !session.supports_resume
                || !matches!(
                    session.availability,
                    firm_core::agentfirm_api::NativeSessionAvailability::Available
                        | firm_core::agentfirm_api::NativeSessionAvailability::Stale
                )
            {
                return Err(trust_error(
                    TrustErrorCode::NativeSessionIncompatible,
                    "NativeSessionRef is not safely resumable",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
            run.runtime_generation += 1;
            run.runtime_status = MemberRuntimeStatus::Idle;
        } else if transition == "closed" {
            run.runtime_status = MemberRuntimeStatus::Stopped;
        } else {
            run.runtime_status = MemberRuntimeStatus::Stopped;
            run.finished_at = Some(updated_at.to_string());
        }
        run.coordination_status = next;
        run.version += 1;
        run.last_event_at = Some(updated_at.to_string());

        let mut side_records = Vec::new();
        if transition == "closed" || transition == "retired" {
            for mut delivery in self.trust_message_deliveries(&context.execution_space_id)? {
                if delivery.recipient_member_run_id != member_run_id {
                    continue;
                }
                if matches!(delivery.status, MessageDeliveryStatus::Queued) {
                    if transition == "closed" {
                        delivery.freeze_generation = Some(run.runtime_generation);
                    } else {
                        delivery.status = MessageDeliveryStatus::Invalidated;
                        delivery.version += 1;
                    }
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                }
            }
            for mut delivery in self.trust_work_deliveries(&context.execution_space_id)? {
                if delivery.recipient_member_run_id != member_run_id {
                    continue;
                }
                if matches!(delivery.status, WorkDeliveryStatus::Queued) {
                    if transition == "closed" {
                        delivery.freeze_generation = Some(run.runtime_generation);
                    } else {
                        delivery.status = WorkDeliveryStatus::Invalidated;
                        delivery.version += 1;
                    }
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                }
            }
        }
        self.commit_trust_projection(
            context,
            "member_run",
            member_run_id,
            transition,
            serde_json::json!({"coordination_status": next, "updated_at": updated_at}),
            &run,
            side_records,
            Vec::new(),
        )
    }

    fn trust_side_records<T: for<'de> Deserialize<'de>>(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<T>> {
        let mut rows = Vec::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            for value in envelope
                .operation
                .initial_outbox_records
                .into_iter()
                .chain(envelope.operation.immutable_side_records)
            {
                if let Ok(row) = serde_json::from_value::<T>(value) {
                    rows.push(row);
                }
            }
        }
        Ok(rows)
    }

    pub fn trust_message_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MessageDelivery>> {
        let mut latest = BTreeMap::new();
        for delivery in self.trust_side_records::<MessageDelivery>(execution_space_id)? {
            latest.insert(delivery.id.clone(), delivery);
        }
        Ok(latest.into_values().collect())
    }

    pub fn trust_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<WorkDelivery>> {
        let mut latest = BTreeMap::new();
        for delivery in self.trust_side_records::<WorkDelivery>(execution_space_id)? {
            latest.insert(delivery.id.clone(), delivery);
        }
        Ok(latest.into_values().collect())
    }

    pub fn create_trust_team_message_with_deliveries(
        &self,
        context: &MutationContext,
        message: TeamMessage,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<TeamMessage>> {
        required(&message.id, "TeamMessage.id")?;
        required(&message.team_run_id, "TeamMessage.team_run_id")?;
        required(&message.body, "TeamMessage.body")?;
        required(&message.correlation_id, "TeamMessage.correlation_id")?;
        if message.sender != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "message sender must equal authenticated actor",
                "team_message",
                &message.id,
                None,
            ));
        }
        if message.recipients.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "message requires at least one recipient",
                "team_message",
                &message.id,
                None,
            ));
        }
        if !self
            .team_runs()?
            .into_iter()
            .any(|run| run.id == message.team_run_id)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "message references a missing TeamRun",
                "team_message",
                &message.id,
                None,
            ));
        }
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        if message.sender.kind == ActorKind::AgentMember
            && !runs.iter().any(|run| {
                run.team_run_id == message.team_run_id
                    && run.agent_member_id == message.sender.id
                    && run.coordination_status == MemberCoordinationStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentMember sender has no active MemberRun in the TeamRun",
                "team_message",
                &message.id,
                None,
            ));
        }
        let mut seen = BTreeSet::new();
        let mut deliveries = Vec::new();
        for recipient in &message.recipients {
            if recipient.kind != ActorKind::AgentMember || !seen.insert(recipient.id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "message recipients must be unique AgentMember references",
                    "team_message",
                    &message.id,
                    None,
                ));
            }
            let matching = runs
                .iter()
                .filter(|run| {
                    run.team_run_id == message.team_run_id
                        && run.agent_member_id == recipient.id
                        && run.coordination_status != MemberCoordinationStatus::Retired
                })
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "recipient must resolve to exactly one non-retired MemberRun in the TeamRun",
                    "team_message",
                    &message.id,
                    None,
                ));
            }
            let run = matching[0];
            deliveries.push(MessageDelivery {
                id: format!("{}:{}", message.id, run.id),
                message_id: message.id.clone(),
                recipient_member_run_id: run.id.clone(),
                status: MessageDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_supervisor_generation: None,
                claimed_member_generation: None,
                claim_expires_at: None,
                freeze_generation: (run.coordination_status == MemberCoordinationStatus::Closed)
                    .then_some(run.runtime_generation),
                provider_receipt_id: None,
                failure_code: None,
                failure_detail: None,
                version: 1,
                updated_at: updated_at.to_string(),
            });
        }
        self.commit_trust_projection(
            context,
            "team_message",
            &message.id,
            "created",
            serde_json::to_value(&message)?,
            &message,
            Vec::new(),
            deliveries
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    pub fn create_trust_work_deliveries(
        &self,
        context: &MutationContext,
        work_event_id: &str,
        work_id: &str,
        work_revision: u64,
        recipient_member_run_ids: &[String],
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Vec<WorkDelivery>>> {
        required(work_event_id, "work_event_id")?;
        required(work_id, "work_id")?;
        if recipient_member_run_ids.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "WorkEvent requires at least one delivery recipient",
                "work_event",
                work_event_id,
                None,
            ));
        }
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        let mut unique = BTreeSet::new();
        let mut deliveries = Vec::new();
        for run_id in recipient_member_run_ids {
            if !unique.insert(run_id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery recipients must be unique",
                    "work_event",
                    work_event_id,
                    None,
                ));
            }
            let run = runs.iter().find(|run| run.id == *run_id).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery recipient MemberRun does not exist",
                    "work_event",
                    work_event_id,
                    None,
                )
            })?;
            match run.coordination_status {
                MemberCoordinationStatus::Active | MemberCoordinationStatus::Closed => {}
                MemberCoordinationStatus::Retired => {
                    return Err(trust_error(
                        TrustErrorCode::MemberRunRetired,
                        "retired MemberRun rejects new WorkDelivery",
                        "member_run",
                        run_id,
                        Some(run.version),
                    ))
                }
            }
            deliveries.push(WorkDelivery {
                id: format!("{work_event_id}:{run_id}"),
                work_event_id: work_event_id.to_string(),
                work_id: work_id.to_string(),
                work_revision,
                recipient_member_run_id: run_id.clone(),
                status: WorkDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_supervisor_generation: None,
                claimed_member_generation: None,
                claim_expires_at: None,
                freeze_generation: (run.coordination_status == MemberCoordinationStatus::Closed)
                    .then_some(run.runtime_generation),
                provider_receipt_id: None,
                failure_code: None,
                failure_detail: None,
                version: 1,
                updated_at: updated_at.to_string(),
            });
        }
        self.commit_trust_projection(
            context,
            "work_event_delivery_batch",
            work_event_id,
            "deliveries_created",
            serde_json::json!({
                "work_event_id": work_event_id,
                "work_id": work_id,
                "work_revision": work_revision,
                "recipients": recipient_member_run_ids,
            }),
            &deliveries,
            Vec::new(),
            deliveries
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    fn claimable_member_run(
        &self,
        execution_space_id: &str,
        member_run_id: &str,
        member_generation: u64,
    ) -> StoreResult<MemberRun> {
        let run = self
            .trust_member_runs(execution_space_id)?
            .into_iter()
            .find(|run| run.id == member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "delivery references a missing MemberRun",
                    "member_run",
                    member_run_id,
                    None,
                )
            })?;
        match run.coordination_status {
            MemberCoordinationStatus::Closed => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunClosed,
                    "closed MemberRun cannot claim delivery",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
            MemberCoordinationStatus::Retired => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunRetired,
                    "retired MemberRun cannot claim delivery",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
            MemberCoordinationStatus::Active => {}
        }
        if run.runtime_generation != member_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "delivery claim used a stale MemberRun generation",
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        let member = self
            .trust_agent_members(execution_space_id)?
            .into_iter()
            .find(|member| member.id == run.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun AgentMember is missing",
                    "agent_member",
                    &run.agent_member_id,
                    None,
                )
            })?;
        match member.organization_status {
            AgentMemberOrganizationStatus::Active => Ok(run),
            AgentMemberOrganizationStatus::Paused => Err(trust_error(
                TrustErrorCode::AgentMemberPaused,
                "paused AgentMember cannot claim delivery",
                "agent_member",
                &member.id,
                Some(member.version),
            )),
            AgentMemberOrganizationStatus::Retired => Err(trust_error(
                TrustErrorCode::AgentMemberRetired,
                "retired AgentMember cannot claim delivery",
                "agent_member",
                &member.id,
                Some(member.version),
            )),
        }
    }

    pub fn claim_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: DeliveryClaim,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Queued {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "only queued MessageDelivery may be claimed",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let run = self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            claim.member_generation,
        )?;
        if delivery
            .freeze_generation
            .is_some_and(|generation| generation >= run.runtime_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "delivery remains frozen for the closed generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_supervisor_generation = Some(claim.supervisor_generation);
        delivery.claimed_member_generation = Some(claim.member_generation);
        delivery.claim_expires_at = Some(claim.claim_expires_at.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "message_delivery",
            delivery_id,
            "claimed",
            serde_json::to_value(&claim)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn receive_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        receipt: ProviderReceipt,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(receipt.claim_id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "provider receipt does not match the active claim",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            receipt.member_generation,
        )?;
        if delivery.claimed_supervisor_generation != Some(receipt.supervisor_generation)
            || delivery.claimed_member_generation != Some(receipt.member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt used a stale supervisor or member generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(receipt.provider_receipt_id.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "message_delivery",
            delivery_id,
            "provider_received",
            serde_json::to_value(&receipt)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn acknowledge_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim_id: &str,
        member_generation: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::ProviderReceived
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.provider_receipt_id.is_none()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryReceiptMissing,
                "acknowledgement requires the exact claim and provider receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            member_generation,
        )?;
        if delivery.claimed_member_generation != Some(member_generation) {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "ack used a stale member generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Acknowledged;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "message_delivery",
            delivery_id,
            "acknowledged",
            serde_json::json!({"claim_id": claim_id, "member_generation": member_generation}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn reconcile_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        outcome: DeliveryReconcileOutcome,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        required(evidence_ref, "evidence_ref")?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Claimed
            || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "reconcile applies only to an uncertain claimed delivery without receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let transition = match outcome {
            DeliveryReconcileOutcome::Acknowledged => {
                delivery.status = MessageDeliveryStatus::Acknowledged;
                "reconciled_acknowledged"
            }
            DeliveryReconcileOutcome::RetrySafeFailure => {
                delivery.status = MessageDeliveryStatus::Failed;
                delivery.failure_code = Some("RECONCILED_RETRY_SAFE".into());
                delivery.failure_detail = Some(evidence_ref.to_string());
                "reconciled_retry_safe_failure"
            }
        };
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "message_delivery",
            delivery_id,
            transition,
            serde_json::json!({"outcome": outcome, "evidence_ref": evidence_ref}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn retry_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Failed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only failed MessageDelivery can be retried",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Queued;
        delivery.attempt += 1;
        delivery.claim_id = None;
        delivery.claimed_supervisor_generation = None;
        delivery.claimed_member_generation = None;
        delivery.claim_expires_at = None;
        delivery.provider_receipt_id = None;
        delivery.failure_code = None;
        delivery.failure_detail = None;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "message_delivery",
            delivery_id,
            "retried",
            serde_json::json!({"attempt": delivery.attempt}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn claim_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: DeliveryClaim,
        current_work_revision: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.work_revision != current_work_revision {
            delivery.status = WorkDeliveryStatus::Invalidated;
            delivery.failure_code = Some("WORK_REVISION_STALE".into());
            delivery.version += 1;
            delivery.updated_at = updated_at.to_string();
            let _ = self.commit_trust_projection(
                context,
                "work_delivery",
                delivery_id,
                "invalidated_stale_revision",
                serde_json::json!({"current_work_revision": current_work_revision}),
                &delivery,
                vec![serde_json::to_value(&delivery)?],
                Vec::new(),
            )?;
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "WorkDelivery revision is stale and was invalidated",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        if delivery.status != WorkDeliveryStatus::Queued {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "only queued WorkDelivery may be claimed",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let run = self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            claim.member_generation,
        )?;
        if delivery
            .freeze_generation
            .is_some_and(|generation| generation >= run.runtime_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkDelivery remains frozen for the closed generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_supervisor_generation = Some(claim.supervisor_generation);
        delivery.claimed_member_generation = Some(claim.member_generation);
        delivery.claim_expires_at = Some(claim.claim_expires_at.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "work_delivery",
            delivery_id,
            "claimed",
            serde_json::to_value(&claim)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn receive_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        receipt: ProviderReceipt,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(receipt.claim_id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "provider receipt does not match the active WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            receipt.member_generation,
        )?;
        if delivery.claimed_supervisor_generation != Some(receipt.supervisor_generation)
            || delivery.claimed_member_generation != Some(receipt.member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt used a stale generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(receipt.provider_receipt_id.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "work_delivery",
            delivery_id,
            "provider_received",
            serde_json::to_value(&receipt)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn reconcile_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        required(evidence_ref, "evidence_ref")?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "reconcile applies only to an uncertain claimed WorkDelivery",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Failed;
        delivery.failure_code = Some("RECONCILED_RETRY_SAFE".into());
        delivery.failure_detail = Some(evidence_ref.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "work_delivery",
            delivery_id,
            "reconciled_retry_safe_failure",
            serde_json::json!({"evidence_ref": evidence_ref}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn retry_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        current_work_revision: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Failed
            || delivery.work_revision != current_work_revision
        {
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "WorkDelivery retry requires failed status and exact current Work revision",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Queued;
        delivery.attempt += 1;
        delivery.claim_id = None;
        delivery.claimed_supervisor_generation = None;
        delivery.claimed_member_generation = None;
        delivery.claim_expires_at = None;
        delivery.provider_receipt_id = None;
        delivery.failure_code = None;
        delivery.failure_detail = None;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection(context, "work_delivery", delivery_id, "retried", serde_json::json!({"attempt": delivery.attempt, "work_revision": current_work_revision}), &delivery, vec![serde_json::to_value(&delivery)?], Vec::new())
    }

    pub fn create_trust_work_report(
        &self,
        context: &MutationContext,
        report: WorkReport,
    ) -> StoreResult<CanonicalMutationResult<WorkReport>> {
        if report.kind == WorkReportKind::Result
            && (report.candidate.is_none()
                || report
                    .candidate_fingerprint
                    .as_deref()
                    .unwrap_or("")
                    .is_empty()
                || report.evidence_refs.is_empty())
        {
            return Err(trust_error(
                TrustErrorCode::ReportEvidenceMissing,
                "result report requires exact CandidateRef, fingerprint and evidence",
                "work_report",
                &report.id,
                None,
            ));
        }
        if let (Some(candidate), Some(fingerprint)) = (
            report.candidate.as_ref(),
            report.candidate_fingerprint.as_ref(),
        ) {
            let expected = canonical_json_fingerprint(&serde_json::to_value(candidate)?);
            if fingerprint != &expected {
                return Err(trust_error(
                    TrustErrorCode::ReportEvidenceMissing,
                    "candidate_fingerprint does not match canonical CandidateRef",
                    "work_report",
                    &report.id,
                    None,
                ));
            }
        }
        if report.kind == WorkReportKind::Failure && report.failure_analysis_ref.is_none() {
            return Err(trust_error(
                TrustErrorCode::FailureAnalysisMissing,
                "failure report requires FailureAnalysis",
                "work_report",
                &report.id,
                None,
            ));
        }
        if let Some(analysis_id) = report.failure_analysis_ref.as_deref() {
            let analysis = self
                .latest_trust_envelopes_unlocked(&context.execution_space_id, "failure_analysis")?
                .remove(analysis_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::FailureAnalysisMissing,
                        "failure report references a missing FailureAnalysis",
                        "work_report",
                        &report.id,
                        None,
                    )
                })
                .and_then(|envelope| event_projection::<FailureAnalysis>(&envelope))?;
            if analysis.work_id != report.work_id || analysis.work_revision != report.work_revision
            {
                return Err(trust_error(
                    TrustErrorCode::FailureAnalysisMissing,
                    "FailureAnalysis does not match the report Work revision",
                    "work_report",
                    &report.id,
                    None,
                ));
            }
        }
        self.commit_trust_projection(
            context,
            "work_report",
            &report.id,
            "created",
            serde_json::to_value(&report)?,
            &report,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_finding(
        &self,
        context: &MutationContext,
        finding: WorkFinding,
    ) -> StoreResult<CanonicalMutationResult<WorkFinding>> {
        self.commit_trust_projection(
            context,
            "work_finding",
            &finding.id,
            "created",
            serde_json::to_value(&finding)?,
            &finding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_failure_analysis(
        &self,
        context: &MutationContext,
        analysis: FailureAnalysis,
    ) -> StoreResult<CanonicalMutationResult<FailureAnalysis>> {
        self.commit_trust_projection(
            context,
            "failure_analysis",
            &analysis.id,
            "created",
            serde_json::to_value(&analysis)?,
            &analysis,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn bind_trust_work_module(
        &self,
        context: &MutationContext,
        binding: WorkModuleBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkModuleBinding>> {
        if binding.config_fingerprint != canonical_json_fingerprint(&binding.resolved_config) {
            return Err(trust_error(
                TrustErrorCode::ModuleConfigInvalid,
                "module config_fingerprint does not match resolved_config",
                "work_module_binding",
                &binding.id,
                None,
            ));
        }
        if binding.module_id == "integration-plan"
            && binding.module_version == 1
            && (!binding.resolved_config.is_object()
                || ![
                    "base_revision",
                    "target_revision",
                    "work_boundaries",
                    "candidate_boundaries",
                    "interfaces",
                    "convergence_points",
                    "merge_order",
                    "conflict_owner",
                    "per_merge_checks",
                    "combined_verification",
                    "rollback_plan",
                ]
                .into_iter()
                .all(|key| binding.resolved_config.get(key).is_some()))
        {
            return Err(trust_error(
                TrustErrorCode::ModuleConfigInvalid,
                "integration-plan@1 config is incomplete",
                "work_module_binding",
                &binding.id,
                None,
            ));
        }
        self.commit_trust_projection(
            context,
            "work_module_binding",
            &binding.id,
            "attached",
            serde_json::to_value(&binding)?,
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_gate_requirement(
        &self,
        context: &MutationContext,
        requirement: GateRequirement,
    ) -> StoreResult<CanonicalMutationResult<GateRequirement>> {
        let existing = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_requirement")?
            .into_values()
            .map(|envelope| event_projection::<GateRequirement>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let mut graph = existing
            .iter()
            .map(|item| (item.id.clone(), item.dependency_requirement_ids.clone()))
            .collect::<BTreeMap<_, _>>();
        graph.insert(
            requirement.id.clone(),
            requirement.dependency_requirement_ids.clone(),
        );
        fn reaches(
            graph: &BTreeMap<String, Vec<String>>,
            current: &str,
            target: &str,
            seen: &mut BTreeSet<String>,
        ) -> bool {
            if current == target {
                return true;
            }
            if !seen.insert(current.to_string()) {
                return false;
            }
            graph
                .get(current)
                .into_iter()
                .flatten()
                .any(|next| reaches(graph, next, target, seen))
        }
        if requirement
            .dependency_requirement_ids
            .iter()
            .any(|dependency| reaches(&graph, dependency, &requirement.id, &mut BTreeSet::new()))
        {
            return Err(trust_error(
                TrustErrorCode::GateDependencyCycle,
                "gate requirement introduces a dependency cycle",
                "gate_requirement",
                &requirement.id,
                None,
            ));
        }
        self.commit_trust_projection(
            context,
            "gate_requirement",
            &requirement.id,
            "created",
            serde_json::to_value(&requirement)?,
            &requirement,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_gate_evaluation(
        &self,
        context: &MutationContext,
        evaluation: GateEvaluation,
    ) -> StoreResult<CanonicalMutationResult<GateEvaluation>> {
        let requirement = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_requirement")?
            .remove(&evaluation.requirement_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "gate requirement not found",
                    "gate_evaluation",
                    &evaluation.id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<GateRequirement>(&envelope))?;
        if requirement.work_id != evaluation.work_id
            || requirement.work_revision != evaluation.work_revision
            || requirement.work_report_id != evaluation.work_report_id
            || requirement.candidate_fingerprint != evaluation.candidate_fingerprint
            || requirement.config_fingerprint != evaluation.config_fingerprint
            || requirement.evaluator_version != evaluation.evaluator_version
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "evaluation does not exactly match the frozen requirement",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        self.commit_trust_projection(
            context,
            "gate_evaluation",
            &evaluation.id,
            "evaluated",
            serde_json::to_value(&evaluation)?,
            &evaluation,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_gate_waiver(
        &self,
        context: &MutationContext,
        waiver: GateWaiver,
    ) -> StoreResult<CanonicalMutationResult<GateWaiver>> {
        if waiver.state != GateWaiverState::Active
            || context.authority_actor.as_ref() != Some(&waiver.authority_actor)
            || context.authenticated_actor != waiver.performed_by_actor
        {
            return Err(trust_error(
                TrustErrorCode::GateWaiverUnauthorized,
                "waiver authority and authenticated actor must match the mutation context",
                "gate_waiver",
                &waiver.id,
                None,
            ));
        }
        let requirement = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_requirement")?
            .remove(&waiver.requirement_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "waiver references a missing gate requirement",
                    "gate_waiver",
                    &waiver.id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<GateRequirement>(&envelope))?;
        if requirement.work_id != waiver.work_id
            || requirement.work_revision != waiver.work_revision
            || requirement.candidate_fingerprint != waiver.candidate_fingerprint
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "waiver does not exactly match the frozen requirement",
                "gate_waiver",
                &waiver.id,
                None,
            ));
        }
        self.commit_trust_projection(
            context,
            "gate_waiver",
            &waiver.id,
            "created",
            serde_json::to_value(&waiver)?,
            &waiver,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_gate_satisfied(
        &self,
        execution_space_id: &str,
        work_id: &str,
        work_revision: u64,
        report_id: &str,
        candidate_fingerprint: &str,
    ) -> StoreResult<()> {
        let requirements = self
            .latest_trust_envelopes_unlocked(execution_space_id, "gate_requirement")?
            .into_values()
            .map(|envelope| event_projection::<GateRequirement>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|requirement| {
                requirement.required
                    && requirement.work_id == work_id
                    && requirement.work_revision == work_revision
                    && requirement.work_report_id == report_id
                    && requirement.candidate_fingerprint == candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let evaluations = self
            .latest_trust_envelopes_unlocked(execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let waivers = self
            .latest_trust_envelopes_unlocked(execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        for requirement in requirements {
            let passed = evaluations.iter().any(|evaluation| {
                evaluation.requirement_id == requirement.id
                    && evaluation.work_id == requirement.work_id
                    && evaluation.work_revision == requirement.work_revision
                    && evaluation.work_report_id == requirement.work_report_id
                    && evaluation.candidate_fingerprint == requirement.candidate_fingerprint
                    && evaluation.config_fingerprint == requirement.config_fingerprint
                    && evaluation.evaluator_version == requirement.evaluator_version
                    && evaluation.verdict == GateVerdict::Passed
            });
            let waived = waivers.iter().any(|waiver| {
                waiver.requirement_id == requirement.id
                    && waiver.work_id == requirement.work_id
                    && waiver.work_revision == requirement.work_revision
                    && waiver.candidate_fingerprint == requirement.candidate_fingerprint
                    && waiver.state == GateWaiverState::Active
            });
            if !passed && !waived {
                return Err(trust_error(
                    TrustErrorCode::GateEvaluationRequired,
                    "required gate has no exact valid evaluation or waiver",
                    "work",
                    work_id,
                    Some(work_revision),
                ));
            }
        }
        Ok(())
    }

    pub fn create_trust_workspace_binding(
        &self,
        context: &MutationContext,
        binding: MemberWorkspaceBinding,
    ) -> StoreResult<CanonicalMutationResult<MemberWorkspaceBinding>> {
        required(
            &binding.canonical_root,
            "MemberWorkspaceBinding.canonical_root",
        )?;
        if binding.version != 1 || binding.lifecycle != WorkspaceLifecycle::Requested {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "workspace binding create requires requested lifecycle and version 1",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let path = std::path::Path::new(&binding.canonical_root);
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "canonical_root must be an absolute normalized path",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let run = self
            .trust_member_runs(&context.execution_space_id)?
            .into_iter()
            .find(|run| run.id == binding.member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace binding references a missing MemberRun",
                    "workspace_binding",
                    &binding.id,
                    None,
                )
            })?;
        if run.team_run_id != binding.team_run_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace binding TeamRun does not match MemberRun",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|team_run| team_run.id == binding.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace TeamRun is missing",
                    "workspace_binding",
                    &binding.id,
                    None,
                )
            })?;
        if team_run.project_binding_id != binding.project_binding_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace ProjectBinding does not match TeamRun placement",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let mut cursor = std::path::PathBuf::new();
        for component in path.components() {
            cursor.push(component.as_os_str());
            match std::fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceLinkEscape,
                        "workspace canonical path contains a symbolic-link component",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
        if binding.mode == WorkspaceMode::SharedLive {
            if binding.ownership != WorkspaceOwnership::SharedProject {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "shared_live requires shared_project ownership",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            let member = self
                .trust_agent_members(&context.execution_space_id)?
                .into_iter()
                .find(|member| member.id == run.agent_member_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "workspace AgentMember is missing",
                        "workspace_binding",
                        &binding.id,
                        None,
                    )
                })?;
            if member.permission_ceiling != firm_core::agentfirm_api::PermissionCeiling::ReadOnly {
                if context.authority_actor.is_none() {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "writable shared_live requires explicit Host authority",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
                if self
                    .trust_workspace_bindings(&context.execution_space_id)?
                    .iter()
                    .any(|existing| {
                        existing.canonical_root == binding.canonical_root
                            && existing.lifecycle == WorkspaceLifecycle::Attached
                    })
                {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceGenerationFenced,
                        "shared_live writable workspace already has an attached writer",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
            }
        }
        self.commit_trust_projection(
            context,
            "workspace_binding",
            &binding.id,
            "requested",
            serde_json::to_value(&binding)?,
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_workspace_bindings(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MemberWorkspaceBinding>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "workspace_binding")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn transition_trust_workspace_binding(
        &self,
        context: &MutationContext,
        binding_id: &str,
        next: WorkspaceLifecycle,
        proof: &WorkspaceSafetyProof,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberWorkspaceBinding>> {
        let mut binding = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "workspace_binding")?
            .remove(binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace binding not found",
                    "workspace_binding",
                    binding_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberWorkspaceBinding>(&envelope))?;
        if proof.canonical_root != binding.canonical_root {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "safety proof canonical path differs from binding",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.project_binding_id != binding.project_binding_id || !proof.repository_matches {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace repository or ProjectBinding does not match",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if !proof.link_escape_free {
            return Err(trust_error(
                TrustErrorCode::WorkspaceLinkEscape,
                "workspace contains a symlink/reparse escape",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if binding
            .attached_member_generation
            .is_some_and(|generation| generation != proof.observed_member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceGenerationFenced,
                "workspace safety proof used a stale MemberRun generation",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.is_conflicted
            && next != WorkspaceLifecycle::Conflicted
            && next != WorkspaceLifecycle::CleanupBlocked
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceConflicted,
                "conflicted workspace cannot make the requested transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.is_dirty
            && next != WorkspaceLifecycle::Dirty
            && next != WorkspaceLifecycle::CleanupBlocked
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceDirty,
                "dirty workspace cannot make the requested transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let allowed = matches!(
            (binding.lifecycle, next),
            (WorkspaceLifecycle::Requested, WorkspaceLifecycle::Preparing)
                | (WorkspaceLifecycle::Preparing, WorkspaceLifecycle::Ready)
                | (WorkspaceLifecycle::Ready, WorkspaceLifecycle::Attached)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Dirty)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Conflicted)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Ready, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Dirty, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Conflicted, WorkspaceLifecycle::Archived)
                | (
                    WorkspaceLifecycle::Ready,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Attached,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Dirty,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Conflicted,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (WorkspaceLifecycle::Dirty, WorkspaceLifecycle::Attached)
                | (WorkspaceLifecycle::Conflicted, WorkspaceLifecycle::Attached)
                | (
                    WorkspaceLifecycle::CleanupBlocked,
                    WorkspaceLifecycle::Archived
                )
                | (WorkspaceLifecycle::Archived, WorkspaceLifecycle::Removed)
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "workspace lifecycle transition is not allowed",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if next == WorkspaceLifecycle::Attached {
            let run = self.claimable_member_run(
                &context.execution_space_id,
                &binding.member_run_id,
                proof.observed_member_generation,
            )?;
            binding.attached_member_generation = Some(run.runtime_generation);
        }
        if next == WorkspaceLifecycle::CleanupBlocked {
            binding.blocked_reason = Some(
                if proof.is_conflicted {
                    "WORKSPACE_CONFLICTED"
                } else if proof.is_dirty {
                    "WORKSPACE_DIRTY"
                } else {
                    "WORKSPACE_CLEANUP_BLOCKED"
                }
                .to_string(),
            );
        } else {
            binding.blocked_reason = None;
        }
        binding.lifecycle = next;
        binding.version += 1;
        binding.updated_at = updated_at.to_string();
        self.commit_trust_projection(
            context,
            "workspace_binding",
            binding_id,
            "lifecycle_transitioned",
            serde_json::json!({"next": next, "proof": proof}),
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use firm_core::agentfirm_api::{ActorRef, AgentMemberOrganizationStatus, PermissionCeiling};
    use std::fs;

    fn actor(id: &str) -> ActorRef {
        ActorRef {
            kind: ActorKind::Human,
            id: id.into(),
        }
    }

    fn context(actor_id: &str, command: &str, key: &str, expected: u64) -> MutationContext {
        MutationContext {
            execution_space_id: "space-test".into(),
            authenticated_actor: actor(actor_id),
            authority_actor: None,
            command_name: command.into(),
            idempotency_key: key.into(),
            expected_version: expected,
        }
    }

    fn member(id: &str) -> AgentMember {
        AgentMember {
            id: id.into(),
            name: "Member".into(),
            description: "Canonical durable member".into(),
            role: "implementer".into(),
            capabilities: vec!["code".into()],
            skill_refs: Vec::new(),
            provider_profile_ref: Some("codex-default".into()),
            model_preference: None,
            workspace_policy: "managed-worktree".into(),
            permission_ceiling: PermissionCeiling::WorkspaceWrite,
            organization_status: AgentMemberOrganizationStatus::Active,
            version: 1,
            created_by: actor("host"),
            created_at: "t1".into(),
            updated_at: "t1".into(),
        }
    }

    #[test]
    fn canonical_operation_is_atomic_scoped_and_exactly_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "firm-trust-kernel-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = HarnessStore::new(&root);
        let first = store
            .create_trust_agent_member(
                &context("host", "agent_member.create", "same", 0),
                member("member-1"),
            )
            .expect("create");
        assert!(!first.replayed);
        let replay = store
            .create_trust_agent_member(
                &context("host", "agent_member.create", "same", 0),
                member("member-1"),
            )
            .expect("replay");
        assert!(replay.replayed);
        assert_eq!(first.event.id, replay.event.id);
        assert_eq!(store.canonical_operations().unwrap().len(), 1);

        let mut changed = member("member-1");
        changed.role = "reviewer".into();
        let error = store
            .create_trust_agent_member(&context("host", "agent_member.create", "same", 0), changed)
            .expect_err("payload drift conflicts")
            .to_string();
        assert!(error.contains("IDEMPOTENCY_KEY_REUSED"), "{error}");

        let mut other_member = member("member-2");
        other_member.created_by = actor("another");
        store
            .create_trust_agent_member(
                &context("another", "agent_member.create", "same", 0),
                other_member,
            )
            .expect("same key in another authenticated actor scope");
        assert_eq!(store.canonical_operations().unwrap().len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
