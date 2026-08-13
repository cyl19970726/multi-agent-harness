use crate::{HarnessStore, StoreError, StoreResult};
use firm_core::agentfirm_api::{
    integration_plan_module_v1, ActorKind, ActorRef, AgentIdentity, AgentMember,
    AgentMemberOrganizationStatus, AgentSession, AgentSessionStatus, CanonicalMessageDelivery,
    CanonicalMessageDeliveryStatus, CanonicalMutationEvent, CanonicalOperation,
    CanonicalWorkDelivery, ControlCommandEnvelope, DeliveryClaim, DeliveryReconcileOutcome,
    FailureAnalysis, GateEvaluation, GateRequirement, GateRequirementSource, GateVerdict,
    GateWaiver, GateWaiverState, MemberCoordinationStatus, MemberRun, MemberRuntimeStatus,
    MemberWorkspaceBinding, Message, MessageRecipientKind, MessageSubscription,
    MessageSubscriptionKind, MessageSubscriptionStatus, MutationContext, ProviderInvocation,
    ProviderReceipt, RuntimeCommandKind, RuntimeCommandRecord, RuntimeCommandStatus,
    RuntimeEffectCertainty, RuntimeRecoveryResolution, SubscriptionCursor, TeamMembership,
    TeamMembershipStatus, TrustError, TrustErrorCode, WorkDelivery, WorkDeliveryStatus,
    WorkExecutionBinding, WorkExecutionBindingStatus, WorkFinding, WorkModuleBinding, WorkReport,
    WorkReportKind, WorkspaceLifecycle, WorkspaceMode, WorkspaceOwnership, WorkspaceSafetyProof,
};
use firm_core::{TeamActorKind, TeamActorRef, Work, WorkCommandContext, WorkDelegationRevision};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
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

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Fingerprint every command-authority and effect field while excluding the
/// server observation timestamp. The timestamp is metadata generated anew at
/// the HTTP boundary, so including it would turn an otherwise exact retry into
/// an idempotency conflict. Expiry, actor, target generations, capability and
/// payload remain bound and any change still conflicts.
pub fn runtime_command_envelope_fingerprint(
    command: &ControlCommandEnvelope,
) -> StoreResult<String> {
    let mut value = serde_json::to_value(command)?;
    if let Some(object) = value.as_object_mut() {
        object.remove("issued_at");
    }
    Ok(crate::canonical_json_fingerprint(&value))
}

#[derive(Debug)]
struct ObservedWorkspaceSafety {
    canonical_root: PathBuf,
    git_common_dir: Option<PathBuf>,
    dirty: bool,
    conflicted: bool,
    link_escape_free: bool,
    dirty_fingerprint: Option<String>,
}

fn canonical_git_path(root: &Path, value: &str) -> StoreResult<PathBuf> {
    let path = Path::new(value);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    std::fs::canonicalize(absolute).map_err(StoreError::Io)
}

fn git_output(root: &Path, args: &[&str]) -> StoreResult<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(StoreError::Io)?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn workspace_tree_link_escape_free(root: &Path) -> StoreResult<bool> {
    let canonical_root = std::fs::canonicalize(root).map_err(StoreError::Io)?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).map_err(StoreError::Io)? {
            let entry = entry.map_err(StoreError::Io)?;
            let child = entry.path();
            let metadata = std::fs::symlink_metadata(&child).map_err(StoreError::Io)?;
            if metadata.file_type().is_symlink() {
                let Ok(target) = std::fs::canonicalize(&child) else {
                    return Ok(false);
                };
                if !target.starts_with(&canonical_root) {
                    return Ok(false);
                }
            } else if metadata.is_dir() && entry.file_name() != ".git" {
                stack.push(child);
            }
        }
    }
    Ok(true)
}

fn observe_workspace_safety(root: &Path) -> StoreResult<ObservedWorkspaceSafety> {
    let canonical_root = std::fs::canonicalize(root).map_err(StoreError::Io)?;
    let link_escape_free = workspace_tree_link_escape_free(root)?;
    let git_common_dir = git_output(root, &["rev-parse", "--git-common-dir"])?
        .filter(|value| !value.is_empty())
        .map(|value| canonical_git_path(root, &value))
        .transpose()?;
    let porcelain = git_output(
        root,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    )?;
    let conflicts = git_output(root, &["diff", "--name-only", "--diff-filter=U"])?;
    let dirty = porcelain.as_deref().is_some_and(|value| !value.is_empty());
    let conflicted = conflicts.as_deref().is_some_and(|value| !value.is_empty());
    let dirty_fingerprint = porcelain.filter(|value| !value.is_empty()).map(|value| {
        let mut digest = Sha256::new();
        digest.update(value.as_bytes());
        format!("sha256:{:x}", digest.finalize())
    });
    Ok(ObservedWorkspaceSafety {
        canonical_root,
        git_common_dir,
        dirty,
        conflicted,
        link_escape_free,
        dirty_fingerprint,
    })
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

fn message_content_fingerprint(message: &Message) -> String {
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

fn event_projection<T: for<'de> Deserialize<'de>>(
    envelope: &TrustOperationEnvelope,
) -> StoreResult<T> {
    serde_json::from_value(envelope.operation.resulting_projection.clone())
        .map_err(StoreError::from)
}

fn gate_requirement_is_satisfied(
    requirement: &GateRequirement,
    requirements: &BTreeMap<String, GateRequirement>,
    evaluations: &[GateEvaluation],
    waivers: &[GateWaiver],
    visiting: &mut BTreeSet<String>,
) -> bool {
    if !visiting.insert(requirement.id.clone()) {
        return false;
    }
    let dependencies_satisfied = requirement.dependency_requirement_ids.iter().all(|id| {
        requirements.get(id).is_some_and(|dependency| {
            gate_requirement_is_satisfied(dependency, requirements, evaluations, waivers, visiting)
        })
    });
    visiting.remove(&requirement.id);
    if !dependencies_satisfied {
        return false;
    }
    let mut dependency_ids = requirement.dependency_requirement_ids.clone();
    dependency_ids.sort();
    let dependency_fingerprint = canonical_json_fingerprint(
        &serde_json::to_value(dependency_ids).expect("dependency ids serialize"),
    );
    evaluations.iter().any(|evaluation| {
        evaluation.requirement_id == requirement.id
            && evaluation.work_id == requirement.work_id
            && evaluation.work_revision == requirement.work_revision
            && evaluation.work_report_id == requirement.work_report_id
            && evaluation.candidate_fingerprint == requirement.candidate_fingerprint
            && evaluation.config_fingerprint == requirement.config_fingerprint
            && evaluation.evaluator_version == requirement.evaluator_version
            && evaluation.evaluator_fingerprint == requirement.evaluator_fingerprint
            && evaluation.performed_by == requirement.evaluator_ref
            && evaluation.dependency_fingerprint == dependency_fingerprint
            && evaluation.verdict == GateVerdict::Passed
    }) || waivers.iter().any(|waiver| {
        waiver.requirement_id == requirement.id
            && waiver.work_id == requirement.work_id
            && waiver.work_revision == requirement.work_revision
            && waiver.candidate_fingerprint == requirement.candidate_fingerprint
            && waiver.state == GateWaiverState::Active
    })
}

fn gate_evaluator_fingerprint(actor: &firm_core::agentfirm_api::ActorRef, version: &str) -> String {
    canonical_json_fingerprint(&serde_json::json!({
        "actor": actor,
        "version": version,
    }))
}

impl HarnessStore {
    fn require_current_trust_supervisor_unlocked(
        &self,
        context: &MutationContext,
        team_run_id: &str,
        supervisor_generation: u64,
        resource_kind: &str,
        resource_id: &str,
        current_version: Option<u64>,
    ) -> StoreResult<()> {
        let lease = self
            .latest_team_supervisor_lease(team_run_id)?
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::SupervisorGenerationFenced,
                    "Team Supervisor lease is missing",
                    resource_kind,
                    resource_id,
                    current_version,
                )
            })?;
        if context.authenticated_actor.kind != firm_core::agentfirm_api::ActorKind::Service
            || context.authenticated_actor.id != lease.supervisor_id
            || lease.generation != supervisor_generation
            || lease.execution_space_id != context.execution_space_id
            || lease.status != firm_core::TeamSupervisorLeaseStatus::Active
            || lease.expires_unix_ms <= current_unix_ms()
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "delivery mutation used a stale or unauthorized Team Supervisor lease",
                resource_kind,
                resource_id,
                current_version,
            ));
        }
        let parent = self
            .latest_node_daemon_lease(&lease.node_id)?
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::SupervisorGenerationFenced,
                    "Team Supervisor parent NodeDaemon lease is missing",
                    resource_kind,
                    resource_id,
                    current_version,
                )
            })?;
        if parent.status != firm_core::NodeDaemonLeaseStatus::Active
            || parent.daemon_id != lease.node_daemon_id
            || parent.generation != lease.node_daemon_generation
            || parent.expires_unix_ms <= current_unix_ms()
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "delivery mutation used a Supervisor whose parent NodeDaemon lease is stale",
                resource_kind,
                resource_id,
                current_version,
            ));
        }
        Ok(())
    }

    #[cfg(any())]
    fn trust_message_team_run_unlocked(
        &self,
        execution_space_id: &str,
        message_id: &str,
    ) -> StoreResult<String> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "team_message")?
            .remove(message_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery references a missing TeamMessage",
                    "team_message",
                    message_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<TeamMessage>(&envelope))
            .map(|message| message.team_run_id)
    }

    fn trust_work_team_run_unlocked(&self, work_id: &str) -> StoreResult<String> {
        self.latest_works_unlocked()?
            .remove(work_id)
            .map(|work| work.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkDelivery references a missing Work",
                    "work",
                    work_id,
                    None,
                )
            })
    }

    fn trust_team_work_unlocked(
        &self,
        team_id: &str,
        work_id: &str,
        work_revision: u64,
    ) -> StoreResult<Work> {
        let work = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "Work not found in the selected Execution Space",
                    "work",
                    work_id,
                    None,
                )
            })?;
        if work.team_id.as_deref() != Some(team_id) || work.version != work_revision {
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "Team-scoped Work authority or exact Work revision does not match",
                "work",
                work_id,
                Some(work.version),
            ));
        }
        Ok(work)
    }

    fn require_exact_work_member_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        actor: &ActorRef,
    ) -> StoreResult<MemberRun> {
        if actor.kind != ActorKind::AgentMember
            || work.owner_member_id.as_deref() != Some(actor.id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "member-owned Work mutation requires the exact accountable AgentMember",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        let active_member_run_id = work.active_member_run_id.as_deref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::UnauthorizedActor,
                "member-owned Work mutation requires an active WorkExecutionBinding",
                "work",
                &work.id,
                Some(work.version),
            )
        })?;
        let run = self
            .latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .remove(active_member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "WorkExecutionBinding references a missing MemberRun",
                    "work",
                    &work.id,
                    Some(work.version),
                )
            })
            .and_then(|envelope| event_projection::<MemberRun>(&envelope))?;
        if run.agent_member_id != actor.id
            || run.team_run_id != work.team_run_id
            || run.coordination_status != MemberCoordinationStatus::Active
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding is not the authenticated Member's exact active MemberRun",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        Ok(run)
    }

    fn trust_operation_envelopes_unlocked(&self) -> StoreResult<Vec<TrustOperationEnvelope>> {
        let path = self.root.join(TRUST_OPERATIONS_LEDGER);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(path)?;
        let durable_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let mut envelopes = Vec::new();
        for row in bytes[..durable_len].split(|byte| *byte == b'\n') {
            if row.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            // A complete malformed frame is corruption and remains fail-closed.
            // Only a non-newline-terminated tail can be the residue of an old
            // append-style crash and is intentionally ignored above.
            envelopes.push(serde_json::from_slice(row)?);
        }
        Ok(envelopes)
    }

    fn write_trust_operation_envelopes_atomic_unlocked(
        &self,
        envelopes: &[TrustOperationEnvelope],
    ) -> StoreResult<()> {
        let path = self.root.join(TRUST_OPERATIONS_LEDGER);
        let next_path = self.root.join("agentfirm_trust_operations.jsonl.next");
        let mut next = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&next_path)?;
        for envelope in envelopes {
            serde_json::to_writer(&mut next, envelope)?;
            next.write_all(b"\n")?;
        }
        next.flush()?;
        next.sync_all()?;
        std::fs::rename(&next_path, &path)?;
        std::fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }

    pub fn canonical_operations(&self) -> StoreResult<Vec<CanonicalOperation>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .map(|envelope| envelope.operation)
            .collect())
    }

    pub fn canonical_execution_space_ids(&self) -> StoreResult<Vec<String>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .map(|envelope| envelope.execution_space_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    /// Scope-preserving canonical operation read for server-built RoleViews.
    /// A physical Store may temporarily contain more than one Execution Space
    /// during recovery/import; callers must never fold another scope's truth.
    pub fn canonical_operations_for_space(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CanonicalOperation>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
            .map(|envelope| envelope.operation)
            .collect())
    }

    pub(crate) fn trust_work_projections_unlocked(&self) -> StoreResult<Vec<Work>> {
        let mut works = Vec::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "work" {
                works.push(event_projection::<Work>(&envelope)?);
            }
            for record in envelope.operation.immutable_side_records {
                if let Ok(work) = serde_json::from_value::<Work>(record) {
                    works.push(work);
                }
            }
        }
        Ok(works)
    }

    pub(crate) fn trust_work_delegation_revisions_unlocked(
        &self,
    ) -> StoreResult<Vec<WorkDelegationRevision>> {
        let mut revisions = Vec::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            for record in envelope.operation.immutable_side_records {
                if let Ok(revision) = serde_json::from_value::<WorkDelegationRevision>(record) {
                    revisions.push(revision);
                }
            }
        }
        Ok(revisions)
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

    fn replay_trust_projection_unlocked<T: for<'de> Deserialize<'de> + Clone>(
        &self,
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        fingerprint: &str,
    ) -> StoreResult<Option<CanonicalMutationResult<T>>> {
        let existing = self.trust_operation_envelopes_unlocked()?;
        let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) else {
            return Ok(None);
        };
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
        Ok(Some(CanonicalMutationResult {
            projection: event_projection(replay)?,
            event: replay.operation.event.clone(),
            replayed: true,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_trust_projection_unlocked<T: Serialize + for<'de> Deserialize<'de> + Clone>(
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
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            aggregate_kind,
            aggregate_id,
            &fingerprint,
        )? {
            return Ok(replay);
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
        let mut committed = existing;
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation,
        });
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: resulting_projection.clone(),
            event,
            replayed: false,
        })
    }

    fn commit_trust_work_acceptance_unlocked(
        &self,
        context: &MutationContext,
        request_payload: Value,
        work: &Work,
        immutable_side_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint
                || replay.operation.event.aggregate_kind != "work"
                || replay.operation.event.aggregate_id != work.id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used for a different Work acceptance",
                    "work",
                    &work.id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }
        let previous = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "work"
                    && envelope.operation.event.aggregate_id == work.id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence);
        let store_sequence = existing
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "work".into(),
            aggregate_id: work.id.clone(),
            sequence: previous
                .map(|envelope| envelope.operation.event.sequence)
                .unwrap_or(0)
                + 1,
            store_sequence,
            transition: "accepted".into(),
            expected_version: context.expected_version,
            resulting_version: work.version,
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
            resulting_projection: serde_json::to_value(work)?,
            immutable_side_records,
            initial_outbox_records: Vec::new(),
        };
        let mut committed = existing;
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation,
        });
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: work.clone(),
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

    /// Company/read-model projection only. One HarnessStore is one Execution
    /// Space in normal operation; this fold exists for callers that were given
    /// only the physical store and must not resurrect a second identity ledger.
    pub fn all_trust_agent_members(&self) -> StoreResult<Vec<AgentMember>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "agent_member" {
                latest.insert(
                    (
                        envelope.execution_space_id.clone(),
                        envelope.operation.event.aggregate_id.clone(),
                    ),
                    envelope,
                );
            }
        }
        latest.values().map(event_projection).collect()
    }

    pub fn create_trust_agent_member(
        &self,
        context: &MutationContext,
        mut member: AgentMember,
    ) -> StoreResult<CanonicalMutationResult<AgentMember>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(
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

    pub fn trust_member_run_scope(&self, member_run_id: &str) -> StoreResult<Option<String>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .rev()
            .find(|envelope| {
                envelope.operation.event.aggregate_kind == "member_run"
                    && envelope.operation.event.aggregate_id == member_run_id
            })
            .map(|envelope| envelope.execution_space_id))
    }

    pub fn create_trust_member_run(
        &self,
        context: &MutationContext,
        run: MemberRun,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        if transition == "reopened" {
            if let Some(binding_id) = run.workspace_binding_id.as_deref() {
                let mut binding = self
                    .trust_workspace_bindings(&context.execution_space_id)?
                    .into_iter()
                    .find(|binding| binding.id == binding_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::WorkspacePathUnsafe,
                            "reopen requires the bound workspace projection",
                            "workspace_binding",
                            binding_id,
                            None,
                        )
                    })?;
                if matches!(
                    binding.lifecycle,
                    WorkspaceLifecycle::Missing
                        | WorkspaceLifecycle::CleanupBlocked
                        | WorkspaceLifecycle::Removed
                ) {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceCleanupBlocked,
                        "reopen cannot reattach an unavailable workspace",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
                let observed = observe_workspace_safety(Path::new(&binding.canonical_root))?;
                if !observed.link_escape_free {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceLinkEscape,
                        "reopen workspace contains a symbolic-link escape",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
                if observed.conflicted || observed.dirty {
                    return Err(trust_error(
                        if observed.conflicted {
                            TrustErrorCode::WorkspaceConflicted
                        } else {
                            TrustErrorCode::WorkspaceDirty
                        },
                        "reopen requires a clean conflict-free workspace",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
                binding.lifecycle = WorkspaceLifecycle::Attached;
                binding.attached_member_generation = Some(run.runtime_generation);
                binding.dirty_fingerprint = None;
                binding.blocked_reason = None;
                binding.version += 1;
                binding.updated_at = updated_at.to_string();
                side_records.push(serde_json::to_value(binding)?);
            }
        }
        if transition == "closed" || transition == "retired" {
            for mut delivery in self.trust_work_deliveries(&context.execution_space_id)? {
                if delivery.recipient_member_run_id != member_run_id {
                    continue;
                }
                if transition == "closed" && delivery.status == WorkDeliveryStatus::Queued {
                    delivery.freeze_generation = Some(run.runtime_generation);
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                } else if transition == "retired"
                    && matches!(
                        delivery.status,
                        WorkDeliveryStatus::Queued | WorkDeliveryStatus::Claimed
                    )
                {
                    delivery.status = WorkDeliveryStatus::Invalidated;
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                }
            }
        }
        self.commit_trust_projection_unlocked(
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

    pub fn resume_trust_native_session(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.claimable_member_run(
            &context.execution_space_id,
            member_run_id,
            run.runtime_generation,
        )?;
        let session = run.native_session.as_ref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::NativeSessionMissing,
                "resume-native-session requires NativeSessionRef",
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
        run.runtime_status = MemberRuntimeStatus::Starting;
        run.version += 1;
        run.last_event_at = Some(updated_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "member_run",
            member_run_id,
            "native_session_resume_requested",
            serde_json::json!({"updated_at": updated_at}),
            &run,
            Vec::new(),
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

    fn trust_gate_requirements_unlocked(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<BTreeMap<String, GateRequirement>> {
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            for value in &envelope.operation.immutable_side_records {
                if let Ok(requirement) = serde_json::from_value::<GateRequirement>(value.clone()) {
                    latest.insert(requirement.id.clone(), requirement);
                }
            }
            if envelope.operation.event.aggregate_kind == "gate_requirement" {
                let requirement = event_projection::<GateRequirement>(&envelope)?;
                latest.insert(requirement.id.clone(), requirement);
            }
        }
        Ok(latest)
    }

    #[cfg(any())]
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

    #[cfg(any())]
    pub fn trust_team_messages(&self, execution_space_id: &str) -> StoreResult<Vec<TeamMessage>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "team_message")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_gate_waivers(&self, execution_space_id: &str) -> StoreResult<Vec<GateWaiver>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "gate_waiver")?
            .values()
            .map(event_projection)
            .collect()
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

    #[cfg(any())]
    pub fn create_trust_team_message_with_deliveries(
        &self,
        context: &MutationContext,
        message: TeamMessage,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<TeamMessage>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|run| run.id == message.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "message references a missing TeamRun",
                    "team_message",
                    &message.id,
                    None,
                )
            })?;
        let team = self
            .latest_teams()?
            .remove(&team_run.agent_team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "message TeamRun references a missing AgentTeam",
                    "team_message",
                    &message.id,
                    None,
                )
            })?;
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        if message.sender.kind == ActorKind::AgentMember
            && message.sender.id != team.host_agent_id
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
        if let Some(work_id) = message.work_id.as_deref() {
            let work = self
                .latest_works_unlocked()?
                .remove(work_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "linked TeamMessage references a missing Work",
                        "work",
                        work_id,
                        None,
                    )
                })?;
            if work.team_run_id != message.team_run_id
                || work.team_id.as_deref() != Some(team.id.as_str())
            {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "linked TeamMessage Work must belong to the exact Team and TeamRun",
                    "work",
                    work_id,
                    Some(work.version),
                ));
            }
            let actor_is_host = context.authenticated_actor.kind == ActorKind::AgentMember
                && (context.authenticated_actor.id == team.host_agent_id
                    || context.authority_actor.as_ref().is_some_and(|authority| {
                        authority.kind == ActorKind::AgentMember
                            && authority.id == team.host_agent_id
                    }));
            if !actor_is_host {
                self.require_exact_work_member_unlocked(
                    &context.execution_space_id,
                    &work,
                    &context.authenticated_actor,
                )?;
            }
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
            if recipient.id == team.host_agent_id && matching.is_empty() {
                continue;
            }
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(
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

    #[cfg(any())]
    pub fn claim_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: DeliveryClaim,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        let team_run_id = self
            .trust_message_team_run_unlocked(&context.execution_space_id, &delivery.message_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            claim.supervisor_generation,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
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
        self.commit_trust_projection_unlocked(
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

    #[cfg(any())]
    pub fn receive_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        receipt: ProviderReceipt,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        let team_run_id = self
            .trust_message_team_run_unlocked(&context.execution_space_id, &delivery.message_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            receipt.supervisor_generation,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
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
        self.commit_trust_projection_unlocked(
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

    #[cfg(any())]
    pub fn acknowledge_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim_id: &str,
        member_generation: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        let claimed_supervisor_generation =
            delivery.claimed_supervisor_generation.ok_or_else(|| {
                trust_error(
                    TrustErrorCode::DeliveryClaimConflict,
                    "acknowledgement requires a claimed Supervisor generation",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let team_run_id = self
            .trust_message_team_run_unlocked(&context.execution_space_id, &delivery.message_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            claimed_supervisor_generation,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
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
        self.commit_trust_projection_unlocked(
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

    #[cfg(any())]
    pub fn reconcile_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        outcome: DeliveryReconcileOutcome,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(
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

    #[cfg(any())]
    pub fn retry_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        // Authority must be established before the stale-revision branch below:
        // invalidation is an intentional durable mutation, not a rejection
        // side effect available to an old or caller-invented Supervisor.
        let team_run_id = self.trust_work_team_run_unlocked(&delivery.work_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            claim.supervisor_generation,
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        if delivery.work_revision != current_work_revision {
            delivery.status = WorkDeliveryStatus::Invalidated;
            delivery.failure_code = Some("WORK_REVISION_STALE".into());
            delivery.version += 1;
            delivery.updated_at = updated_at.to_string();
            let _ = self.commit_trust_projection_unlocked(
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        let team_run_id = self.trust_work_team_run_unlocked(&delivery.work_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            receipt.supervisor_generation,
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.commit_trust_projection_unlocked(context, "work_delivery", delivery_id, "retried", serde_json::json!({"attempt": delivery.attempt, "work_revision": current_work_revision}), &delivery, vec![serde_json::to_value(&delivery)?], Vec::new())
    }

    pub fn create_trust_work_report(
        &self,
        context: &MutationContext,
        team_id: &str,
        report: WorkReport,
    ) -> StoreResult<CanonicalMutationResult<WorkReport>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let source_work_revision = if report.kind == WorkReportKind::Result {
            report.work_revision.checked_sub(1).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "result report must name the resulting non-zero Work revision",
                    "work_report",
                    &report.id,
                    None,
                )
            })?
        } else {
            report.work_revision
        };
        let current_work =
            self.trust_team_work_unlocked(team_id, &report.work_id, source_work_revision)?;
        self.require_exact_work_member_unlocked(
            &context.execution_space_id,
            &current_work,
            &context.authenticated_actor,
        )?;
        if report.authored_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkReport.authored_by must equal the authenticated actor",
                "work_report",
                &report.id,
                None,
            ));
        }
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
        if report.kind == WorkReportKind::Result
            && (current_work.phase != firm_core::WorkPhase::Active
                || current_work.condition != firm_core::WorkCondition::Normal
                || report.work_revision != current_work.version + 1)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "result report may submit only normal active Work and must name the resulting Work revision",
                "work_report",
                &report.id,
                Some(current_work.version),
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
        let mut resolved_requirements = Vec::new();
        if report.kind == WorkReportKind::Result {
            let candidate_fingerprint = report
                .candidate_fingerprint
                .as_ref()
                .expect("result validation requires candidate fingerprint");
            let bindings = self
                .latest_trust_envelopes_unlocked(
                    &context.execution_space_id,
                    "work_module_binding",
                )?
                .into_values()
                .map(|envelope| event_projection::<WorkModuleBinding>(&envelope))
                .collect::<StoreResult<Vec<_>>>()?;
            for binding in bindings.into_iter().filter(|binding| {
                binding.work_id == report.work_id
                    && binding.work_revision == source_work_revision
                    && binding.module_id == "integration-plan"
                    && binding.module_version == 1
            }) {
                let definition = integration_plan_module_v1();
                for (index, template) in definition.default_gate_templates.iter().enumerate() {
                    let resolved_config = serde_json::json!({
                        "module_binding_id": binding.id,
                        "module_binding_version": binding.version,
                        "module_config_fingerprint": binding.config_fingerprint,
                        "template": template,
                    });
                    let evaluator_ref = firm_core::agentfirm_api::ActorRef {
                        kind: firm_core::agentfirm_api::ActorKind::Service,
                        id: definition.implementation_ref.clone(),
                    };
                    let evaluator_version = definition.module_version.to_string();
                    resolved_requirements.push(GateRequirement {
                        id: format!("gate:{}:{}:{index}", report.id, binding.id),
                        work_id: report.work_id.clone(),
                        work_revision: report.work_revision,
                        work_report_id: report.id.clone(),
                        candidate_fingerprint: candidate_fingerprint.clone(),
                        source: GateRequirementSource::Module,
                        source_binding_id: Some(binding.id.clone()),
                        gate_type: template
                            .get("gate_type")
                            .and_then(Value::as_str)
                            .unwrap_or("integration-plan-completeness")
                            .to_string(),
                        gate_contract_version: template
                            .get("gate_contract_version")
                            .and_then(Value::as_str)
                            .unwrap_or("1")
                            .to_string(),
                        evaluator_fingerprint: gate_evaluator_fingerprint(
                            &evaluator_ref,
                            &evaluator_version,
                        ),
                        evaluator_ref,
                        evaluator_version,
                        config_fingerprint: canonical_json_fingerprint(&resolved_config),
                        resolved_config,
                        required: template
                            .get("required")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
                        dependency_requirement_ids: Vec::new(),
                        requirement_set_fingerprint: String::new(),
                        created_at: report.created_at.clone(),
                        version: 1,
                    });
                }
            }
            let mut requirement_ids = resolved_requirements
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect::<Vec<_>>();
            requirement_ids.sort();
            let set_fingerprint =
                canonical_json_fingerprint(&serde_json::to_value(requirement_ids)?);
            for requirement in &mut resolved_requirements {
                requirement.requirement_set_fingerprint = set_fingerprint.clone();
            }
        }
        let mut side_records = resolved_requirements
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
        if report.kind == WorkReportKind::Result {
            let mut submitted_work = current_work;
            submitted_work.phase = firm_core::WorkPhase::Review;
            submitted_work.condition = firm_core::WorkCondition::Normal;
            submitted_work.version = report.work_revision;
            submitted_work.result_summary = Some(report.summary.clone());
            submitted_work.updated_at = report.created_at.clone();
            side_records.push(serde_json::to_value(submitted_work)?);
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_report",
            &report.id,
            "created",
            serde_json::to_value(&report)?,
            &report,
            side_records,
            Vec::new(),
        )
    }

    /// Latest immutable Work reports available to server-side application
    /// services. Callers must still bind the selected report to the current
    /// Work, Team, actor and placement before publishing it remotely.
    pub fn trust_work_reports(&self, execution_space_id: &str) -> StoreResult<Vec<WorkReport>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "work_report")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_work_findings(&self, execution_space_id: &str) -> StoreResult<Vec<WorkFinding>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "work_finding")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_failure_analyses(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<FailureAnalysis>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "failure_analysis")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn create_trust_finding(
        &self,
        context: &MutationContext,
        team_id: &str,
        finding: WorkFinding,
    ) -> StoreResult<CanonicalMutationResult<WorkFinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let work =
            self.trust_team_work_unlocked(team_id, &finding.work_id, finding.work_revision)?;
        self.require_exact_work_member_unlocked(
            &context.execution_space_id,
            &work,
            &context.authenticated_actor,
        )?;
        if finding.reported_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkFinding.reported_by must equal the authenticated actor",
                "work_finding",
                &finding.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
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
        team_id: &str,
        analysis: FailureAnalysis,
    ) -> StoreResult<CanonicalMutationResult<FailureAnalysis>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let work =
            self.trust_team_work_unlocked(team_id, &analysis.work_id, analysis.work_revision)?;
        let run = self.require_exact_work_member_unlocked(
            &context.execution_space_id,
            &work,
            &context.authenticated_actor,
        )?;
        if analysis.reported_by != context.authenticated_actor
            || analysis.member_run_id.as_deref() != Some(run.id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "FailureAnalysis must name the authenticated Work owner's exact active MemberRun",
                "failure_analysis",
                &analysis.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
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
        team_id: &str,
        binding: WorkModuleBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkModuleBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &binding.work_id, binding.work_revision)?;
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
        self.commit_trust_projection_unlocked(
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
        team_id: &str,
        mut requirement: GateRequirement,
    ) -> StoreResult<CanonicalMutationResult<GateRequirement>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &requirement.work_id, requirement.work_revision)?;
        let expected_evaluator_fingerprint =
            gate_evaluator_fingerprint(&requirement.evaluator_ref, &requirement.evaluator_version);
        if requirement.evaluator_fingerprint != expected_evaluator_fingerprint {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "GateRequirement evaluator fingerprint does not match its frozen ActorRef/version",
                "gate_requirement",
                &requirement.id,
                None,
            ));
        }
        let existing = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .into_values()
            .collect::<Vec<_>>();
        if existing.iter().any(|item| item.id == requirement.id) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "GateRequirement id already exists",
                "gate_requirement",
                &requirement.id,
                Some(1),
            ));
        }
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
        let mut same_set = existing
            .into_iter()
            .filter(|item| {
                item.work_id == requirement.work_id
                    && item.work_revision == requirement.work_revision
                    && item.work_report_id == requirement.work_report_id
                    && item.candidate_fingerprint == requirement.candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let mut required_ids = same_set
            .iter()
            .filter(|item| item.required)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        if requirement.required {
            required_ids.push(requirement.id.clone());
        }
        required_ids.sort();
        let set_fingerprint = canonical_json_fingerprint(&serde_json::to_value(required_ids)?);
        requirement.requirement_set_fingerprint = set_fingerprint.clone();
        for existing in &mut same_set {
            if existing.required {
                existing.requirement_set_fingerprint = set_fingerprint.clone();
                existing.version += 1;
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "gate_requirement",
            &requirement.id,
            "created",
            serde_json::to_value(&requirement)?,
            &requirement,
            same_set
                .into_iter()
                .filter(|item| item.required)
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
            Vec::new(),
        )
    }

    pub fn create_trust_gate_evaluation(
        &self,
        context: &MutationContext,
        evaluation: GateEvaluation,
    ) -> StoreResult<CanonicalMutationResult<GateEvaluation>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let requirements = self.trust_gate_requirements_unlocked(&context.execution_space_id)?;
        let requirement = requirements
            .get(&evaluation.requirement_id)
            .cloned()
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "gate requirement not found",
                    "gate_evaluation",
                    &evaluation.id,
                    None,
                )
            })?;
        if context.authenticated_actor != requirement.evaluator_ref
            || evaluation.performed_by != context.authenticated_actor
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "authenticated evaluator must exactly match the frozen GateRequirement evaluator",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        let mut dependency_ids = requirement.dependency_requirement_ids.clone();
        dependency_ids.sort();
        let expected_dependency_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(dependency_ids)?);
        let prior_evaluations = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let waivers = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        if requirement.dependency_requirement_ids.iter().any(|id| {
            requirements.get(id).is_none_or(|dependency| {
                !gate_requirement_is_satisfied(
                    dependency,
                    &requirements,
                    &prior_evaluations,
                    &waivers,
                    &mut BTreeSet::new(),
                )
            })
        }) {
            return Err(trust_error(
                TrustErrorCode::GateEvaluationRequired,
                "gate dependencies must be satisfied before evaluation",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        if requirement.work_id != evaluation.work_id
            || requirement.work_revision != evaluation.work_revision
            || requirement.work_report_id != evaluation.work_report_id
            || requirement.candidate_fingerprint != evaluation.candidate_fingerprint
            || requirement.config_fingerprint != evaluation.config_fingerprint
            || requirement.evaluator_version != evaluation.evaluator_version
            || requirement.evaluator_fingerprint != evaluation.evaluator_fingerprint
            || evaluation.dependency_fingerprint != expected_dependency_fingerprint
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "evaluation does not exactly match the frozen requirement",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .remove(&waiver.requirement_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "waiver references a missing gate requirement",
                    "gate_waiver",
                    &waiver.id,
                    None,
                )
            })?;
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
        self.commit_trust_projection_unlocked(
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

    pub fn revoke_trust_gate_waiver(
        &self,
        context: &MutationContext,
        waiver_id: &str,
        revoked_at: &str,
    ) -> StoreResult<CanonicalMutationResult<GateWaiver>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut waiver = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .remove(waiver_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "gate waiver not found",
                    "gate_waiver",
                    waiver_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<GateWaiver>(&envelope))?;
        if waiver.state != GateWaiverState::Active
            || context.authority_actor.as_ref() != Some(&waiver.authority_actor)
            || context.authenticated_actor != waiver.performed_by_actor
        {
            return Err(trust_error(
                TrustErrorCode::GateWaiverUnauthorized,
                "only the exact authorized actor may revoke an active waiver",
                "gate_waiver",
                waiver_id,
                Some(waiver.version),
            ));
        }
        waiver.state = GateWaiverState::Revoked;
        waiver.version += 1;
        waiver.revoked_at = Some(revoked_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "gate_waiver",
            waiver_id,
            "revoked",
            serde_json::json!({"revoked_at": revoked_at}),
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
            .trust_gate_requirements_unlocked(execution_space_id)?
            .into_values()
            .filter(|requirement| {
                requirement.work_id == work_id
                    && requirement.work_revision == work_revision
                    && requirement.work_report_id == report_id
                    && requirement.candidate_fingerprint == candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let mut requirement_ids = requirements
            .iter()
            .filter(|requirement| requirement.required)
            .map(|requirement| requirement.id.clone())
            .collect::<Vec<_>>();
        requirement_ids.sort();
        let expected_set_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(requirement_ids)?);
        if requirements
            .iter()
            .filter(|requirement| requirement.required)
            .any(|requirement| requirement.requirement_set_fingerprint != expected_set_fingerprint)
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "gate requirement set fingerprint is stale",
                "work",
                work_id,
                Some(work_revision),
            ));
        }
        let bindings = self
            .latest_trust_envelopes_unlocked(execution_space_id, "work_module_binding")?
            .into_values()
            .map(|envelope| event_projection::<WorkModuleBinding>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        for requirement in &requirements {
            if requirement.source == GateRequirementSource::Module {
                let binding = requirement
                    .source_binding_id
                    .as_deref()
                    .and_then(|id| bindings.iter().find(|binding| binding.id == id))
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::GateRequirementStale,
                            "module-derived gate lost its source binding",
                            "work",
                            work_id,
                            Some(work_revision),
                        )
                    })?;
                if binding.work_id != requirement.work_id
                    || binding.work_revision != requirement.work_revision
                    || binding.config_fingerprint
                        != requirement
                            .resolved_config
                            .get("module_config_fingerprint")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    || binding.version
                        != requirement
                            .resolved_config
                            .get("module_binding_version")
                            .and_then(Value::as_u64)
                            .unwrap_or_default()
                {
                    return Err(trust_error(
                        TrustErrorCode::GateRequirementStale,
                        "module-derived gate no longer matches its frozen source binding",
                        "work",
                        work_id,
                        Some(work_revision),
                    ));
                }
            }
        }
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
        let requirement_map = requirements
            .iter()
            .cloned()
            .map(|requirement| (requirement.id.clone(), requirement))
            .collect::<BTreeMap<_, _>>();
        for requirement in requirements
            .into_iter()
            .filter(|requirement| requirement.required)
        {
            if !gate_requirement_is_satisfied(
                &requirement,
                &requirement_map,
                &evaluations,
                &waivers,
                &mut BTreeSet::new(),
            ) {
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

    pub fn accept_trust_work(
        &self,
        context: &MutationContext,
        team_id: &str,
        work_id: &str,
        report_id: &str,
        candidate_fingerprint: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "team_id": team_id,
            "work_id": work_id,
            "work_report_id": report_id,
            "candidate_fingerprint": candidate_fingerprint,
            "updated_at": updated_at,
        });
        let request_fingerprint = canonical_json_fingerprint(&request_payload);
        if let Some(replay) =
            self.trust_operation_envelopes_unlocked()?
                .into_iter()
                .find(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                        && envelope.authenticated_actor_id == context.authenticated_actor.id
                        && envelope.command_name == context.command_name
                        && envelope.operation.event.idempotency_key == context.idempotency_key
                })
        {
            if replay.operation.event.canonical_request_fingerprint != request_fingerprint
                || replay.operation.event.aggregate_kind != "work"
                || replay.operation.event.aggregate_id != work_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used for a different Work acceptance",
                    "work",
                    work_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(&replay)?,
                event: replay.operation.event,
                replayed: true,
            });
        }
        let current = self.trust_team_work_unlocked(team_id, work_id, context.expected_version)?;
        if current.phase != firm_core::WorkPhase::Review
            || current.condition != firm_core::WorkCondition::Normal
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Work must be in normal review before acceptance",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        if current.owner_member_id.as_deref() == Some(context.authenticated_actor.id.as_str()) {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "the accountable Work owner cannot accept its own candidate",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        let report = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "work_report")?
            .remove(report_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::ReportEvidenceMissing,
                    "exact result WorkReport not found",
                    "work",
                    work_id,
                    Some(current.version),
                )
            })
            .and_then(|envelope| event_projection::<WorkReport>(&envelope))?;
        if report.kind != WorkReportKind::Result
            || report.work_id != current.id
            || report.work_revision != current.version
            || report.candidate.is_none()
            || report.candidate_fingerprint.as_deref() != Some(candidate_fingerprint)
            || report.evidence_refs.is_empty()
        {
            return Err(trust_error(
                TrustErrorCode::ReportEvidenceMissing,
                "acceptance requires the exact result Report, Candidate and evidence",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        self.trust_gate_satisfied(
            &context.execution_space_id,
            work_id,
            current.version,
            report_id,
            candidate_fingerprint,
        )?;
        let requirements = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .into_values()
            .filter(|requirement| {
                requirement.work_id == work_id
                    && requirement.work_revision == current.version
                    && requirement.work_report_id == report_id
                    && requirement.candidate_fingerprint == candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let requirement_ids = requirements
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect::<BTreeSet<_>>();
        let evaluations = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|evaluation| requirement_ids.contains(evaluation.requirement_id.as_str()))
            .collect::<Vec<_>>();
        let waivers = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|waiver| requirement_ids.contains(waiver.requirement_id.as_str()))
            .collect::<Vec<_>>();
        let mut next = current;
        next.phase = firm_core::WorkPhase::Closed;
        next.condition = firm_core::WorkCondition::Normal;
        next.resolution = Some(firm_core::WorkResolution::Accepted);
        next.result_summary = Some(report.summary.clone());
        next.version += 1;
        next.updated_at = updated_at.to_string();
        let actor_kind = match context.authenticated_actor.kind {
            ActorKind::Human => TeamActorKind::Operator,
            ActorKind::AgentMember => TeamActorKind::AgentMember,
            ActorKind::External => TeamActorKind::Operator,
            ActorKind::Service => TeamActorKind::Service,
        };
        let rollup_context = WorkCommandContext {
            event_id: format!("trust-accept:{}", context.idempotency_key),
            performed_by_actor: TeamActorRef {
                kind: actor_kind,
                id: context.authenticated_actor.id.clone(),
                display_name: None,
                authn_source: Some("agentfirm-trust-kernel".into()),
            },
            authority_actor: context
                .authority_actor
                .as_ref()
                .map(|authority| TeamActorRef {
                    kind: match authority.kind {
                        ActorKind::Human => TeamActorKind::Operator,
                        ActorKind::AgentMember => TeamActorKind::AgentMember,
                        ActorKind::External => TeamActorKind::Operator,
                        ActorKind::Service => TeamActorKind::Service,
                    },
                    id: authority.id.clone(),
                    display_name: None,
                    authn_source: Some("agentfirm-trust-kernel".into()),
                }),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            created_at: updated_at.to_string(),
            duplicate_ok: false,
        };
        let delegation_revisions =
            self.work_delegation_rollup_revisions_unlocked(&next, &rollup_context)?;
        let side_records = std::iter::once(serde_json::to_value(&report)?)
            .chain(
                requirements
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                evaluations
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                waivers
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                delegation_revisions
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .collect();
        self.commit_trust_work_acceptance_unlocked(context, request_payload, &next, side_records)
    }

    pub fn create_trust_workspace_binding(
        &self,
        context: &MutationContext,
        mut binding: MemberWorkspaceBinding,
    ) -> StoreResult<CanonicalMutationResult<MemberWorkspaceBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
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
        if path.exists() {
            let observed = observe_workspace_safety(path)?;
            if observed.canonical_root != path {
                return Err(trust_error(
                    TrustErrorCode::WorkspacePathUnsafe,
                    "canonical_root must equal the filesystem canonical path",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if !observed.link_escape_free {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceLinkEscape,
                    "workspace tree contains a symbolic-link escape",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if matches!(
                binding.mode,
                WorkspaceMode::Worktree | WorkspaceMode::SharedLive
            ) && observed.git_common_dir.is_none()
            {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "worktree/shared_live workspace must resolve a Git common directory",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if let (Some(expected), Some(actual)) = (
                binding.git_common_dir.as_deref(),
                observed.git_common_dir.as_ref(),
            ) {
                let expected = canonical_git_path(path, expected)?;
                if &expected != actual {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceRepositoryMismatch,
                        "workspace Git common directory does not match the binding",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
            }
            binding.git_common_dir = observed
                .git_common_dir
                .map(|value| value.display().to_string());
            binding.dirty_fingerprint = observed.dirty_fingerprint;
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
        self.commit_trust_projection_unlocked(
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
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        if proof.project_binding_id != binding.project_binding_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace ProjectBinding does not match",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let root = Path::new(&binding.canonical_root);
        let observed = if root.exists() {
            Some(observe_workspace_safety(root)?)
        } else {
            None
        };
        if next == WorkspaceLifecycle::Removed {
            if observed.is_some() {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceCleanupBlocked,
                    "workspace cleanup cannot complete while canonical_root still exists",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
        } else if next != WorkspaceLifecycle::Preparing && observed.is_none() {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "workspace path is missing for the requested lifecycle transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if let Some(observed) = observed.as_ref() {
            if observed.canonical_root != root {
                return Err(trust_error(
                    TrustErrorCode::WorkspacePathUnsafe,
                    "workspace path no longer equals its canonical filesystem path",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if !observed.link_escape_free || !proof.link_escape_free {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceLinkEscape,
                    "workspace contains a symlink/reparse escape",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if matches!(
                binding.mode,
                WorkspaceMode::Worktree | WorkspaceMode::SharedLive
            ) && observed.git_common_dir.is_none()
            {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "workspace no longer resolves the required Git repository",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if let Some(expected) = binding.git_common_dir.as_deref() {
                let expected = canonical_git_path(root, expected)?;
                if observed.git_common_dir.as_ref() != Some(&expected)
                    || proof
                        .git_common_dir
                        .as_deref()
                        .map(|value| canonical_git_path(root, value))
                        .transpose()?
                        .as_ref()
                        != Some(&expected)
                {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceRepositoryMismatch,
                        "workspace Git identity differs from binding or safety proof",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
            }
            if !proof.repository_matches {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "workspace safety proof did not affirm the bound repository",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if observed.conflicted != proof.is_conflicted {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceConflicted,
                    "workspace conflict proof differs from the filesystem observation",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if observed.dirty != proof.is_dirty {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceDirty,
                    "workspace dirty proof differs from the filesystem observation",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            binding.dirty_fingerprint = observed.dirty_fingerprint.clone();
        } else if !proof.link_escape_free {
            return Err(trust_error(
                TrustErrorCode::WorkspaceLinkEscape,
                "workspace safety proof did not establish a link-safe path",
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
        self.commit_trust_projection_unlocked(
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

impl HarnessStore {
    fn latest_fabric_side_records_unlocked<T, F>(
        &self,
        execution_space_id: &str,
        mut id: F,
    ) -> StoreResult<BTreeMap<String, T>>
    where
        T: for<'de> Deserialize<'de>,
        F: FnMut(&T) -> String,
    {
        let mut rows = BTreeMap::new();
        for row in self.trust_side_records::<T>(execution_space_id)? {
            rows.insert(id(&row), row);
        }
        Ok(rows)
    }

    #[allow(clippy::too_many_arguments)]
    fn require_current_node_daemon_unlocked(
        &self,
        execution_space_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        actor: &ActorRef,
        resource_kind: &str,
        resource_id: &str,
    ) -> StoreResult<()> {
        if actor.kind != ActorKind::Service || actor.id != daemon_id {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "runtime mutation requires the exact authenticated NodeDaemon service",
                resource_kind,
                resource_id,
                None,
            ));
        }
        let lease = self.latest_node_daemon_lease(node_id)?.ok_or_else(|| {
            trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "NodeDaemon lease is missing",
                resource_kind,
                resource_id,
                None,
            )
        })?;
        let registered = self
            .latest_node_project_registrations()?
            .iter()
            .any(|registration| {
                registration.node_id == node_id
                    && registration.execution_space_id == execution_space_id
                    && registration.status == firm_core::NodeProjectRegistrationStatus::Active
            });
        if !registered
            || lease.daemon_id != daemon_id
            || lease.generation != daemon_generation
            || lease.status != firm_core::NodeDaemonLeaseStatus::Active
            || lease.expires_unix_ms <= current_unix_ms()
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "runtime mutation used a stale, foreign, or expired NodeDaemon generation",
                resource_kind,
                resource_id,
                None,
            ));
        }
        Ok(())
    }

    pub fn fabric_agent_identities(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<AgentIdentity>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_identity")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn create_agent_identity(
        &self,
        context: &MutationContext,
        identity: AgentIdentity,
    ) -> StoreResult<CanonicalMutationResult<AgentIdentity>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&identity.id, "AgentIdentity.id")?;
        required(&identity.display_name, "AgentIdentity.display_name")?;
        if identity.version != 1 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "new AgentIdentity must start at version 1",
                "agent_identity",
                &identity.id,
                Some(identity.version),
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "agent_identity",
            &identity.id,
            "created",
            serde_json::to_value(&identity)?,
            &identity,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn fabric_agent_sessions(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<AgentSession>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_session")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn create_agent_session(
        &self,
        context: &MutationContext,
        session: AgentSession,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&session.id, "AgentSession.id")?;
        required(&session.agent_identity_id, "AgentSession.agent_identity_id")?;
        required(&session.node_id, "AgentSession.node_id")?;
        required(&session.provider_kind, "AgentSession.provider_kind")?;
        if session.execution_space_id != context.execution_space_id || session.version != 1 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentSession must start at version 1 in the authenticated Execution Space",
                "agent_session",
                &session.id,
                Some(session.version),
            ));
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            &session.id,
        )?;
        let identity = self
            .fabric_agent_identities(&context.execution_space_id)?
            .into_iter()
            .find(|identity| identity.id == session.agent_identity_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession references a missing AgentIdentity",
                    "agent_session",
                    &session.id,
                    None,
                )
            })?;
        if identity.organization_status != AgentMemberOrganizationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession requires an active AgentIdentity",
                "agent_session",
                &session.id,
                None,
            ));
        }
        if session.effective_permission_ceiling > identity.permission_ceiling {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentSession effective permission exceeds the frozen AgentIdentity ceiling",
                "agent_session",
                &session.id,
                None,
            ));
        }
        let current_count = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .filter(|row| {
                row.agent_identity_id == session.agent_identity_id
                    && row.lifecycle != AgentSessionStatus::Closed
            })
            .count();
        if current_count != 0 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentIdentity already has a current AgentSession; explicit stop or recovery is required",
                "agent_identity",
                &session.agent_identity_id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "agent_session",
            &session.id,
            "created",
            serde_json::to_value(&session)?,
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn fabric_team_memberships(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<TeamMembership>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "team_membership")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn join_team_membership(
        &self,
        context: &MutationContext,
        membership: TeamMembership,
    ) -> StoreResult<CanonicalMutationResult<TeamMembership>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&membership.id, "TeamMembership.id")?;
        required(&membership.team_id, "TeamMembership.team_id")?;
        required(
            &membership.agent_identity_id,
            "TeamMembership.agent_identity_id",
        )?;
        if membership.revision != 1 || membership.state != TeamMembershipStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new TeamMembership must be active at version 1",
                "team_membership",
                &membership.id,
                Some(membership.revision),
            ));
        }
        let run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|run| run.agent_team_id == membership.team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing TeamRun",
                    "team_membership",
                    &membership.id,
                    None,
                )
            })?;
        if run.execution_node_id != membership.node_id {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership must remain on the TeamRun's one pinned machine",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        if !self
            .fabric_agent_identities(&context.execution_space_id)?
            .iter()
            .any(|identity| identity.id == membership.agent_identity_id)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "TeamMembership references a missing AgentIdentity",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        // Membership is a generation-fenced collaboration binding.  The
        // cardinality check and the append deliberately share this Store
        // write lock so two concurrent joins cannot both observe an empty
        // active set and create ambiguous authority.
        let prior_memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        if prior_memberships.iter().any(|row| {
            row.team_id == membership.team_id
                && row.agent_identity_id == membership.agent_identity_id
                && row.state == TeamMembershipStatus::Active
        }) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team and AgentIdentity already have an active TeamMembership generation",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        let expected_generation = prior_memberships
            .iter()
            .filter(|row| {
                row.team_id == membership.team_id
                    && row.agent_identity_id == membership.agent_identity_id
            })
            .map(|row| row.membership_generation)
            .max()
            .unwrap_or(0)
            + 1;
        if membership.membership_generation != expected_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                format!(
                    "TeamMembership generation must be the exact successor generation {expected_generation}"
                ),
                "team_membership",
                &membership.id,
                Some(expected_generation.saturating_sub(1)),
            ));
        }
        let direct = MessageSubscription {
            id: format!("direct:{}:{}", membership.agent_identity_id, membership.id),
            subscriber_agent_id: membership.agent_identity_id.clone(),
            execution_space_id: context.execution_space_id.clone(),
            source_kind: MessageSubscriptionKind::Agent,
            source_ref: "active_team_members".into(),
            delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
            membership_ref: Some(membership.id.clone()),
            authorization_policy_ref: "team.direct.active-members".into(),
            policy_revision: 1,
            policy_digest: canonical_json_fingerprint(
                &serde_json::json!({"team_id": membership.team_id, "kind": "direct_from_active_team_members"}),
            ),
            status: MessageSubscriptionStatus::Active,
            revision: 1,
            created_by: membership.created_by.clone(),
            created_at: membership.joined_at.clone(),
            revoked_at: None,
        };
        let team = MessageSubscription {
            id: format!("team:{}:{}", membership.team_id, membership.id),
            subscriber_agent_id: membership.agent_identity_id.clone(),
            execution_space_id: context.execution_space_id.clone(),
            source_kind: MessageSubscriptionKind::Team,
            source_ref: membership.team_id.clone(),
            delivery_mode: if membership.role
                == firm_core::agentfirm_api::TeamMembershipRole::Observer
            {
                firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly
            } else {
                firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle
            },
            history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
            membership_ref: Some(membership.id.clone()),
            authorization_policy_ref: "team.channel.membership".into(),
            policy_revision: 1,
            policy_digest: canonical_json_fingerprint(
                &serde_json::json!({"team_id": membership.team_id, "kind": "team_channel"}),
            ),
            status: MessageSubscriptionStatus::Active,
            revision: 1,
            created_by: membership.created_by.clone(),
            created_at: membership.joined_at.clone(),
            revoked_at: None,
        };
        self.commit_trust_projection_unlocked(
            context,
            "team_membership",
            &membership.id,
            "joined",
            serde_json::to_value(&membership)?,
            &membership,
            vec![serde_json::to_value(direct)?, serde_json::to_value(team)?],
            Vec::new(),
        )
    }

    pub fn fabric_message_subscriptions(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MessageSubscription>> {
        Ok(self
            .latest_fabric_side_records_unlocked(
                execution_space_id,
                |row: &MessageSubscription| row.id.clone(),
            )?
            .into_values()
            .collect())
    }

    pub fn leave_team_membership(
        &self,
        context: &MutationContext,
        membership_id: &str,
        ended_at: &str,
    ) -> StoreResult<CanonicalMutationResult<TeamMembership>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut membership = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "team_membership")?
            .remove(membership_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership not found",
                    "team_membership",
                    membership_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<TeamMembership>(&envelope))?;
        if membership.state != TeamMembershipStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an active TeamMembership can leave",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let active_bindings = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .filter(|binding| {
                binding.team_membership_id == membership.id
                    && binding.status == WorkExecutionBindingStatus::Active
            })
            .map(|binding| binding.work_id)
            .collect::<Vec<_>>();
        if !active_bindings.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "TeamMembership cannot leave with active WorkExecutionBindings: {}",
                    active_bindings.join(",")
                ),
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        if context.authenticated_actor.kind != ActorKind::AgentMember
            || context.authenticated_actor.id != membership.agent_identity_id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership leave requires the exact stable AgentIdentity",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let revoked = self
            .fabric_message_subscriptions(&context.execution_space_id)?
            .into_iter()
            .filter(|subscription| {
                subscription.membership_ref.as_deref() == Some(membership_id)
                    && subscription.status == MessageSubscriptionStatus::Active
            })
            .map(|mut subscription| {
                subscription.status = MessageSubscriptionStatus::Revoked;
                subscription.revision += 1;
                subscription.revoked_at = Some(ended_at.to_string());
                serde_json::to_value(subscription)
            })
            .collect::<Result<Vec<_>, _>>()?;
        membership.state = TeamMembershipStatus::Inactive;
        membership.revision += 1;
        membership.left_at = Some(ended_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "team_membership",
            membership_id,
            "left",
            serde_json::json!({"ended_at": ended_at}),
            &membership,
            revoked,
            Vec::new(),
        )
    }

    pub fn transition_agent_session(
        &self,
        context: &MutationContext,
        session_id: &str,
        next_status: AgentSessionStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut session = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_session")?
            .remove(session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession not found",
                    "agent_session",
                    session_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentSession>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            session_id,
        )?;
        let executing_runtime_key = context.idempotency_key.strip_suffix(":effect");
        let runtime_commands = self.runtime_commands(&context.execution_space_id)?;
        let authorized_stop = executing_runtime_key.is_some_and(|key| {
            runtime_commands.iter().any(|command| {
                command.idempotency_key == key
                    && command.command == RuntimeCommandKind::StopSession
                    && command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && command.target_node_daemon_id == session.node_daemon_id
                    && command.target_node_daemon_generation == session.node_daemon_generation
                    && matches!(
                        (command.status, command.effect_certainty),
                        (
                            RuntimeCommandStatus::Accepted,
                            RuntimeEffectCertainty::Unknown
                        ) | (
                            RuntimeCommandStatus::Applied,
                            RuntimeEffectCertainty::Applied
                        )
                    )
            })
        });
        let executing_stop = authorized_stop
            && runtime_commands.iter().any(|command| {
                executing_runtime_key == Some(command.idempotency_key.as_str())
                    && command.status == RuntimeCommandStatus::Accepted
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
            });
        let allowed = matches!(
            (session.lifecycle, next_status),
            (AgentSessionStatus::Cold, AgentSessionStatus::Idle)
                | (
                    AgentSessionStatus::Cold,
                    AgentSessionStatus::RecoveryRequired
                )
                | (AgentSessionStatus::Idle, AgentSessionStatus::Active)
                | (AgentSessionStatus::Idle, AgentSessionStatus::Closed)
                | (AgentSessionStatus::Active, AgentSessionStatus::Waiting)
                | (AgentSessionStatus::Active, AgentSessionStatus::Idle)
                | (AgentSessionStatus::Active, AgentSessionStatus::Interrupted)
                | (
                    AgentSessionStatus::Active,
                    AgentSessionStatus::RecoveryRequired
                )
                | (AgentSessionStatus::Waiting, AgentSessionStatus::Active)
                | (AgentSessionStatus::Waiting, AgentSessionStatus::Idle)
                | (AgentSessionStatus::Waiting, AgentSessionStatus::Closed)
                | (AgentSessionStatus::Interrupted, AgentSessionStatus::Cold)
                | (AgentSessionStatus::Interrupted, AgentSessionStatus::Closed)
        ) || (matches!(
            session.lifecycle,
            AgentSessionStatus::Cold | AgentSessionStatus::Active
        ) && next_status == AgentSessionStatus::Closed
            && authorized_stop);
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "invalid AgentSession transition {:?}->{next_status:?}",
                    session.lifecycle
                ),
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if matches!(
            next_status,
            AgentSessionStatus::Closed | AgentSessionStatus::Interrupted
        ) {
            let active_work = self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .into_iter()
                .any(|binding| {
                    binding.agent_session_id == session.id
                        && binding.agent_session_generation == session.runtime_generation
                        && binding.status == WorkExecutionBindingStatus::Active
                });
            let uncertain_command = runtime_commands.into_iter().any(|command| {
                command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && matches!(
                        command.status,
                        RuntimeCommandStatus::Accepted
                            | RuntimeCommandStatus::Quiesced
                            | RuntimeCommandStatus::RecoveryRequired
                    )
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
                    && !(executing_stop
                        && executing_runtime_key == Some(command.idempotency_key.as_str()))
            });
            if active_work || uncertain_command {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    if active_work {
                        "AgentSession cannot close or interrupt while an active WorkExecutionBinding exists; release or atomically rebind it first"
                    } else {
                        "AgentSession cannot close or interrupt while a RuntimeCommand effect is ambiguous; reconcile it first"
                    },
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
        }
        session.lifecycle = next_status;
        session.version += 1;
        session.last_active_at = updated_at.to_string();
        match next_status {
            AgentSessionStatus::Active => {
                session.current_turn_id =
                    Some(format!("provider-turn:{}:{}", session.id, session.version));
                session.queued_input_count = session.queued_input_count.saturating_sub(1);
            }
            AgentSessionStatus::Idle
            | AgentSessionStatus::Waiting
            | AgentSessionStatus::Interrupted
            | AgentSessionStatus::RecoveryRequired
            | AgentSessionStatus::Closed => session.current_turn_id = None,
            AgentSessionStatus::Cold => {}
        }
        if next_status == AgentSessionStatus::Closed {
            session.closed_at = Some(updated_at.to_string());
        }
        self.commit_trust_projection_unlocked(
            context,
            "agent_session",
            session_id,
            "status_changed",
            serde_json::json!({"status": next_status, "updated_at": updated_at}),
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn fabric_work_execution_bindings(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<WorkExecutionBinding>> {
        let mut latest = BTreeMap::<String, WorkExecutionBinding>::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id != execution_space_id {
                continue;
            }
            if envelope.operation.event.aggregate_kind == "work_execution_binding" {
                let binding = event_projection::<WorkExecutionBinding>(&envelope)?;
                latest.insert(binding.id.clone(), binding);
            }
            // StopSession atomically quiesces active Work bindings in the same
            // RuntimeCommand operation. Side records are full resulting
            // projections and participate in latest-version selection.
            for record in envelope.operation.immutable_side_records {
                if let Ok(binding) = serde_json::from_value::<WorkExecutionBinding>(record) {
                    let replace = latest
                        .get(&binding.id)
                        .is_none_or(|current| binding.version > current.version);
                    if replace {
                        latest.insert(binding.id.clone(), binding);
                    }
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn fabric_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CanonicalWorkDelivery>> {
        Ok(self
            .latest_fabric_side_records_unlocked(
                execution_space_id,
                |row: &CanonicalWorkDelivery| row.id.clone(),
            )?
            .into_values()
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_for_provider(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        dispatch_mode: firm_core::agentfirm_api::RuntimeDispatchMode,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<ProviderInvocation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalWorkDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        let binding = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .find(|binding| binding.id == delivery.work_execution_binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery binding is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|session| session.id == delivery.recipient_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "WorkDelivery session is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let work = self
            .latest_works_unlocked()?
            .remove(&delivery.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkDelivery Work is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Queued
            || delivery.target_node_id != node_id
            || binding.status != WorkExecutionBindingStatus::Active
            || binding.work_revision != work.version
            || delivery.work_revision != work.version
            || session.agent_identity_id != delivery.recipient_identity_id
            || session.runtime_generation != delivery.recipient_session_generation
            || session.node_daemon_id != daemon_id
            || session.node_daemon_generation != daemon_generation
            || session.lifecycle == AgentSessionStatus::Closed
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkDelivery no longer matches its exact active binding, Work revision, session, or NodeDaemon generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_node_daemon_generation = Some(daemon_generation);
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let content = serde_json::to_string(&serde_json::json!({
            "work_id": work.id,
            "work_revision": work.version,
            "title": work.title,
            "context_markdown": work.context_markdown,
            "completion_criteria_markdown": work.completion_criteria_markdown,
        }))?;
        let invocation = ProviderInvocation {
            id: format!("provider-invocation:{}:{}", delivery.id, delivery.attempt),
            source_plane: "work_delivery".into(),
            source_record_id: delivery.id.clone(),
            recipient_identity_id: delivery.recipient_identity_id.clone(),
            recipient_session_id: delivery.recipient_session_id.clone(),
            recipient_session_generation: delivery.recipient_session_generation,
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
            provider: session.provider_kind,
            dispatch_mode,
            permission_ceiling: session.effective_permission_ceiling,
            content_fingerprint: canonical_json_fingerprint(
                &serde_json::json!({"content": content}),
            ),
            content,
            created_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "provider_invocation",
            &invocation.id,
            "prepared_from_work_delivery",
            serde_json::json!({"delivery_id": delivery_id, "claim_id": claim_id}),
            &invocation,
            vec![serde_json::to_value(delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_work_provider_receipt(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalWorkDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalWorkDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
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
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_node_daemon_generation != Some(daemon_generation)
            || delivery.target_node_id != node_id
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt does not match the exact WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery_receipt",
            delivery_id,
            "provider_received",
            serde_json::json!({"claim_id": claim_id, "provider_receipt_id": provider_receipt_id}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn bind_work_execution(
        &self,
        context: &MutationContext,
        binding: WorkExecutionBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if binding.version != 1 || binding.status != WorkExecutionBindingStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new WorkExecutionBinding must be active at version 1",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let membership = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.team_membership_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing TeamMembership",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.agent_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing AgentSession",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        let work = self
            .latest_works_unlocked()?
            .remove(&binding.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkExecutionBinding references a missing Work",
                    "work",
                    &binding.work_id,
                    None,
                )
            })?;
        if membership.state != TeamMembershipStatus::Active
            || membership.agent_identity_id != binding.agent_identity_id
            || session.agent_identity_id != binding.agent_identity_id
            || session.node_id != membership.node_id
            || session.runtime_generation != binding.agent_session_generation
            || session.lifecycle == AgentSessionStatus::Closed
            || work.version != binding.work_revision
            || work.team_id.as_deref() != Some(membership.team_id.as_str())
            || binding.team_id != membership.team_id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding identity, session generation, Team, or Work revision mismatch",
                "work_execution_binding",
                &binding.id,
                None,
            ));
        }
        if self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .iter()
            .any(|row| {
                row.work_id == binding.work_id && row.status == WorkExecutionBindingStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Work already has an active WorkExecutionBinding; explicit release is required",
                "work",
                &binding.work_id,
                Some(work.version),
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            &binding.id,
            "bound",
            serde_json::to_value(&binding)?,
            &binding,
            vec![serde_json::to_value(CanonicalWorkDelivery {
                id: format!(
                    "work-delivery:{}:{}",
                    binding.work_id, binding.binding_generation
                ),
                work_id: binding.work_id.clone(),
                work_revision: binding.work_revision,
                work_execution_binding_id: binding.id.clone(),
                recipient_identity_id: binding.agent_identity_id.clone(),
                recipient_session_id: binding.agent_session_id.clone(),
                recipient_session_generation: binding.agent_session_generation,
                target_node_id: session.node_id.clone(),
                status: WorkDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_node_daemon_generation: None,
                provider_receipt_id: None,
                failure_code: None,
                version: 1,
                created_at: binding.bound_at.clone(),
                updated_at: binding.bound_at.clone(),
            })?],
            Vec::new(),
        )
    }

    pub fn release_work_execution_binding(
        &self,
        context: &MutationContext,
        binding_id: &str,
        ended_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut binding = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "work_execution_binding")?
            .remove(binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding not found",
                    "work_execution_binding",
                    binding_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<WorkExecutionBinding>(&envelope))?;
        if binding.status != WorkExecutionBindingStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an active WorkExecutionBinding can be released",
                "work_execution_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let exact_member = context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == binding.agent_identity_id;
        let host_or_operator = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        );
        if !exact_member && !host_or_operator {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding release requires exact member or control-plane authority",
                "work_execution_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        binding.status = WorkExecutionBindingStatus::Released;
        binding.version += 1;
        binding.ended_at = Some(ended_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            binding_id,
            "released",
            serde_json::json!({"ended_at": ended_at}),
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn fabric_messages(&self, execution_space_id: &str) -> StoreResult<Vec<Message>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "message")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn fabric_message_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CanonicalMessageDelivery>> {
        Ok(self
            .latest_fabric_side_records_unlocked(
                execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .into_values()
            .collect())
    }

    pub fn author_message(
        &self,
        context: &MutationContext,
        message: Message,
    ) -> StoreResult<CanonicalMutationResult<Message>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&message.id, "Message.id")?;
        required(&message.sender_actor_ref.id, "Message.sender_actor_ref.id")?;
        required(&message.body, "Message.body")?;
        if message.source_execution_space_id != context.execution_space_id
            || message.recipients.is_empty()
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Message must have recipients in the authenticated Execution Space",
                "message",
                &message.id,
                None,
            ));
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &message.source_node_id,
            &message.source_node_daemon_id,
            message.source_authority_generation,
            &context.authenticated_actor,
            "message",
            &message.id,
        )?;
        if let Some(sender_agent_id) = message.sender_agent_id.as_deref() {
            let sender_sessions = self
                .fabric_agent_sessions(&context.execution_space_id)?
                .into_iter()
                .filter(|session| {
                    session.agent_identity_id == sender_agent_id
                        && session.node_id == message.source_node_id
                        && session.node_daemon_generation == message.source_authority_generation
                        && session.lifecycle != AgentSessionStatus::Closed
                        && message.sender_session_id.as_deref() == Some(session.id.as_str())
                })
                .count();
            if sender_sessions != 1 || message.sender_actor_ref.id != sender_agent_id {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "Agent Message author must resolve to the exact current local AgentSession",
                    "message",
                    &message.id,
                    None,
                ));
            }
        } else if context.authority_actor.as_ref() != Some(&message.sender_actor_ref) {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "Human/Service Message actor must be server-resolved as command authority",
                "message",
                &message.id,
                None,
            ));
        }
        let expected_fingerprint = message_content_fingerprint(&message);
        if message.content_fingerprint != expected_fingerprint {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Message content_fingerprint does not match immutable authored content",
                "message",
                &message.id,
                None,
            ));
        }
        crate::validate_message_collaboration_scope(&message)?;
        let subscriptions = self.fabric_message_subscriptions(&context.execution_space_id)?;
        let sessions = self.fabric_agent_sessions(&context.execution_space_id)?;
        let memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        if let Some(team_id) = message.team_id.as_deref() {
            let sender_is_member = message.sender_agent_id.as_deref().is_some_and(|sender| {
                memberships.iter().any(|membership| {
                    membership.team_id == team_id
                        && membership.agent_identity_id == sender
                        && membership.state == TeamMembershipStatus::Active
                })
            });
            let control_plane_sender = message.sender_agent_id.is_none()
                && context.authority_actor.as_ref() == Some(&message.sender_actor_ref);
            if !sender_is_member && !control_plane_sender {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "Message sender is not an active member or server-resolved control-plane actor for the Team",
                    "message",
                    &message.id,
                    None,
                ));
            }
        }
        let mut delivery_rows = Vec::new();
        let mut delivered_identities = BTreeSet::new();
        for recipient in &message.recipients {
            let matching = subscriptions.iter().filter(|subscription| {
                subscription.status == MessageSubscriptionStatus::Active
                    && match recipient.kind {
                        MessageRecipientKind::AgentIdentity => {
                            subscription.source_kind == MessageSubscriptionKind::Agent
                                && subscription.subscriber_agent_id == recipient.id
                                && if let Some(team_id) = message.team_id.as_deref() {
                                    subscription.membership_ref.as_deref().is_some_and(
                                        |membership_id| {
                                            memberships.iter().any(|membership| {
                                                membership.id == membership_id
                                                    && membership.state
                                                        == TeamMembershipStatus::Active
                                                    && membership.team_id == team_id
                                            })
                                        },
                                    )
                                } else {
                                    subscription.membership_ref.is_none()
                                        && message.sender_agent_id.as_deref()
                                            == Some(subscription.source_ref.as_str())
                                }
                        }
                        MessageRecipientKind::Team => {
                            subscription.source_kind == MessageSubscriptionKind::Team
                                && subscription.source_ref == recipient.id
                        }
                        MessageRecipientKind::ControlPlaneActor => false,
                    }
            });
            for subscription in matching {
                if !delivered_identities.insert(subscription.subscriber_agent_id.clone()) {
                    continue;
                }
                let current = sessions
                    .iter()
                    .filter(|session| {
                        session.agent_identity_id == subscription.subscriber_agent_id
                            && session.lifecycle != AgentSessionStatus::Closed
                    })
                    .collect::<Vec<_>>();
                if current.len() > 1 {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "recipient identity has multiple current AgentSessions",
                        "agent_identity",
                        &subscription.subscriber_agent_id,
                        None,
                    ));
                }
                let target_node_id = current
                    .first()
                    .map(|session| session.node_id.clone())
                    .or_else(|| {
                        subscription
                            .membership_ref
                            .as_ref()
                            .and_then(|membership_id| {
                                memberships
                                    .iter()
                                    .find(|membership| &membership.id == membership_id)
                                    .map(|membership| membership.node_id.clone())
                            })
                    })
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "recipient identity has no routable Node placement",
                            "agent_identity",
                            &subscription.subscriber_agent_id,
                            None,
                        )
                    })?;
                delivery_rows.push(CanonicalMessageDelivery {
                    id: format!("{}:{}", message.id, subscription.subscriber_agent_id),
                    message_id: message.id.clone(),
                    subscription_id: subscription.id.clone(),
                    recipient_identity_id: subscription.subscriber_agent_id.clone(),
                    target_node_id,
                    recipient_session_id: None,
                    recipient_session_generation: None,
                    status: CanonicalMessageDeliveryStatus::Queued,
                    attempt: 1,
                    claim_id: None,
                    claimed_node_daemon_generation: None,
                    provider_receipt_id: None,
                    failure_code: None,
                    failure_detail: None,
                    version: 1,
                    created_at: message.created_at.clone(),
                    updated_at: message.created_at.clone(),
                });
            }
        }
        let cross_node_collaboration = message
            .collaboration_scope
            .as_ref()
            .is_some_and(|scope| scope.source_team_id != scope.target_team_id);
        if delivery_rows.is_empty()
            && !cross_node_collaboration
            && !message
                .recipients
                .iter()
                .all(|recipient| recipient.kind == MessageRecipientKind::ControlPlaneActor)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Message recipients resolved to no active subscription",
                "message",
                &message.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "message",
            &message.id,
            "authored",
            serde_json::to_value(&message)?,
            &message,
            Vec::new(),
            delivery_rows
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    /// Persist an immutable source-authored cross-node Message before creating
    /// target-owned MessageDelivery rows. Fabric route journals remain the
    /// only cross-node route truth; this canonical operation records target
    /// application state and cannot re-author the Message.
    pub fn persist_remote_message(
        &self,
        context: &MutationContext,
        operation: &firm_fabric::RoutedOperation,
        message: Message,
        target_node_id: &str,
        target_daemon_id: &str,
        target_daemon_generation: u64,
    ) -> StoreResult<CanonicalMutationResult<Message>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            target_node_id,
            target_daemon_id,
            target_daemon_generation,
            &context.authenticated_actor,
            "message",
            &message.id,
        )?;
        let reference = match operation.closed_body().map_err(|error| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!("Remote Message route is invalid: {error}"),
                "message",
                &message.id,
                None,
            )
        })? {
            firm_fabric::ClosedOperationBody::Message(reference) => reference,
            firm_fabric::ClosedOperationBody::CollaborationBusiness(reference)
                if reference.business_kind == "team_message_deliver"
                    && reference.required_capability == "collaboration.team_message_deliver" =>
            {
                serde_json::from_value::<firm_fabric::MessageReference>(reference.payload).map_err(
                    |error| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            format!(
                                "team_message_deliver payload is not a MessageReference: {error}"
                            ),
                            "message",
                            &message.id,
                            None,
                        )
                    },
                )?
            }
            _ => {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Remote persistence requires a closed Message route",
                    "message",
                    &message.id,
                    None,
                ))
            }
        };
        if operation.target_node_id != target_node_id
            || operation.target_execution_space_id.as_deref()
                != Some(context.execution_space_id.as_str())
            || operation.source_execution_space_id.as_deref()
                != Some(message.source_execution_space_id.as_str())
            || operation.source_node_id.as_deref() != Some(message.source_node_id.as_str())
            || operation.source_node_daemon_id.as_deref()
                != Some(message.source_node_daemon_id.as_str())
            || operation.source_node_daemon_generation != Some(message.source_authority_generation)
            || reference.message_id != message.id
            || reference.body_digest != message.body_digest
            || reference.canonical_message_envelope.as_ref()
                != Some(&serde_json::to_value(&message)?)
            || message.body_digest
                != format!("sha256:{:x}", Sha256::digest(message.body.as_bytes()))
            || message.content_fingerprint != message_content_fingerprint(&message)
            || message.recipients.is_empty()
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "Remote Message or route disagrees with immutable source/target authority",
                "message",
                &message.id,
                None,
            ));
        }
        crate::validate_message_collaboration_scope(&message)?;
        let request_fingerprint = match context.request_fingerprint.clone() {
            Some(fingerprint) => fingerprint,
            None => canonical_json_fingerprint(&serde_json::to_value(operation)?),
        };
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "message",
            &message.id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let subscriptions = self.fabric_message_subscriptions(&context.execution_space_id)?;
        let sessions = self.fabric_agent_sessions(&context.execution_space_id)?;
        let memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        let mut deliveries = Vec::new();
        let mut delivered_identities = BTreeSet::new();
        for recipient in &message.recipients {
            for subscription in subscriptions.iter().filter(|subscription| {
                subscription.status == MessageSubscriptionStatus::Active
                    && match recipient.kind {
                        MessageRecipientKind::AgentIdentity => {
                            subscription.source_kind == MessageSubscriptionKind::Agent
                                && subscription.subscriber_agent_id == recipient.id
                        }
                        MessageRecipientKind::Team => {
                            subscription.source_kind == MessageSubscriptionKind::Team
                                && subscription.source_ref == recipient.id
                        }
                        MessageRecipientKind::ControlPlaneActor => false,
                    }
            }) {
                if !delivered_identities.insert(subscription.subscriber_agent_id.clone()) {
                    continue;
                }
                // `Message.team_id` remains the immutable source-Team scope.
                // On the target Node, recipient authorization must bind the
                // collaboration target Team; requiring a target membership in
                // the source Team would make every valid cross-Team transfer
                // undeliverable (or tempt a split-Team model).
                let recipient_team_id = message
                    .collaboration_scope
                    .as_ref()
                    .map(|scope| scope.target_team_id.as_str())
                    .or(message.team_id.as_deref());
                if let Some(team_id) = recipient_team_id {
                    let exact_membership =
                        subscription
                            .membership_ref
                            .as_deref()
                            .is_some_and(|membership_id| {
                                memberships.iter().any(|membership| {
                                    membership.id == membership_id
                                        && membership.team_id == team_id
                                        && membership.agent_identity_id
                                            == subscription.subscriber_agent_id
                                        && membership.node_id == target_node_id
                                        && membership.state == TeamMembershipStatus::Active
                                })
                            });
                    if !exact_membership {
                        continue;
                    }
                }
                let current = sessions
                    .iter()
                    .filter(|session| {
                        session.agent_identity_id == subscription.subscriber_agent_id
                            && session.node_id == target_node_id
                            && session.lifecycle != AgentSessionStatus::Closed
                    })
                    .collect::<Vec<_>>();
                if current.len() > 1 {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "remote Message recipient has ambiguous current AgentSession",
                        "agent_identity",
                        &subscription.subscriber_agent_id,
                        None,
                    ));
                }
                deliveries.push(CanonicalMessageDelivery {
                    id: format!("{}:{}", message.id, subscription.subscriber_agent_id),
                    message_id: message.id.clone(),
                    subscription_id: subscription.id.clone(),
                    recipient_identity_id: subscription.subscriber_agent_id.clone(),
                    target_node_id: target_node_id.into(),
                    recipient_session_id: current.first().map(|session| session.id.clone()),
                    recipient_session_generation: current
                        .first()
                        .map(|session| session.runtime_generation),
                    status: CanonicalMessageDeliveryStatus::Queued,
                    attempt: 1,
                    claim_id: None,
                    claimed_node_daemon_generation: None,
                    provider_receipt_id: None,
                    failure_code: None,
                    failure_detail: None,
                    version: 1,
                    created_at: message.created_at.clone(),
                    updated_at: message.created_at.clone(),
                });
            }
        }
        if deliveries.is_empty()
            && !message
                .recipients
                .iter()
                .all(|recipient| recipient.kind == MessageRecipientKind::ControlPlaneActor)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "remote Message has no authorized local recipient subscription",
                "message",
                &message.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "message",
            &message.id,
            "remote_persisted",
            serde_json::to_value(operation)?,
            &message,
            Vec::new(),
            deliveries
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_message_for_provider(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        dispatch_mode: firm_core::agentfirm_api::RuntimeDispatchMode,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<ProviderInvocation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "message_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.target_node_id != node_id
            || delivery.status != CanonicalMessageDeliveryStatus::Queued
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "only the target NodeDaemon can claim a queued MessageDelivery",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let current = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_identity_id == delivery.recipient_identity_id
                    && session.node_id == node_id
                    && session.node_daemon_id == daemon_id
                    && session.node_daemon_generation == daemon_generation
                    && session.lifecycle != AgentSessionStatus::Closed
            })
            .collect::<Vec<_>>();
        if current.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                if current.is_empty() {
                    "recipient has no current local AgentSession; delivery remains queued"
                } else {
                    "recipient identity has multiple current AgentSessions"
                },
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let session = &current[0];
        let message = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "message")?
            .remove(&delivery.message_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery references a missing Message",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })
            .and_then(|envelope| event_projection::<Message>(&envelope))?;
        delivery.status = CanonicalMessageDeliveryStatus::Claimed;
        delivery.recipient_session_id = Some(session.id.clone());
        delivery.recipient_session_generation = Some(session.runtime_generation);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_node_daemon_generation = Some(daemon_generation);
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let dispatch = ProviderInvocation {
            id: format!("provider-invocation:{}:{}", delivery.id, delivery.attempt),
            source_plane: "message".into(),
            source_record_id: message.id,
            recipient_identity_id: delivery.recipient_identity_id.clone(),
            recipient_session_id: session.id.clone(),
            recipient_session_generation: session.runtime_generation,
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
            provider: session.provider_kind.clone(),
            dispatch_mode,
            permission_ceiling: session.effective_permission_ceiling,
            content: message.body,
            content_fingerprint: message.content_fingerprint,
            created_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "provider_invocation",
            &dispatch.id,
            "prepared",
            serde_json::json!({
                "delivery_id": delivery_id,
                "claim_id": claim_id,
                "dispatch_mode": dispatch_mode,
            }),
            &dispatch,
            vec![serde_json::to_value(delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_message_provider_receipt(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "message_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.target_node_id != node_id
            || delivery.status != CanonicalMessageDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_node_daemon_generation != Some(daemon_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt does not match the exact delivery claim and NodeDaemon generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let session_id = delivery.recipient_session_id.as_deref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "claimed MessageDelivery did not freeze a recipient session",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            )
        })?;
        let current = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "frozen recipient session no longer exists",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        if Some(current.runtime_generation) != delivery.recipient_session_generation
            || current.node_daemon_generation != daemon_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "recipient session generation changed before provider receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = CanonicalMessageDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery_receipt",
            delivery_id,
            "provider_received",
            serde_json::json!({
                "delivery_id": delivery_id,
                "claim_id": claim_id,
                "provider_receipt_id": provider_receipt_id,
            }),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn acknowledge_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if context.authenticated_actor.kind != ActorKind::AgentMember
            || context.authenticated_actor.id != delivery.recipient_identity_id
            || delivery.status != CanonicalMessageDeliveryStatus::ProviderReceived
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "acknowledge requires the exact recipient identity after provider receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = CanonicalMessageDeliveryStatus::Acknowledged;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let current_cursor = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "subscription_cursor")?
            .remove(&delivery.subscription_id)
            .map(|envelope| event_projection::<SubscriptionCursor>(&envelope))
            .transpose()?;
        let cursor = SubscriptionCursor {
            subscription_id: delivery.subscription_id.clone(),
            recipient_agent_id: delivery.recipient_identity_id.clone(),
            last_visible_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_visible_store_sequence.saturating_add(1))
                .unwrap_or(1),
            last_delivered_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_delivered_store_sequence.saturating_add(1))
                .unwrap_or(1),
            last_read_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_read_store_sequence.saturating_add(1))
                .unwrap_or(1),
            cursor_revision: current_cursor
                .as_ref()
                .map(|cursor| cursor.cursor_revision + 1)
                .unwrap_or(1),
            updated_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery_ack",
            delivery_id,
            "acknowledged",
            serde_json::json!({"delivery_id": delivery_id, "updated_at": updated_at}),
            &delivery,
            vec![
                serde_json::to_value(&delivery)?,
                serde_json::to_value(cursor)?,
            ],
            Vec::new(),
        )
    }

    /// Operator-requested recovery is executed by the exact current target
    /// NodeDaemon. Replay is resolved before mutable delivery state, and an
    /// acknowledged provider receipt can never be converted into a retry.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_canonical_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        outcome: DeliveryReconcileOutcome,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(evidence_ref, "MessageDelivery reconciliation evidence_ref")?;
        let fingerprint = canonical_json_fingerprint(&serde_json::json!({
            "transport_request_fingerprint": context.request_fingerprint,
            "delivery_id": delivery_id,
            "node_id": node_id,
            "daemon_id": daemon_id,
            "daemon_generation": daemon_generation,
            "outcome": outcome,
            "evidence_ref": evidence_ref,
        }));
        let existing = self.trust_operation_envelopes_unlocked()?;
        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint
                || replay.operation.event.aggregate_kind != "canonical_message_delivery"
                || replay.operation.event.aggregate_id != delivery_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "MessageDelivery reconciliation key was reused with different semantics",
                    "canonical_message_delivery",
                    delivery_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "canonical_message_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "canonical MessageDelivery not found",
                    "canonical_message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.target_node_id != node_id || context.expected_version != delivery.version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "MessageDelivery recovery requires its exact target Node and revision",
                "canonical_message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        match outcome {
            DeliveryReconcileOutcome::Acknowledged => {
                if delivery.status != CanonicalMessageDeliveryStatus::ProviderReceived
                    || delivery.provider_receipt_id.is_none()
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "acknowledged recovery requires a durable provider receipt",
                        "canonical_message_delivery",
                        delivery_id,
                        Some(delivery.version),
                    ));
                }
                delivery.status = CanonicalMessageDeliveryStatus::Acknowledged;
            }
            DeliveryReconcileOutcome::RetrySafeFailure => {
                if delivery.status != CanonicalMessageDeliveryStatus::Claimed
                    || delivery.provider_receipt_id.is_some()
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "retry requires a claimed delivery with proven no provider receipt",
                        "canonical_message_delivery",
                        delivery_id,
                        Some(delivery.version),
                    ));
                }
                delivery.status = CanonicalMessageDeliveryStatus::Queued;
                delivery.attempt += 1;
                delivery.claim_id = None;
                delivery.claimed_node_daemon_generation = None;
                delivery.recipient_session_id = None;
                delivery.recipient_session_generation = None;
                delivery.failure_code = Some("RETRY_SAFE_FAILURE".into());
                delivery.failure_detail = Some(evidence_ref.to_string());
            }
        }
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let aggregate_version = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "canonical_message_delivery"
                    && envelope.operation.event.aggregate_id == delivery_id
            })
            .map(|envelope| envelope.operation.event.resulting_version)
            .max()
            .unwrap_or(0);
        let mut commit_context = context.clone();
        commit_context.expected_version = aggregate_version;
        commit_context.request_fingerprint = Some(fingerprint);
        self.commit_trust_projection_unlocked(
            &commit_context,
            "canonical_message_delivery",
            delivery_id,
            "reconciled",
            serde_json::json!({
                "outcome": outcome,
                "evidence_ref": evidence_ref,
                "daemon_generation": daemon_generation,
            }),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn validate_runtime_command(
        &self,
        command: &ControlCommandEnvelope,
        now_unix_ms: u64,
    ) -> StoreResult<()> {
        required(&command.id, "ControlCommandEnvelope.id")?;
        required(
            &command.idempotency_key,
            "ControlCommandEnvelope.idempotency_key",
        )?;
        required(
            &command.required_capability,
            "ControlCommandEnvelope.required_capability",
        )?;
        if command.payload_fingerprint != canonical_json_fingerprint(&command.payload) {
            return Err(trust_error(
                TrustErrorCode::IdempotencyKeyReused,
                "runtime command payload fingerprint is invalid",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        if command.authenticated_actor.kind == ActorKind::External {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "external actors cannot issue machine runtime commands",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        if command.expires_unix_ms <= now_unix_ms {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "runtime command expired before NodeDaemon admission",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        self.require_current_node_daemon_unlocked(
            &command.execution_space_id,
            &command.target_node_id,
            &command.target_node_daemon_id,
            command.target_node_daemon_generation,
            &ActorRef {
                kind: ActorKind::Service,
                id: command.target_node_daemon_id.clone(),
            },
            "runtime_command",
            &command.id,
        )
    }

    pub fn runtime_commands(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<RuntimeCommandRecord>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "runtime_command")?
            .values()
            .map(event_projection)
            .collect()
    }

    /// Persist command admission before a provider or process effect. Replay is
    /// resolved by the canonical operation ledger before current-state checks,
    /// while ambiguous prior effects fail closed as RecoveryRequired.
    pub fn prepare_runtime_command(
        &self,
        context: &MutationContext,
        command: &ControlCommandEnvelope,
        now_unix_ms: u64,
        now: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let command_fingerprint = runtime_command_envelope_fingerprint(command)?;
        if context.request_fingerprint.as_deref() != Some(command_fingerprint.as_str()) {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand full envelope fingerprint was not server-bound",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        // Resolve exact replay before mutable lease/session checks. This
        // returns the original durable result without repeating an effect;
        // changing any envelope field under the same key conflicts.
        if let Some(replay) =
            self.trust_operation_envelopes_unlocked()?
                .into_iter()
                .find(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                        && envelope.authenticated_actor_id == context.authenticated_actor.id
                        && envelope.command_name == context.command_name
                        && envelope.operation.event.idempotency_key == context.idempotency_key
                })
        {
            if replay.operation.event.canonical_request_fingerprint != command_fingerprint
                || replay.operation.event.aggregate_kind != "runtime_command"
                || replay.operation.event.aggregate_id != command.id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "RuntimeCommand idempotency key was reused with a different full envelope",
                    "runtime_command",
                    &command.id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            let latest = self
                .trust_operation_envelopes_unlocked()?
                .into_iter()
                .filter(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.operation.event.aggregate_kind == "runtime_command"
                        && envelope.operation.event.aggregate_id == command.id
                })
                .max_by_key(|envelope| envelope.operation.event.sequence)
                .unwrap_or(replay);
            return Ok(CanonicalMutationResult {
                projection: event_projection(&latest)?,
                event: latest.operation.event,
                replayed: true,
            });
        }
        self.validate_runtime_command(command, now_unix_ms)?;
        if command.execution_space_id != context.execution_space_id
            || command.authenticated_actor
                != context
                    .authority_actor
                    .clone()
                    .unwrap_or_else(|| context.authenticated_actor.clone())
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand authority or fingerprint was not server-bound",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        let expected_capability = match command.command {
            RuntimeCommandKind::AuthorMessage => "message.author",
            RuntimeCommandKind::StartSession => "agent_session.start",
            RuntimeCommandKind::StopSession => "agent_session.stop",
            RuntimeCommandKind::ResumeSession => "agent_session.resume",
            RuntimeCommandKind::DispatchProvider => "provider.dispatch",
            RuntimeCommandKind::CancelProviderTurn => "provider.cancel",
        };
        if command.required_capability != expected_capability {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand capability is not the server-owned capability for this command",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        let requested_start_session = if command.command == RuntimeCommandKind::StartSession
            && command.payload.get("session").is_some()
        {
            Some(
                serde_json::from_value::<AgentSession>(command.payload["session"].clone())
                    .map_err(|error| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            format!("StartSession payload is invalid: {error}"),
                            "runtime_command",
                            &command.id,
                            None,
                        )
                    })?,
            )
        } else {
            None
        };
        let target_session_id = command.payload["session_id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                requested_start_session
                    .as_ref()
                    .map(|session| session.id.clone())
            });
        let target_session_generation =
            command.payload["session_generation"].as_u64().or_else(|| {
                requested_start_session
                    .as_ref()
                    .map(|session| session.runtime_generation)
            });
        if command.command != RuntimeCommandKind::AuthorMessage {
            let session = if let Some(session) = requested_start_session.as_ref() {
                session.clone()
            } else {
                let session_id = target_session_id.as_deref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "RuntimeCommand requires an exact target AgentSession",
                        "runtime_command",
                        &command.id,
                        None,
                    )
                })?;
                self.fabric_agent_sessions(&context.execution_space_id)?
                    .into_iter()
                    .find(|session| session.id == session_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "RuntimeCommand target AgentSession does not exist",
                            "runtime_command",
                            &command.id,
                            None,
                        )
                    })?
            };
            if session.node_id != command.target_node_id
                || session.node_daemon_id != command.target_node_daemon_id
                || session.node_daemon_generation != command.target_node_daemon_generation
                || target_session_generation != Some(session.runtime_generation)
            {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "RuntimeCommand does not bind the exact current AgentSession and NodeDaemon generation",
                    "runtime_command",
                    &command.id,
                    Some(session.version),
                ));
            }
            let actor = &command.authenticated_actor;
            let exact_self =
                actor.kind == ActorKind::AgentMember && actor.id == session.agent_identity_id;
            let exact_operator = actor.kind == ActorKind::Service
                && (actor.id == session.node_id || actor.id == session.node_daemon_id);
            if !exact_self && !exact_operator {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "AgentSession RuntimeCommand requires exact self or exact machine NodeDaemon/Operator authority; Team Host authority is Team-scoped only",
                    "runtime_command",
                    &command.id,
                    None,
                ));
            }
            if let Some(requested) = requested_start_session.as_ref() {
                let identity = self
                    .fabric_agent_identities(&context.execution_space_id)?
                    .into_iter()
                    .find(|identity| identity.id == requested.agent_identity_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "StartSession target AgentIdentity does not exist",
                            "runtime_command",
                            &command.id,
                            None,
                        )
                    })?;
                if requested.effective_permission_ceiling > identity.permission_ceiling {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "StartSession cannot widen the frozen AgentIdentity permission ceiling",
                        "runtime_command",
                        &command.id,
                        None,
                    ));
                }
            }
            let active_bindings = self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .into_iter()
                .filter(|binding| {
                    binding.agent_session_id == session.id
                        && binding.agent_session_generation == session.runtime_generation
                        && binding.status == WorkExecutionBindingStatus::Active
                })
                .collect::<Vec<_>>();
            match command.command {
                RuntimeCommandKind::DispatchProvider => {
                    if session.lifecycle != AgentSessionStatus::Active {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "provider dispatch requires the exact active AgentSession",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::CancelProviderTurn => {
                    if session.lifecycle != AgentSessionStatus::Active
                        || session.current_turn_id.is_none()
                    {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "provider cancel requires an exact active provider turn",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::StopSession => {
                    if !matches!(
                        session.lifecycle,
                        AgentSessionStatus::Cold
                            | AgentSessionStatus::Active
                            | AgentSessionStatus::Idle
                            | AgentSessionStatus::Waiting
                            | AgentSessionStatus::Interrupted
                    ) {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "AgentSession stop cannot target a terminal session",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                    if !active_bindings.is_empty() {
                        return Err(trust_error(
                            TrustErrorCode::WorkExecutionBindingActive,
                            format!(
                                "AgentSession stop requires explicit release, rebind, or quiesce of active WorkExecutionBindings first: {}",
                                active_bindings
                                    .iter()
                                    .map(|binding| binding.id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                            "agent_session",
                            &session.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::StartSession | RuntimeCommandKind::ResumeSession => {
                    if matches!(
                        session.lifecycle,
                        AgentSessionStatus::Closed | AgentSessionStatus::RecoveryRequired
                    ) {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "provider process start/resume cannot target a terminal or recovery-required AgentSession",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::AuthorMessage => {}
            }
            let ambiguous = self
                .runtime_commands(&context.execution_space_id)?
                .into_iter()
                .any(|prior| {
                    prior.id != command.id
                        && prior.target_session_id.as_deref() == Some(session.id.as_str())
                        && matches!(
                            prior.status,
                            RuntimeCommandStatus::Accepted
                                | RuntimeCommandStatus::Quiesced
                                | RuntimeCommandStatus::RecoveryRequired
                        )
                        && prior.effect_certainty == RuntimeEffectCertainty::Unknown
                        && !matches!(
                            (command.command, prior.command),
                            (
                                RuntimeCommandKind::CancelProviderTurn
                                    | RuntimeCommandKind::StopSession,
                                RuntimeCommandKind::DispatchProvider
                            )
                        )
                });
            if ambiguous {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession has an ambiguous in-flight RuntimeCommand; reconciliation is required",
                    "runtime_command",
                    &command.id,
                    None,
                ));
            }
        }
        let record = RuntimeCommandRecord {
            id: command.id.clone(),
            execution_space_id: command.execution_space_id.clone(),
            target_node_id: command.target_node_id.clone(),
            target_node_daemon_id: command.target_node_daemon_id.clone(),
            target_node_daemon_generation: command.target_node_daemon_generation,
            authenticated_actor: command.authenticated_actor.clone(),
            command: command.command,
            required_capability: command.required_capability.clone(),
            idempotency_key: command.idempotency_key.clone(),
            request_fingerprint: command.payload_fingerprint.clone(),
            status: RuntimeCommandStatus::Accepted,
            effect_certainty: RuntimeEffectCertainty::Unknown,
            target_session_id,
            target_session_generation,
            source_record_id: command.payload["delivery_id"].as_str().map(str::to_string),
            result: None,
            failure_code: None,
            version: 1,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "runtime_command",
            &record.id,
            "accepted",
            serde_json::to_value(command)?,
            &record,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Resolve an Unknown provider effect without blindly repeating it. The
    /// exact current machine Operator asks the current NodeDaemon to record an
    /// evidence-backed certainty decision for one immutable command/session
    /// generation. Exact replay returns the original decision; changed
    /// semantics under the same key conflict before mutable-state checks.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve_runtime_command_recovery(
        &self,
        context: &MutationContext,
        command_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        resolution: RuntimeRecoveryResolution,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(evidence_ref, "RuntimeCommand recovery evidence_ref")?;
        let fingerprint = canonical_json_fingerprint(&serde_json::json!({
            "transport_request_fingerprint": context.request_fingerprint,
            "command_id": command_id,
            "node_id": node_id,
            "daemon_id": daemon_id,
            "daemon_generation": daemon_generation,
            "resolution": resolution,
            "evidence_ref": evidence_ref,
        }));
        let existing = self.trust_operation_envelopes_unlocked()?;
        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint
                || replay.operation.event.aggregate_kind != "runtime_command"
                || replay.operation.event.aggregate_id != command_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "RuntimeCommand recovery key was reused with different semantics",
                    "runtime_command",
                    command_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "runtime_command",
            command_id,
        )?;
        if context.authority_actor.as_ref()
            != Some(&ActorRef {
                kind: ActorKind::Service,
                id: node_id.to_string(),
            })
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand recovery requires the exact Execution Node Operator",
                "runtime_command",
                command_id,
                None,
            ));
        }
        let mut record = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "runtime_command")?
            .remove(command_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "RuntimeCommand recovery target does not exist",
                    "runtime_command",
                    command_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<RuntimeCommandRecord>(&envelope))?;
        if record.target_node_id != node_id || context.expected_version != record.version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "RuntimeCommand recovery requires the exact command Node and revision",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        if record.status != RuntimeCommandStatus::RecoveryRequired
            || record.effect_certainty != RuntimeEffectCertainty::Unknown
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an Unknown RecoveryRequired RuntimeCommand can be resolved",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        match resolution {
            RuntimeRecoveryResolution::ConfirmApplied => {
                record.status = RuntimeCommandStatus::Applied;
                record.effect_certainty = RuntimeEffectCertainty::Applied;
                record.failure_code = None;
            }
            RuntimeRecoveryResolution::ConfirmNotApplied => {
                record.status = RuntimeCommandStatus::Failed;
                record.effect_certainty = RuntimeEffectCertainty::NotApplied;
                record.failure_code = Some("RECOVERY_CONFIRMED_NOT_APPLIED".into());
            }
            RuntimeRecoveryResolution::KeepRecoveryRequired => {
                record.failure_code = Some("RECOVERY_EVIDENCE_INSUFFICIENT".into());
            }
        }
        record.result = Some(serde_json::json!({
            "resolution": resolution,
            "evidence_ref": evidence_ref,
            "blind_replay": false,
        }));
        record.version += 1;
        record.updated_at = updated_at.to_string();
        let aggregate_version = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "runtime_command"
                    && envelope.operation.event.aggregate_id == command_id
            })
            .map(|envelope| envelope.operation.event.resulting_version)
            .max()
            .unwrap_or(0);
        let mut commit_context = context.clone();
        commit_context.expected_version = aggregate_version;
        commit_context.request_fingerprint = Some(fingerprint);
        self.commit_trust_projection_unlocked(
            &commit_context,
            "runtime_command",
            command_id,
            "recovery_resolved",
            serde_json::json!({
                "resolution": resolution,
                "evidence_ref": evidence_ref,
                "daemon_generation": daemon_generation,
            }),
            &record,
            Vec::new(),
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn settle_runtime_command(
        &self,
        context: &MutationContext,
        command_id: &str,
        status: RuntimeCommandStatus,
        effect_certainty: RuntimeEffectCertainty,
        result: Option<Value>,
        failure_code: Option<String>,
        now: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut record = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "runtime_command")?
            .remove(command_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "RuntimeCommand was not durably accepted",
                    "runtime_command",
                    command_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<RuntimeCommandRecord>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &record.target_node_id,
            &record.target_node_daemon_id,
            record.target_node_daemon_generation,
            &context.authenticated_actor,
            "runtime_command",
            command_id,
        )?;
        if let (Some(session_id), Some(session_generation)) = (
            record.target_session_id.as_deref(),
            record.target_session_generation,
        ) {
            let session = self
                .fabric_agent_sessions(&context.execution_space_id)?
                .into_iter()
                .find(|session| session.id == session_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "RuntimeCommand target AgentSession disappeared before settlement",
                        "runtime_command",
                        command_id,
                        Some(record.version),
                    )
                })?;
            if session.runtime_generation != session_generation
                || session.node_id != record.target_node_id
                || session.node_daemon_id != record.target_node_daemon_id
                || session.node_daemon_generation != record.target_node_daemon_generation
            {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "RuntimeCommand settlement no longer owns the exact AgentSession/NodeDaemon generation",
                    "runtime_command",
                    command_id,
                    Some(record.version),
                ));
            }
        }
        if record.target_node_daemon_id != context.authenticated_actor.id
            || context.authenticated_actor.kind != ActorKind::Service
            || !matches!(
                record.status,
                RuntimeCommandStatus::Accepted
                    | RuntimeCommandStatus::Quiesced
                    | RuntimeCommandStatus::RecoveryRequired
            )
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "only the exact target NodeDaemon can settle an admitted RuntimeCommand",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        if !matches!(
            status,
            RuntimeCommandStatus::Applied
                | RuntimeCommandStatus::Failed
                | RuntimeCommandStatus::RecoveryRequired
                | RuntimeCommandStatus::Quiesced
        ) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "invalid RuntimeCommand settlement",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        record.status = status;
        record.effect_certainty = effect_certainty;
        record.result = result;
        record.failure_code = failure_code;
        record.version += 1;
        record.updated_at = now.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "runtime_command",
            command_id,
            "settled",
            serde_json::json!({
                "status": status,
                "effect_certainty": effect_certainty,
                "result": record.result,
                "failure_code": record.failure_code,
            }),
            &record,
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
    use std::io::Write;

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
            request_fingerprint: None,
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

    fn service_context(command: &str, key: &str, expected: u64) -> MutationContext {
        MutationContext {
            execution_space_id: "space-test".into(),
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: "daemon-1".into(),
            },
            authority_actor: None,
            command_name: command.into(),
            idempotency_key: key.into(),
            expected_version: expected,
            request_fingerprint: None,
        }
    }

    fn identity(id: &str) -> AgentIdentity {
        AgentIdentity {
            id: id.into(),
            display_name: id.into(),
            organization_status: AgentMemberOrganizationStatus::Active,
            permission_ceiling: PermissionCeiling::WorkspaceWrite,
            version: 1,
            created_at: "t1".into(),
            updated_at: "t1".into(),
        }
    }

    fn session(id: &str, identity_id: &str) -> AgentSession {
        AgentSession {
            id: id.into(),
            agent_identity_id: identity_id.into(),
            node_id: "11111111-1111-4111-8111-111111111111".into(),
            execution_space_id: "space-test".into(),
            node_daemon_id: "daemon-1".into(),
            node_daemon_generation: 1,
            provider_kind: "codex".into(),
            provider_profile_ref: "codex-default".into(),
            permission_envelope_ref: "permission-default".into(),
            effective_permission_ceiling: PermissionCeiling::WorkspaceWrite,
            lifecycle: AgentSessionStatus::Idle,
            runtime_generation: 1,
            native_session_ref: None,
            current_turn_id: None,
            queued_input_count: 0,
            version: 1,
            opened_at: "t1".into(),
            last_active_at: "t1".into(),
            closed_at: None,
        }
    }

    fn runtime_command_fixture(
        id: &str,
        kind: RuntimeCommandKind,
        session: &AgentSession,
        operation: &str,
    ) -> (ControlCommandEnvelope, MutationContext) {
        let payload = serde_json::json!({
            "session_id": session.id,
            "session_generation": session.runtime_generation,
            "operation": operation,
            "delivery_id": format!("delivery-{id}"),
        });
        let required_capability = match kind {
            RuntimeCommandKind::AuthorMessage => "message.author",
            RuntimeCommandKind::StartSession => "agent_session.start",
            RuntimeCommandKind::StopSession => "agent_session.stop",
            RuntimeCommandKind::ResumeSession => "agent_session.resume",
            RuntimeCommandKind::DispatchProvider => "provider.dispatch",
            RuntimeCommandKind::CancelProviderTurn => "provider.cancel",
        };
        let command = ControlCommandEnvelope {
            id: id.into(),
            execution_space_id: session.execution_space_id.clone(),
            target_node_id: session.node_id.clone(),
            target_node_daemon_id: session.node_daemon_id.clone(),
            target_node_daemon_generation: session.node_daemon_generation,
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: session.node_daemon_id.clone(),
            },
            command: kind,
            required_capability: required_capability.into(),
            idempotency_key: id.into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 60_000,
            payload_fingerprint: canonical_json_fingerprint(&payload),
            payload,
            issued_at: "t-command".into(),
        };
        let mut context = service_context("node_daemon.runtime.prepare", id, 0);
        context.authority_actor = Some(command.authenticated_actor.clone());
        context.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
        (command, context)
    }

    fn fabric_store() -> (HarnessStore, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "firm-runtime-fabric-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = HarnessStore::new(&root);
        store.init().unwrap();
        store
            .insert_execution_node(&firm_core::ExecutionNode {
                id: "11111111-1111-4111-8111-111111111111".into(),
                display_name: "local".into(),
                status: firm_core::ExecutionNodeStatus::Active,
                created_at: "t1".into(),
                updated_at: "t1".into(),
            })
            .unwrap();
        store
            .register_node_project(
                &firm_core::NodeProjectRegistration {
                    node_id: "11111111-1111-4111-8111-111111111111".into(),
                    execution_space_id: "space-test".into(),
                    project_binding_id: "project-1".into(),
                    status: firm_core::NodeProjectRegistrationStatus::Active,
                    created_at: "t1".into(),
                    updated_at: "t1".into(),
                },
                "space-test",
            )
            .unwrap();
        store
            .acquire_node_daemon_lease(
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                "instance-1",
                current_unix_ms(),
                60_000,
            )
            .unwrap();
        (store, root)
    }

    fn membership_fixture(id: &str, generation: u64) -> TeamMembership {
        TeamMembership {
            id: id.into(),
            team_id: "team-membership-test".into(),
            agent_identity_id: "membership-agent".into(),
            node_id: "11111111-1111-4111-8111-111111111111".into(),
            role: firm_core::agentfirm_api::TeamMembershipRole::Member,
            state: TeamMembershipStatus::Active,
            membership_generation: generation,
            default_subscription_refs: Vec::new(),
            created_by: actor("host"),
            revision: 1,
            joined_at: format!("t-join-{generation}"),
            left_at: None,
        }
    }

    fn append_runtime_team(store: &HarnessStore, team_id: &str, run_id: &str) {
        store
            .append_team_run(&firm_core::AgentTeamRun {
                id: run_id.into(),
                agent_team_id: team_id.into(),
                execution_node_id: "11111111-1111-4111-8111-111111111111".into(),
                project_binding_id: "project-1".into(),
                previous_run_id: None,
                host_surface: "test".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: firm_core::HostControlMode::External,
                objective: format!("runtime authority for {team_id}"),
                execution_root: None,
                status: firm_core::TeamRunStatus::Running,
                member_run_ids: Vec::new(),
                budget_limit_usd: None,
                created_at: "t1".into(),
                updated_at: "t1".into(),
                completed_at: None,
            })
            .unwrap();
    }

    fn join_runtime_membership(
        store: &HarnessStore,
        id: &str,
        team_id: &str,
        identity_id: &str,
        role: firm_core::agentfirm_api::TeamMembershipRole,
    ) -> TeamMembership {
        let membership = TeamMembership {
            id: id.into(),
            team_id: team_id.into(),
            agent_identity_id: identity_id.into(),
            node_id: "11111111-1111-4111-8111-111111111111".into(),
            role,
            state: TeamMembershipStatus::Active,
            membership_generation: 1,
            default_subscription_refs: Vec::new(),
            created_by: actor("fixture-host"),
            revision: 1,
            joined_at: "t-join".into(),
            left_at: None,
        };
        store
            .join_team_membership(
                &context("fixture-host", "membership.join", id, 0),
                membership.clone(),
            )
            .unwrap();
        membership
    }

    fn insert_runtime_work(
        store: &HarnessStore,
        id: &str,
        team_id: &str,
        team_run_id: &str,
    ) -> firm_core::Work {
        store
            .insert_work(
                firm_core::Work {
                    id: id.into(),
                    team_run_id: team_run_id.into(),
                    team_id: Some(team_id.into()),
                    parent_work_id: None,
                    title: format!("runtime binding {id}"),
                    context_markdown: "runtime authority test".into(),
                    completion_criteria_markdown: "binding is exact".into(),
                    phase: firm_core::WorkPhase::Open,
                    condition: firm_core::WorkCondition::Normal,
                    resolution: None,
                    owner_member_id: None,
                    active_member_run_id: None,
                    claim_mode: firm_core::WorkClaimMode::TeamClaim,
                    eligible_member_ids: Vec::new(),
                    prerequisite_work_ids: Vec::new(),
                    priority: firm_core::WorkPriority::Normal,
                    created_by_actor: firm_core::TeamActorRef {
                        kind: firm_core::TeamActorKind::Host,
                        id: "fixture-host".into(),
                        display_name: None,
                        authn_source: Some("test".into()),
                    },
                    created_by_member_id: None,
                    result_summary: None,
                    blocker_reason: None,
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    version: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                firm_core::WorkCommandContext {
                    event_id: format!("event-{id}"),
                    performed_by_actor: firm_core::TeamActorRef {
                        kind: firm_core::TeamActorKind::Host,
                        id: "fixture-host".into(),
                        display_name: None,
                        authn_source: Some("test".into()),
                    },
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("work-{id}"),
                    created_at: "t-work".into(),
                    duplicate_ok: false,
                },
            )
            .unwrap()
    }

    fn seed_membership_scope(store: &HarnessStore) {
        store
            .append_team_run(&firm_core::AgentTeamRun {
                id: "team-run-membership-test".into(),
                agent_team_id: "team-membership-test".into(),
                execution_node_id: "11111111-1111-4111-8111-111111111111".into(),
                project_binding_id: "project-1".into(),
                previous_run_id: None,
                host_surface: "test".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: firm_core::HostControlMode::External,
                objective: "membership cardinality".into(),
                execution_root: None,
                status: firm_core::TeamRunStatus::Running,
                member_run_ids: Vec::new(),
                budget_limit_usd: None,
                created_at: "t1".into(),
                updated_at: "t1".into(),
                completed_at: None,
            })
            .unwrap();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-membership-agent", 0),
                identity("membership-agent"),
            )
            .unwrap();
    }

    #[test]
    fn team_membership_is_single_active_generation_and_rejoin_is_exact_successor() {
        let (store, root) = fabric_store();
        seed_membership_scope(&store);
        let first = membership_fixture("membership-1", 1);
        store
            .join_team_membership(
                &context("host", "membership.join", "membership-1", 0),
                first.clone(),
            )
            .unwrap();

        let operations_before_duplicate = store.canonical_operations().unwrap();
        let subscriptions_before_duplicate =
            store.fabric_message_subscriptions("space-test").unwrap();
        let duplicate = store
            .join_team_membership(
                &context("host", "membership.join", "membership-2", 0),
                membership_fixture("membership-2", 2),
            )
            .expect_err("a second active generation must fail under the Store lock");
        assert!(duplicate.to_string().contains("already have an active"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_duplicate
        );
        assert_eq!(
            store.fabric_message_subscriptions("space-test").unwrap(),
            subscriptions_before_duplicate
        );

        let mut leave_context = context(
            "membership-agent",
            "membership.leave",
            "membership-1:leave",
            1,
        );
        leave_context.authenticated_actor.kind = ActorKind::AgentMember;
        store
            .leave_team_membership(&leave_context, &first.id, "t-leave")
            .unwrap();

        let wrong_generation = store
            .join_team_membership(
                &context("host", "membership.join", "membership-3", 0),
                membership_fixture("membership-3", 3),
            )
            .expect_err("rejoin cannot skip a membership generation");
        assert!(wrong_generation
            .to_string()
            .contains("exact successor generation 2"));
        store
            .join_team_membership(
                &context("host", "membership.join", "membership-2", 0),
                membership_fixture("membership-2", 2),
            )
            .unwrap();
        let active = store
            .fabric_team_memberships("space-test")
            .unwrap()
            .into_iter()
            .filter(|membership| membership.state == TeamMembershipStatus::Active)
            .collect::<Vec<_>>();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].membership_generation, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_team_membership_join_has_one_linearized_winner() {
        let (store, root) = fabric_store();
        seed_membership_scope(&store);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for suffix in ["a", "b"] {
            let root = root.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                let contender = HarnessStore::new(root);
                barrier.wait();
                contender.join_team_membership(
                    &context(
                        "host",
                        "membership.join",
                        &format!("membership-concurrent-{suffix}"),
                        0,
                    ),
                    membership_fixture(&format!("membership-concurrent-{suffix}"), 1),
                )
            }));
        }
        barrier.wait();
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().expect("membership contender"))
            .collect::<Vec<_>>();
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_err()).count(),
            1
        );
        let active = store
            .fabric_team_memberships("space-test")
            .unwrap()
            .into_iter()
            .filter(|membership| membership.state == TeamMembershipStatus::Active)
            .collect::<Vec<_>>();
        assert_eq!(active.len(), 1);
        assert_eq!(
            store
                .fabric_message_subscriptions("space-test")
                .unwrap()
                .len(),
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn node_daemon_authors_and_claims_identity_first_message() {
        let (store, root) = fabric_store();
        for id in ["sender", "recipient"] {
            store
                .create_agent_identity(
                    &context("host", "identity.create", &format!("identity-{id}"), 0),
                    identity(id),
                )
                .unwrap();
        }
        store
            .create_agent_session(
                &service_context("session.create", "sender-session", 0),
                session("session-sender", "sender"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "recipient-session", 0),
                session("session-recipient", "recipient"),
            )
            .unwrap();

        let subscription = MessageSubscription {
            id: "direct-recipient".into(),
            subscriber_agent_id: "recipient".into(),
            execution_space_id: "space-test".into(),
            source_kind: MessageSubscriptionKind::Agent,
            source_ref: "sender".into(),
            delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
            history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
            membership_ref: None,
            authorization_policy_ref: "direct.test".into(),
            policy_revision: 1,
            policy_digest: canonical_json_fingerprint(&serde_json::json!({"direct": true})),
            status: MessageSubscriptionStatus::Active,
            revision: 1,
            created_by: actor("host"),
            created_at: "t1".into(),
            revoked_at: None,
        };
        {
            let _lock = store.acquire_write_lock().unwrap();
            store
                .commit_trust_projection_unlocked(
                    &context("host", "subscription.create", "subscription", 0),
                    "message_subscription_set",
                    "recipient",
                    "created",
                    serde_json::to_value(&subscription).unwrap(),
                    &serde_json::json!({"recipient_identity_id": "recipient"}),
                    vec![serde_json::to_value(&subscription).unwrap()],
                    Vec::new(),
                )
                .unwrap();
        }
        let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::AgentIdentity,
            id: "recipient".into(),
        }];
        let body_digest = format!("sha256:{:x}", Sha256::digest(b"hello"));
        let fingerprint = canonical_json_fingerprint(&serde_json::json!({
            "sender_actor_ref": {"kind": "agent_member", "id": "sender"},
            "sender_agent_id": "sender",
            "sender_session_id": "session-sender",
            "address_kind": "direct_agent",
            "target_ref": {"kind": "agent_identity", "id": "recipient"},
            "recipients": recipients,
            "team_id": null,
            "team_run_id": null,
            "work_id": null,
            "collaboration_scope": null,
            "kind": firm_core::agentfirm_api::MessageKind::Message,
            "body": "hello",
            "body_digest": body_digest,
            "correlation_id": "corr-1",
            "causation_id": null,
            "response_intent": firm_core::agentfirm_api::ResponseIntent::Informational,
            "evidence_refs": Vec::<String>::new(),
            "schema_version": 1,
            "idempotency_key": "message-1",
        }));
        let authored = store
            .author_message(
                &service_context("message.author", "message-1", 0),
                Message {
                    id: "message-1".into(),
                    source_execution_space_id: "space-test".into(),
                    source_node_id: "11111111-1111-4111-8111-111111111111".into(),
                    source_node_daemon_id: "daemon-1".into(),
                    source_authority_generation: 1,
                    sender_actor_ref: ActorRef {
                        kind: ActorKind::AgentMember,
                        id: "sender".into(),
                    },
                    sender_agent_id: Some("sender".into()),
                    sender_session_id: Some("session-sender".into()),
                    address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
                    target_ref: firm_core::agentfirm_api::MessageRecipientRef {
                        kind: MessageRecipientKind::AgentIdentity,
                        id: "recipient".into(),
                    },
                    recipients,
                    team_id: None,
                    team_run_id: None,
                    work_id: None,
                    collaboration_scope: None,
                    kind: firm_core::agentfirm_api::MessageKind::Message,
                    body: "hello".into(),
                    body_digest,
                    correlation_id: "corr-1".into(),
                    causation_id: None,
                    response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
                    evidence_refs: Vec::new(),
                    content_fingerprint: fingerprint.clone(),
                    schema_version: 1,
                    idempotency_key: "message-1".into(),
                    created_at: "t2".into(),
                },
            )
            .unwrap();
        assert!(!authored.replayed);
        let delivery = store.fabric_message_deliveries("space-test").unwrap();
        assert_eq!(delivery.len(), 1);
        assert_eq!(delivery[0].recipient_session_id, None);

        let dispatch = store
            .claim_message_for_provider(
                &service_context("message.claim", "claim-1", 0),
                &delivery[0].id,
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
                "claim-1",
                firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
                "t3",
            )
            .unwrap();
        assert_eq!(dispatch.projection.recipient_identity_id, "recipient");
        assert_eq!(
            dispatch.projection.recipient_session_id,
            "session-recipient"
        );
        assert_eq!(dispatch.projection.content_fingerprint, fingerprint);

        let operations_before = store.canonical_operations().unwrap().len();
        let stale = store
            .claim_message_for_provider(
                &service_context("message.claim", "claim-stale", 0),
                &delivery[0].id,
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                0,
                "claim-stale",
                firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
                "t4",
            )
            .expect_err("stale daemon is fenced");
        assert!(stale.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
        assert_eq!(
            store.canonical_operations().unwrap().len(),
            operations_before
        );

        let mut reconcile_context =
            service_context("node_daemon.message_delivery.reconcile", "reconcile-1", 2);
        reconcile_context.request_fingerprint = Some(canonical_json_fingerprint(
            &serde_json::json!({"outcome":"retry_safe_failure","evidence_ref":"audit:no-provider-receipt"}),
        ));
        let reconciled = store
            .reconcile_canonical_message_delivery(
                &reconcile_context,
                &delivery[0].id,
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
                DeliveryReconcileOutcome::RetrySafeFailure,
                "audit:no-provider-receipt",
                "t5",
            )
            .unwrap();
        assert_eq!(
            reconciled.projection.status,
            CanonicalMessageDeliveryStatus::Queued
        );
        assert_eq!(reconciled.projection.attempt, 2);
        let replay = store
            .reconcile_canonical_message_delivery(
                &reconcile_context,
                &delivery[0].id,
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
                DeliveryReconcileOutcome::RetrySafeFailure,
                "audit:no-provider-receipt",
                "t5",
            )
            .unwrap();
        assert!(replay.replayed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_node_authors_cross_node_message_without_inventing_target_delivery() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-remote-sender", 0),
                identity("remote-sender"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "remote-sender-session", 0),
                session("session-remote-sender", "remote-sender"),
            )
            .unwrap();
        append_runtime_team(&store, "source-team", "source-team-run");
        join_runtime_membership(
            &store,
            "source-membership",
            "source-team",
            "remote-sender",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::AgentIdentity,
            id: "target-host-on-another-node".into(),
        }];
        let mut message = Message {
            id: "cross-node-message".into(),
            source_execution_space_id: "space-test".into(),
            source_node_id: "11111111-1111-4111-8111-111111111111".into(),
            source_node_daemon_id: "daemon-1".into(),
            source_authority_generation: 1,
            sender_actor_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "remote-sender".into(),
            },
            sender_agent_id: Some("remote-sender".into()),
            sender_session_id: Some("session-remote-sender".into()),
            address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
            target_ref: recipients[0].clone(),
            recipients,
            team_id: Some("source-team".into()),
            team_run_id: Some("source-team-run".into()),
            work_id: None,
            collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
                source_team_id: "source-team".into(),
                target_team_id: "target-team".into(),
                delegation_id: Some("delegation-a-b".into()),
                expected_delegation_revision: Some(3),
                source_work_ref: None,
                target_work_ref: None,
            }),
            kind: firm_core::agentfirm_api::MessageKind::Message,
            body: "cross-node immutable body".into(),
            body_digest: format!("sha256:{:x}", Sha256::digest(b"cross-node immutable body")),
            correlation_id: "cross-node-correlation".into(),
            causation_id: None,
            response_intent: firm_core::agentfirm_api::ResponseIntent::ResponseRequired,
            evidence_refs: Vec::new(),
            content_fingerprint: String::new(),
            schema_version: 1,
            idempotency_key: "cross-node-message".into(),
            created_at: "t2".into(),
        };
        message.content_fingerprint = message_content_fingerprint(&message);

        let authored = store
            .author_message(
                &service_context("message.author", "cross-node-message", 0),
                message.clone(),
            )
            .expect("source Node owns Message authorship without target delivery authority");
        assert_eq!(authored.projection, message);
        assert!(store
            .fabric_message_deliveries("space-test")
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remote_message_persists_before_delivery_and_replays_without_route_duplication() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "remote-recipient", 0),
                identity("remote-recipient"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "remote-recipient-session", 0),
                session("session-remote-recipient", "remote-recipient"),
            )
            .unwrap();
        append_runtime_team(&store, "target-team", "target-team-run");
        join_runtime_membership(
            &store,
            "target-membership",
            "target-team",
            "remote-recipient",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        let subscription = MessageSubscription {
            id: "remote-direct-recipient".into(),
            subscriber_agent_id: "remote-recipient".into(),
            execution_space_id: "space-test".into(),
            source_kind: MessageSubscriptionKind::Agent,
            source_ref: "remote-sender".into(),
            delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
            history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
            membership_ref: None,
            authorization_policy_ref: "direct.remote.test".into(),
            policy_revision: 1,
            policy_digest: canonical_json_fingerprint(&serde_json::json!({"direct": true})),
            status: MessageSubscriptionStatus::Active,
            revision: 1,
            created_by: actor("host"),
            created_at: "t1".into(),
            revoked_at: None,
        };
        {
            let _lock = store.acquire_write_lock().unwrap();
            store
                .commit_trust_projection_unlocked(
                    &context("host", "subscription.create", "remote-subscription", 0),
                    "message_subscription_set",
                    "remote-recipient",
                    "created",
                    serde_json::to_value(&subscription).unwrap(),
                    &serde_json::json!({"recipient_identity_id": "remote-recipient"}),
                    vec![serde_json::to_value(&subscription).unwrap()],
                    Vec::new(),
                )
                .unwrap();
        }

        let make_message = |body: &str| {
            let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::AgentIdentity,
                id: "remote-recipient".into(),
            }];
            let mut message = Message {
                id: "message-remote-1".into(),
                source_execution_space_id: "space-source".into(),
                source_node_id: "22222222-2222-4222-8222-222222222222".into(),
                source_node_daemon_id: "daemon-source".into(),
                source_authority_generation: 4,
                sender_actor_ref: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: "remote-sender".into(),
                },
                sender_agent_id: Some("remote-sender".into()),
                sender_session_id: Some("remote-sender-session".into()),
                address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
                target_ref: recipients[0].clone(),
                recipients,
                team_id: Some("source-team".into()),
                team_run_id: None,
                work_id: None,
                collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
                    source_team_id: "source-team".into(),
                    target_team_id: "target-team".into(),
                    delegation_id: Some("delegation-source-target".into()),
                    expected_delegation_revision: Some(3),
                    source_work_ref: None,
                    target_work_ref: None,
                }),
                kind: firm_core::agentfirm_api::MessageKind::Message,
                body: body.into(),
                body_digest: format!("sha256:{:x}", Sha256::digest(body.as_bytes())),
                correlation_id: "remote-correlation-1".into(),
                causation_id: None,
                response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
                evidence_refs: Vec::new(),
                content_fingerprint: String::new(),
                schema_version: 1,
                idempotency_key: "source-message-key-1".into(),
                created_at: "t2".into(),
            };
            message.content_fingerprint = message_content_fingerprint(&message);
            message
        };
        let make_operation = |message: &Message| {
            let body = serde_json::to_value(firm_fabric::MessageReference {
                message_id: message.id.clone(),
                body_digest: message.body_digest.clone(),
                canonical_message_envelope: Some(serde_json::to_value(message).unwrap()),
                message_object_ref: None,
            })
            .unwrap();
            firm_fabric::RoutedOperation {
                id: "remote-route-1".into(),
                company_id: "company-test".into(),
                kind: firm_fabric::MESSAGE_REFERENCE_KIND.into(),
                source_authority: firm_fabric::OperationSourceAuthority::Node,
                source_node_id: Some(message.source_node_id.clone()),
                target_node_id: "11111111-1111-4111-8111-111111111111".into(),
                source_gateway_generation: Some(4),
                source_node_daemon_id: Some(message.source_node_daemon_id.clone()),
                source_node_daemon_generation: Some(message.source_authority_generation),
                control_plane_generation: 2,
                source_execution_space_id: Some(message.source_execution_space_id.clone()),
                target_execution_space_id: Some("space-test".into()),
                actor: firm_fabric::AuthenticatedActor {
                    company_id: "company-test".into(),
                    actor_id: "remote-sender".into(),
                    actor_kind: firm_fabric::ActorKind::AgentMember,
                    role_bindings: BTreeSet::from(["fabric_submit".into()]),
                    session_id: "remote-sender-session".into(),
                    issued_at_unix_ms: 10,
                    expires_at_unix_ms: 90_000,
                },
                actor_runtime_generation: Some(3),
                authorization_context: BTreeMap::from([(
                    "capability".into(),
                    "remote-message".into(),
                )]),
                idempotency_key: "remote-route-1".into(),
                ordering_key: "message:remote-recipient".into(),
                correlation_id: message.correlation_id.clone(),
                causation_id: None,
                expected_target_revision: Some(0),
                body_schema: firm_fabric::MESSAGE_REFERENCE_SCHEMA.into(),
                body_digest: firm_fabric::json_digest(&body).unwrap(),
                body,
                priority: firm_fabric::OperationPriority::Normal,
                created_at_unix_ms: 20,
                expires_at_unix_ms: 90_000,
                protocol_version: firm_fabric::FABRIC_PROTOCOL_VERSION,
                schema_version: firm_fabric::FABRIC_SCHEMA_VERSION.into(),
                canonicalization_version: firm_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
            }
        };

        let message = make_message("remote hello");
        let operation = make_operation(&message);
        let mut persist_context = service_context("remote_message_persist", &operation.id, 0);
        persist_context.request_fingerprint = Some(firm_fabric::json_digest(&operation).unwrap());
        let before = store.canonical_operations().unwrap().len();
        let first = store
            .persist_remote_message(
                &persist_context,
                &operation,
                message.clone(),
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
            )
            .unwrap();
        assert!(!first.replayed);
        assert_eq!(store.canonical_operations().unwrap().len(), before + 1);
        let deliveries = store.fabric_message_deliveries("space-test").unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].message_id, message.id);
        assert_eq!(deliveries[0].recipient_identity_id, "remote-recipient");

        let replay = store
            .persist_remote_message(
                &persist_context,
                &operation,
                message,
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
            )
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(store.canonical_operations().unwrap().len(), before + 1);
        assert_eq!(
            store.fabric_message_deliveries("space-test").unwrap(),
            deliveries
        );

        let hostile_message = make_message("rewritten remote body");
        let hostile_operation = make_operation(&hostile_message);
        let mut hostile_context = persist_context;
        hostile_context.request_fingerprint =
            Some(firm_fabric::json_digest(&hostile_operation).unwrap());
        let hostile = store
            .persist_remote_message(
                &hostile_context,
                &hostile_operation,
                hostile_message,
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
            )
            .expect_err("same route id cannot rewrite an immutable Message");
        assert!(hostile.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
        assert_eq!(store.canonical_operations().unwrap().len(), before + 1);
        assert_eq!(
            store.fabric_message_deliveries("space-test").unwrap(),
            deliveries
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_command_replay_and_ambiguous_effect_fail_closed() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-runtime", 0),
                identity("runtime-agent"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "runtime-session", 0),
                session("session-runtime", "runtime-agent"),
            )
            .unwrap();
        let payload = serde_json::json!({
            "session_id": "session-runtime",
            "session_generation": 1,
        });
        let fingerprint = canonical_json_fingerprint(&payload);
        let command = ControlCommandEnvelope {
            id: "runtime-command-1".into(),
            execution_space_id: "space-test".into(),
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
            target_node_daemon_id: "daemon-1".into(),
            target_node_daemon_generation: 1,
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: "daemon-1".into(),
            },
            command: firm_core::agentfirm_api::RuntimeCommandKind::StopSession,
            required_capability: "agent_session.stop".into(),
            idempotency_key: "runtime-command-1".into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 60_000,
            payload,
            payload_fingerprint: fingerprint.clone(),
            issued_at: "t2".into(),
        };
        let command_fingerprint = runtime_command_envelope_fingerprint(&command).unwrap();
        let admission_context = MutationContext {
            execution_space_id: "space-test".into(),
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: "daemon-1".into(),
            },
            authority_actor: Some(ActorRef {
                kind: ActorKind::Service,
                id: "daemon-1".into(),
            }),
            command_name: "runtime.stop".into(),
            idempotency_key: "runtime-command-1".into(),
            expected_version: 0,
            request_fingerprint: Some(command_fingerprint),
        };
        let accepted = store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
            .unwrap();
        assert_eq!(accepted.projection.status, RuntimeCommandStatus::Accepted);
        assert_eq!(
            accepted.projection.effect_certainty,
            RuntimeEffectCertainty::Unknown
        );
        let replay = store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
            .unwrap();
        assert!(replay.replayed);

        let mut second = command.clone();
        second.id = "runtime-command-2".into();
        second.idempotency_key = "runtime-command-2".into();
        let before = store.canonical_operations().unwrap().len();
        let mut second_context = admission_context.clone();
        second_context.idempotency_key = "runtime-command-2".into();
        second_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&second).unwrap());
        let error = store
            .prepare_runtime_command(&second_context, &second, current_unix_ms(), "t3")
            .expect_err("ambiguous accepted command fences a successor");
        assert!(error.to_string().contains("reconciliation is required"));
        assert_eq!(store.canonical_operations().unwrap().len(), before);

        let settle_context = MutationContext {
            command_name: "runtime.stop.settle".into(),
            idempotency_key: "runtime-command-1:settle".into(),
            expected_version: 1,
            authority_actor: Some(actor("host")),
            ..service_context("unused", "unused", 0)
        };
        store
            .settle_runtime_command(
                &settle_context,
                "runtime-command-1",
                RuntimeCommandStatus::RecoveryRequired,
                RuntimeEffectCertainty::Unknown,
                None,
                Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
                "t4",
            )
            .unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_command_replay_precedes_successor_fence_but_stale_settlement_is_zero_effect() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-runtime-fence", 0),
                identity("runtime-fence"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "runtime-session-fence", 0),
                session("session-runtime-fence", "runtime-fence"),
            )
            .unwrap();
        store
            .transition_agent_session(
                &service_context("session.activate", "runtime-session-fence-active", 1),
                "session-runtime-fence",
                AgentSessionStatus::Active,
                "t2",
            )
            .unwrap();
        let payload = serde_json::json!({
            "session_id": "session-runtime-fence",
            "session_generation": 1,
            "delivery_id": "delivery-1",
        });
        let command = ControlCommandEnvelope {
            id: "runtime-command-fence".into(),
            execution_space_id: "space-test".into(),
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
            target_node_daemon_id: "daemon-1".into(),
            target_node_daemon_generation: 1,
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: "daemon-1".into(),
            },
            command: firm_core::agentfirm_api::RuntimeCommandKind::DispatchProvider,
            required_capability: "provider.dispatch".into(),
            idempotency_key: "runtime-command-fence".into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 120_000,
            payload_fingerprint: canonical_json_fingerprint(&payload),
            payload,
            issued_at: "t2".into(),
        };
        let mut admission_context = service_context(
            "runtime.provider_effect.prepare",
            "runtime-command-fence",
            0,
        );
        admission_context.authority_actor = Some(ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        });
        admission_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
            .unwrap();

        let successor_time = current_unix_ms() + 60_001;
        store
            .acquire_node_daemon_lease(
                "11111111-1111-4111-8111-111111111111",
                "daemon-2",
                "instance-2",
                successor_time,
                60_000,
            )
            .unwrap();

        let replay = store
            .prepare_runtime_command(&admission_context, &command, successor_time, "t3")
            .expect("exact replay is resolved before mutable successor state");
        assert!(replay.replayed);

        let operations_before = store.canonical_operations().unwrap();
        let settle_context = MutationContext {
            command_name: "runtime.provider_effect.settle".into(),
            idempotency_key: "runtime-command-fence:settle".into(),
            expected_version: 1,
            ..service_context("unused", "unused", 0)
        };
        let error = store
            .settle_runtime_command(
                &settle_context,
                "runtime-command-fence",
                RuntimeCommandStatus::Applied,
                RuntimeEffectCertainty::Applied,
                Some(serde_json::json!({"provider_receipt": "spoofed"})),
                None,
                "t4",
            )
            .expect_err("superseded daemon cannot settle an effect");
        assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), operations_before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_control_rejects_missing_turn_and_requires_explicit_binding_release_before_stop() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-runtime-control", 0),
                identity("runtime-control"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "runtime-control-session", 0),
                session("session-runtime-control", "runtime-control"),
            )
            .unwrap();

        let daemon = ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        };
        let cancel_payload = serde_json::json!({
            "session_id": "session-runtime-control",
            "session_generation": 1,
            "delivery_id": "control-cancel",
        });
        let cancel = ControlCommandEnvelope {
            id: "runtime-control-cancel".into(),
            execution_space_id: "space-test".into(),
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
            target_node_daemon_id: "daemon-1".into(),
            target_node_daemon_generation: 1,
            authenticated_actor: daemon.clone(),
            command: RuntimeCommandKind::CancelProviderTurn,
            required_capability: "provider.cancel".into(),
            idempotency_key: "runtime-control-cancel".into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 60_000,
            payload_fingerprint: canonical_json_fingerprint(&cancel_payload),
            payload: cancel_payload,
            issued_at: "t2".into(),
        };
        let mut cancel_context = service_context(
            "node_daemon.provider_effect.prepare",
            "runtime-control-cancel",
            0,
        );
        cancel_context.authority_actor = Some(daemon.clone());
        cancel_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&cancel).unwrap());
        let operations_before_cancel = store.canonical_operations().unwrap();
        let error = store
            .prepare_runtime_command(&cancel_context, &cancel, current_unix_ms(), "t2")
            .expect_err("an idle session has no provider turn to cancel");
        assert!(error.to_string().contains("exact active provider turn"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_cancel
        );

        let binding = WorkExecutionBinding {
            id: "binding-runtime-control".into(),
            work_id: "work-runtime-control".into(),
            work_revision: 1,
            team_id: "team-runtime-control".into(),
            team_membership_id: "membership-runtime-control".into(),
            agent_identity_id: "runtime-control".into(),
            agent_session_id: "session-runtime-control".into(),
            agent_session_generation: 1,
            delivery_id: "work-delivery-runtime-control".into(),
            binding_generation: 1,
            status: WorkExecutionBindingStatus::Active,
            version: 1,
            created_by: actor("host"),
            bound_at: "t2".into(),
            ended_at: None,
        };
        {
            let _lock = store.acquire_write_lock().unwrap();
            store
                .commit_trust_projection_unlocked(
                    &context("host", "binding.test_fixture", "binding-runtime-control", 0),
                    "work_execution_binding",
                    &binding.id,
                    "bound",
                    serde_json::to_value(&binding).unwrap(),
                    &binding,
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap();
        }
        let stop_payload = serde_json::json!({
            "session_id": "session-runtime-control",
            "session_generation": 1,
            "delivery_id": "control-stop",
        });
        let stop = ControlCommandEnvelope {
            id: "runtime-control-stop".into(),
            execution_space_id: "space-test".into(),
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
            target_node_daemon_id: "daemon-1".into(),
            target_node_daemon_generation: 1,
            authenticated_actor: daemon.clone(),
            command: RuntimeCommandKind::StopSession,
            required_capability: "agent_session.stop".into(),
            idempotency_key: "runtime-control-stop".into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 60_000,
            payload_fingerprint: canonical_json_fingerprint(&stop_payload),
            payload: stop_payload,
            issued_at: "t3".into(),
        };
        let mut stop_context = service_context(
            "node_daemon.provider_effect.prepare",
            "runtime-control-stop",
            0,
        );
        stop_context.authority_actor = Some(daemon);
        stop_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&stop).unwrap());
        let operations_before_stop = store.canonical_operations().unwrap();
        let stop_error = store
            .prepare_runtime_command(&stop_context, &stop, current_unix_ms(), "t3")
            .expect_err("StopSession cannot silently rewrite an active Work binding");
        assert!(stop_error
            .to_string()
            .contains("WORK_EXECUTION_BINDING_ACTIVE"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_stop
        );
        let active = store.fabric_work_execution_bindings("space-test").unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].status, WorkExecutionBindingStatus::Active);

        store
            .release_work_execution_binding(
                &context(
                    "runtime-control",
                    "work_binding.release",
                    "binding-runtime-control-release",
                    1,
                ),
                &binding.id,
                "t-release",
            )
            .expect("exact owner explicitly releases the binding");
        let stopped = store
            .prepare_runtime_command(&stop_context, &stop, current_unix_ms(), "t4")
            .expect("StopSession is admitted after explicit release");
        assert_eq!(stopped.projection.status, RuntimeCommandStatus::Accepted);
        let released = store.fabric_work_execution_bindings("space-test").unwrap();
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].status, WorkExecutionBindingStatus::Released);
        assert_eq!(released[0].ended_at.as_deref(), Some("t-release"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn admitted_stop_closes_exact_session_once_and_replays_after_terminal_state() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-runtime-stop", 0),
                identity("runtime-stop"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "runtime-stop-session", 0),
                session("session-runtime-stop", "runtime-stop"),
            )
            .unwrap();
        store
            .transition_agent_session(
                &service_context("session.activate", "runtime-stop-active", 1),
                "session-runtime-stop",
                AgentSessionStatus::Active,
                "t2",
            )
            .unwrap();
        let daemon = ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        };
        let payload = serde_json::json!({
            "session_id": "session-runtime-stop",
            "session_generation": 1,
            "delivery_id": "stop-control",
        });
        let command = ControlCommandEnvelope {
            id: "runtime-stop-command".into(),
            execution_space_id: "space-test".into(),
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
            target_node_daemon_id: "daemon-1".into(),
            target_node_daemon_generation: 1,
            authenticated_actor: daemon.clone(),
            command: RuntimeCommandKind::StopSession,
            required_capability: "agent_session.stop".into(),
            idempotency_key: "runtime-stop-once".into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 60_000,
            payload_fingerprint: canonical_json_fingerprint(&payload),
            payload,
            issued_at: "t2".into(),
        };
        let mut admission_context = service_context("runtime.stopsession", "runtime-stop-once", 0);
        admission_context.authority_actor = Some(daemon);
        admission_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let admitted = store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
            .unwrap();
        store
            .transition_agent_session(
                &service_context("runtime.stopsession.effect", "runtime-stop-once:effect", 2),
                "session-runtime-stop",
                AgentSessionStatus::Closed,
                "t3",
            )
            .unwrap();
        store
            .settle_runtime_command(
                &service_context(
                    "runtime.stopsession.settle",
                    "runtime-stop-once:settle",
                    admitted.projection.version,
                ),
                &command.id,
                RuntimeCommandStatus::Applied,
                RuntimeEffectCertainty::Applied,
                Some(serde_json::json!({"closed": true})),
                None,
                "t3",
            )
            .unwrap();
        let operations_before_replay = store.canonical_operations().unwrap();
        let replay = store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t4")
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.projection.status, RuntimeCommandStatus::Applied);
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_replay
        );
        assert_eq!(
            store.fabric_agent_sessions("space-test").unwrap()[0].lifecycle,
            AgentSessionStatus::Closed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_command_effect_matrix_is_exactly_replayable_and_fingerprint_closed() {
        let cases = [
            ("start", RuntimeCommandKind::StartSession, false),
            ("resume", RuntimeCommandKind::ResumeSession, false),
            ("turn", RuntimeCommandKind::DispatchProvider, true),
            ("input", RuntimeCommandKind::DispatchProvider, true),
            ("interrupt", RuntimeCommandKind::CancelProviderTurn, true),
            ("stop", RuntimeCommandKind::StopSession, false),
        ];
        for (operation, kind, needs_active_turn) in cases {
            let (store, root) = fabric_store();
            let identity_id = format!("runtime-{operation}");
            let session_id = format!("session-{operation}");
            store
                .create_agent_identity(
                    &context(
                        "host",
                        "identity.create",
                        &format!("identity-{operation}"),
                        0,
                    ),
                    identity(&identity_id),
                )
                .unwrap();
            store
                .create_agent_session(
                    &service_context("session.create", &format!("session-create-{operation}"), 0),
                    session(&session_id, &identity_id),
                )
                .unwrap();
            if needs_active_turn {
                store
                    .transition_agent_session(
                        &service_context(
                            "session.activate",
                            &format!("session-activate-{operation}"),
                            1,
                        ),
                        &session_id,
                        AgentSessionStatus::Active,
                        "t-active",
                    )
                    .unwrap();
            }
            let current_session = store
                .fabric_agent_sessions("space-test")
                .unwrap()
                .into_iter()
                .find(|candidate| candidate.id == session_id)
                .unwrap();
            let command_id = format!("runtime-{operation}");
            let (command, admission_context) =
                runtime_command_fixture(&command_id, kind, &current_session, operation);
            let accepted = store
                .prepare_runtime_command(
                    &admission_context,
                    &command,
                    current_unix_ms(),
                    "t-accepted",
                )
                .unwrap();
            assert_eq!(accepted.projection.status, RuntimeCommandStatus::Accepted);

            let operations_after_accept = store.canonical_operations().unwrap();
            let accepted_replay = store
                .prepare_runtime_command(
                    &admission_context,
                    &command,
                    current_unix_ms(),
                    "t-replay",
                )
                .unwrap();
            assert!(accepted_replay.replayed, "{operation} accepted replay");
            assert_eq!(
                store.canonical_operations().unwrap(),
                operations_after_accept
            );

            let mut drifted = command.clone();
            drifted.payload["operation"] = serde_json::json!(format!("{operation}-drift"));
            drifted.payload_fingerprint = canonical_json_fingerprint(&drifted.payload);
            let mut drifted_context = admission_context.clone();
            drifted_context.request_fingerprint =
                Some(runtime_command_envelope_fingerprint(&drifted).unwrap());
            let conflict = store
                .prepare_runtime_command(&drifted_context, &drifted, current_unix_ms(), "t-drift")
                .expect_err("changed full fingerprint must conflict");
            assert!(conflict.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
            assert_eq!(
                store.canonical_operations().unwrap(),
                operations_after_accept
            );

            let settled = store
                .settle_runtime_command(
                    &service_context(
                        "node_daemon.runtime.settle",
                        &format!("{command_id}:settle"),
                        accepted.projection.version,
                    ),
                    &command_id,
                    RuntimeCommandStatus::Applied,
                    RuntimeEffectCertainty::Applied,
                    Some(serde_json::json!({"operation": operation, "applied": true})),
                    None,
                    "t-applied",
                )
                .unwrap();
            assert_eq!(settled.projection.status, RuntimeCommandStatus::Applied);
            let operations_after_settle = store.canonical_operations().unwrap();
            let terminal_replay = store
                .prepare_runtime_command(
                    &admission_context,
                    &command,
                    current_unix_ms(),
                    "t-terminal-replay",
                )
                .unwrap();
            assert!(terminal_replay.replayed, "{operation} terminal replay");
            assert_eq!(
                terminal_replay.projection.status,
                RuntimeCommandStatus::Applied
            );
            assert_eq!(
                store.canonical_operations().unwrap(),
                operations_after_settle
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn runtime_command_failure_certainty_and_torn_rows_recover_without_duplicate_effect() {
        let outcomes = [
            (
                "socket-lost-before-effect",
                RuntimeCommandStatus::Failed,
                RuntimeEffectCertainty::NotApplied,
            ),
            (
                "socket-lost-after-effect",
                RuntimeCommandStatus::RecoveryRequired,
                RuntimeEffectCertainty::Unknown,
            ),
            (
                "provider-terminal-callback-race",
                RuntimeCommandStatus::Applied,
                RuntimeEffectCertainty::Applied,
            ),
        ];
        for (label, status, certainty) in outcomes {
            let (store, root) = fabric_store();
            store
                .create_agent_identity(
                    &context("host", "identity.create", &format!("identity-{label}"), 0),
                    identity(label),
                )
                .unwrap();
            let session_id = format!("session-{label}");
            store
                .create_agent_session(
                    &service_context("session.create", &format!("session-{label}"), 0),
                    session(&session_id, label),
                )
                .unwrap();
            let current = store
                .fabric_agent_sessions("space-test")
                .unwrap()
                .pop()
                .unwrap();
            let command_id = format!("runtime-{label}");
            let (command, admission_context) = runtime_command_fixture(
                &command_id,
                RuntimeCommandKind::StartSession,
                &current,
                label,
            );
            let admitted = store
                .prepare_runtime_command(
                    &admission_context,
                    &command,
                    current_unix_ms(),
                    "t-prepared",
                )
                .unwrap();
            let ledger = root.join("agentfirm_trust_operations.jsonl");
            let mut torn = fs::OpenOptions::new().append(true).open(&ledger).unwrap();
            torn.write_all(b"{\"torn_prepared\":").unwrap();
            torn.sync_all().unwrap();
            assert_eq!(store.runtime_commands("space-test").unwrap().len(), 1);

            store
                .settle_runtime_command(
                    &service_context(
                        "node_daemon.runtime.settle",
                        &format!("{command_id}:settle"),
                        admitted.projection.version,
                    ),
                    &command_id,
                    status,
                    certainty,
                    (certainty == RuntimeEffectCertainty::Applied)
                        .then(|| serde_json::json!({"effect": "observed"})),
                    (certainty != RuntimeEffectCertainty::Applied).then(|| label.to_string()),
                    "t-settled",
                )
                .unwrap();
            let mut torn = fs::OpenOptions::new().append(true).open(&ledger).unwrap();
            torn.write_all(b"{\"torn_completed\":").unwrap();
            torn.sync_all().unwrap();
            let recovered = store.runtime_commands("space-test").unwrap();
            assert_eq!(recovered.len(), 1);
            assert_eq!(recovered[0].status, status);
            assert_eq!(recovered[0].effect_certainty, certainty);
            let operations_before_replay = store.canonical_operations().unwrap();
            let replay = store
                .prepare_runtime_command(
                    &admission_context,
                    &command,
                    current_unix_ms(),
                    "t-replay",
                )
                .unwrap();
            assert!(replay.replayed);
            assert_eq!(replay.projection.status, status);
            assert_eq!(
                store.canonical_operations().unwrap(),
                operations_before_replay
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn runtime_recovery_resolution_is_operator_fenced_replay_safe_and_never_blind_replays() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-recovery-agent", 0),
                identity("recovery-agent"),
            )
            .unwrap();
        let target_session = session("session-recovery-agent", "recovery-agent");
        store
            .create_agent_session(
                &service_context("session.create", "session-recovery-agent", 0),
                target_session.clone(),
            )
            .unwrap();
        let (command, admission_context) = runtime_command_fixture(
            "runtime-recovery-command",
            RuntimeCommandKind::StopSession,
            &target_session,
            "stop_session",
        );
        store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t-prepare")
            .unwrap();
        let mut settle_context = service_context(
            "node_daemon.runtime.settle",
            "runtime-recovery-command:settle",
            1,
        );
        settle_context.authority_actor = Some(command.authenticated_actor.clone());
        store
            .settle_runtime_command(
                &settle_context,
                &command.id,
                RuntimeCommandStatus::RecoveryRequired,
                RuntimeEffectCertainty::Unknown,
                None,
                Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
                "t-ambiguous",
            )
            .unwrap();

        let operations_before_hostile = store.canonical_operations().unwrap();
        let mut hostile = service_context(
            "operator.runtime.resolve",
            "runtime-recovery-command:hostile",
            2,
        );
        hostile.authority_actor = Some(ActorRef {
            kind: ActorKind::Service,
            id: "sibling-node".into(),
        });
        let rejected = store
            .resolve_runtime_command_recovery(
                &hostile,
                &command.id,
                &target_session.node_id,
                &target_session.node_daemon_id,
                target_session.node_daemon_generation,
                RuntimeRecoveryResolution::ConfirmApplied,
                "evidence:hostile",
                "t-hostile",
            )
            .expect_err("a sibling Operator cannot resolve another Node's effect");
        assert!(rejected
            .to_string()
            .contains("exact Execution Node Operator"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_hostile
        );

        let mut resolve_context = service_context(
            "operator.runtime.resolve",
            "runtime-recovery-command:resolve",
            2,
        );
        resolve_context.authority_actor = Some(ActorRef {
            kind: ActorKind::Service,
            id: target_session.node_id.clone(),
        });
        let resolved = store
            .resolve_runtime_command_recovery(
                &resolve_context,
                &command.id,
                &target_session.node_id,
                &target_session.node_daemon_id,
                target_session.node_daemon_generation,
                RuntimeRecoveryResolution::ConfirmNotApplied,
                "evidence:provider-process-absent",
                "t-resolved",
            )
            .unwrap();
        assert_eq!(resolved.projection.status, RuntimeCommandStatus::Failed);
        assert_eq!(
            resolved.projection.effect_certainty,
            RuntimeEffectCertainty::NotApplied
        );
        assert_eq!(
            resolved.projection.result.as_ref().unwrap()["blind_replay"],
            false
        );
        let operations_after_resolution = store.canonical_operations().unwrap();
        let replay = store
            .resolve_runtime_command_recovery(
                &resolve_context,
                &command.id,
                &target_session.node_id,
                &target_session.node_daemon_id,
                target_session.node_daemon_generation,
                RuntimeRecoveryResolution::ConfirmNotApplied,
                "evidence:provider-process-absent",
                "t-replay",
            )
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_after_resolution
        );

        let conflict = store
            .resolve_runtime_command_recovery(
                &resolve_context,
                &command.id,
                &target_session.node_id,
                &target_session.node_daemon_id,
                target_session.node_daemon_generation,
                RuntimeRecoveryResolution::ConfirmApplied,
                "evidence:different-semantics",
                "t-conflict",
            )
            .expect_err("same key with changed resolution must conflict");
        assert!(conflict.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_after_resolution
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_command_hostile_member_and_permission_widening_have_zero_side_effects() {
        let (store, root) = fabric_store();
        for identity_id in ["runtime-owner", "runtime-sibling"] {
            store
                .create_agent_identity(
                    &context(
                        "host",
                        "identity.create",
                        &format!("identity-{identity_id}"),
                        0,
                    ),
                    identity(identity_id),
                )
                .unwrap();
        }
        let owner_session = session("session-runtime-owner", "runtime-owner");
        store
            .create_agent_session(
                &service_context("session.create", "session-runtime-owner", 0),
                owner_session.clone(),
            )
            .unwrap();

        let (mut hostile_command, mut hostile_context) = runtime_command_fixture(
            "runtime-hostile-sibling",
            RuntimeCommandKind::StopSession,
            &owner_session,
            "stop_session",
        );
        hostile_command.authenticated_actor = ActorRef {
            kind: ActorKind::AgentMember,
            id: "runtime-sibling".into(),
        };
        hostile_context.authenticated_actor = hostile_command.authenticated_actor.clone();
        hostile_context.authority_actor = Some(hostile_command.authenticated_actor.clone());
        hostile_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&hostile_command).unwrap());
        let operations_before_hostile = store.canonical_operations().unwrap();
        let sessions_before_hostile = store.fabric_agent_sessions("space-test").unwrap();
        let commands_before_hostile = store.runtime_commands("space-test").unwrap();
        let error = store
            .prepare_runtime_command(
                &hostile_context,
                &hostile_command,
                current_unix_ms(),
                "t-hostile",
            )
            .expect_err("an ordinary sibling Member cannot control this AgentSession");
        assert!(error.to_string().contains("exact self or exact machine"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_hostile
        );
        assert_eq!(
            store.fabric_agent_sessions("space-test").unwrap(),
            sessions_before_hostile
        );
        assert_eq!(
            store.runtime_commands("space-test").unwrap(),
            commands_before_hostile
        );

        let mut widened = session("session-runtime-widened", "runtime-owner");
        widened.effective_permission_ceiling = PermissionCeiling::FullAccess;
        let payload = serde_json::json!({"session": widened});
        let widening_command = ControlCommandEnvelope {
            id: "runtime-permission-widening".into(),
            execution_space_id: "space-test".into(),
            target_node_id: owner_session.node_id.clone(),
            target_node_daemon_id: owner_session.node_daemon_id.clone(),
            target_node_daemon_generation: owner_session.node_daemon_generation,
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: owner_session.node_daemon_id.clone(),
            },
            command: RuntimeCommandKind::StartSession,
            required_capability: "agent_session.start".into(),
            idempotency_key: "runtime-permission-widening".into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 60_000,
            payload_fingerprint: canonical_json_fingerprint(&payload),
            payload,
            issued_at: "t-widening".into(),
        };
        let mut widening_context = service_context(
            "node_daemon.runtime.prepare",
            "runtime-permission-widening",
            0,
        );
        widening_context.authority_actor = Some(widening_command.authenticated_actor.clone());
        widening_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&widening_command).unwrap());
        let operations_before_widening = store.canonical_operations().unwrap();
        let sessions_before_widening = store.fabric_agent_sessions("space-test").unwrap();
        let commands_before_widening = store.runtime_commands("space-test").unwrap();
        let error = store
            .prepare_runtime_command(
                &widening_context,
                &widening_command,
                current_unix_ms(),
                "t-widening",
            )
            .expect_err("StartSession cannot widen the AgentIdentity ceiling");
        assert!(error.to_string().contains("cannot widen"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_widening
        );
        assert_eq!(
            store.fabric_agent_sessions("space-test").unwrap(),
            sessions_before_widening
        );
        assert_eq!(
            store.runtime_commands("space-test").unwrap(),
            commands_before_widening
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn standalone_session_is_machine_owned_and_team_membership_is_only_an_overlay() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("operator", "identity.create", "standalone-identity", 0),
                identity("standalone-agent"),
            )
            .unwrap();
        assert!(store
            .fabric_team_memberships("space-test")
            .unwrap()
            .is_empty());

        let standalone = session("session-standalone", "standalone-agent");
        let payload = serde_json::json!({
            "session_id": standalone.id,
            "session_generation": standalone.runtime_generation,
            "session": standalone,
        });
        let command = ControlCommandEnvelope {
            id: "runtime-start-standalone".into(),
            execution_space_id: "space-test".into(),
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
            target_node_daemon_id: "daemon-1".into(),
            target_node_daemon_generation: 1,
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: "daemon-1".into(),
            },
            command: RuntimeCommandKind::StartSession,
            required_capability: "agent_session.start".into(),
            idempotency_key: "runtime-start-standalone".into(),
            expected_version: 0,
            expires_unix_ms: current_unix_ms() + 60_000,
            payload_fingerprint: canonical_json_fingerprint(&payload),
            payload,
            issued_at: "t-start".into(),
        };
        let mut start_context =
            service_context("node_daemon.runtime.prepare", "runtime-start-standalone", 0);
        start_context.authority_actor = Some(command.authenticated_actor.clone());
        start_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        store
            .prepare_runtime_command(&start_context, &command, current_unix_ms(), "t-start")
            .expect("standalone StartSession admission does not require TeamMembership");
        store
            .create_agent_session(
                &service_context("session.create", "session-standalone", 0),
                session("session-standalone", "standalone-agent"),
            )
            .unwrap();

        append_runtime_team(&store, "team-a", "team-run-a");
        append_runtime_team(&store, "team-b", "team-run-b");
        let membership_a = join_runtime_membership(
            &store,
            "membership-standalone-a",
            "team-a",
            "standalone-agent",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        join_runtime_membership(
            &store,
            "membership-standalone-b",
            "team-b",
            "standalone-agent",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        let sessions_before_leave = store.fabric_agent_sessions("space-test").unwrap();
        let mut leave_context = context(
            "standalone-agent",
            "membership.leave",
            "membership-standalone-a:leave",
            1,
        );
        leave_context.authenticated_actor.kind = ActorKind::AgentMember;
        store
            .leave_team_membership(&leave_context, &membership_a.id, "t-leave-a")
            .unwrap();
        assert_eq!(
            store.fabric_agent_sessions("space-test").unwrap(),
            sessions_before_leave,
            "joining or leaving Team overlays must not create, close, or rewrite the machine AgentSession"
        );
        assert!(store
            .fabric_team_memberships("space-test")
            .unwrap()
            .iter()
            .any(|membership| {
                membership.team_id == "team-b"
                    && membership.agent_identity_id == "standalone-agent"
                    && membership.state == TeamMembershipStatus::Active
            }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn team_host_cannot_stop_shared_session_and_active_bindings_require_explicit_release() {
        let (store, root) = fabric_store();
        for identity_id in ["shared-agent", "host-a", "host-b"] {
            store
                .create_agent_identity(
                    &context(
                        "operator",
                        "identity.create",
                        &format!("identity-{identity_id}"),
                        0,
                    ),
                    identity(identity_id),
                )
                .unwrap();
        }
        let shared_session = session("session-shared", "shared-agent");
        store
            .create_agent_session(
                &service_context("session.create", "session-shared", 0),
                shared_session.clone(),
            )
            .unwrap();
        append_runtime_team(&store, "team-a", "team-run-a");
        append_runtime_team(&store, "team-b", "team-run-b");
        let shared_a = join_runtime_membership(
            &store,
            "membership-shared-a",
            "team-a",
            "shared-agent",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        let shared_b = join_runtime_membership(
            &store,
            "membership-shared-b",
            "team-b",
            "shared-agent",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        join_runtime_membership(
            &store,
            "membership-host-a",
            "team-a",
            "host-a",
            firm_core::agentfirm_api::TeamMembershipRole::Host,
        );
        join_runtime_membership(
            &store,
            "membership-host-b",
            "team-b",
            "host-b",
            firm_core::agentfirm_api::TeamMembershipRole::Host,
        );
        let work_a = insert_runtime_work(&store, "work-a", "team-a", "team-run-a");
        let work_b = insert_runtime_work(&store, "work-b", "team-b", "team-run-b");
        for (id, work, membership) in [
            ("binding-a", &work_a, &shared_a),
            ("binding-b", &work_b, &shared_b),
        ] {
            store
                .bind_work_execution(
                    &context("fixture-host", "work.bind", id, 0),
                    WorkExecutionBinding {
                        id: id.into(),
                        work_id: work.id.clone(),
                        work_revision: work.version,
                        team_id: membership.team_id.clone(),
                        team_membership_id: membership.id.clone(),
                        agent_identity_id: "shared-agent".into(),
                        agent_session_id: shared_session.id.clone(),
                        agent_session_generation: shared_session.runtime_generation,
                        delivery_id: format!("delivery-{id}"),
                        binding_generation: 1,
                        status: WorkExecutionBindingStatus::Active,
                        version: 1,
                        created_by: actor("fixture-host"),
                        bound_at: "t-bound".into(),
                        ended_at: None,
                    },
                )
                .unwrap();
        }

        let (mut host_command, mut host_context) = runtime_command_fixture(
            "runtime-host-a-stop-shared",
            RuntimeCommandKind::StopSession,
            &shared_session,
            "stop_session",
        );
        host_command.authenticated_actor = ActorRef {
            kind: ActorKind::AgentMember,
            id: "host-a".into(),
        };
        host_context.authenticated_actor = host_command.authenticated_actor.clone();
        host_context.authority_actor = Some(host_command.authenticated_actor.clone());
        host_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&host_command).unwrap());
        let before_host = (
            store.canonical_operations().unwrap(),
            store.fabric_agent_sessions("space-test").unwrap(),
            store.fabric_work_execution_bindings("space-test").unwrap(),
            store.runtime_commands("space-test").unwrap(),
        );
        let host_error = store
            .prepare_runtime_command(&host_context, &host_command, current_unix_ms(), "t-host-a")
            .expect_err("Team A Host has no authority over the shared machine Session");
        assert!(host_error
            .to_string()
            .contains("Team Host authority is Team-scoped"));
        assert_eq!(
            (
                store.canonical_operations().unwrap(),
                store.fabric_agent_sessions("space-test").unwrap(),
                store.fabric_work_execution_bindings("space-test").unwrap(),
                store.runtime_commands("space-test").unwrap(),
            ),
            before_host,
            "cross-Team Host rejection must have zero canonical/session/binding/command side effects"
        );

        let (operator_command, operator_context) = runtime_command_fixture(
            "runtime-operator-stop-bound",
            RuntimeCommandKind::StopSession,
            &shared_session,
            "stop_session",
        );
        let before_bound_stop = (
            store.canonical_operations().unwrap(),
            store.fabric_agent_sessions("space-test").unwrap(),
            store.fabric_work_execution_bindings("space-test").unwrap(),
            store.runtime_commands("space-test").unwrap(),
        );
        let bound_error = store
            .prepare_runtime_command(
                &operator_context,
                &operator_command,
                current_unix_ms(),
                "t-bound-stop",
            )
            .expect_err("StopSession must not auto-release cross-Team Work bindings");
        assert!(bound_error
            .to_string()
            .contains("WORK_EXECUTION_BINDING_ACTIVE"));
        assert!(bound_error
            .to_string()
            .contains("explicit release, rebind, or quiesce"));
        assert_eq!(
            (
                store.canonical_operations().unwrap(),
                store.fabric_agent_sessions("space-test").unwrap(),
                store.fabric_work_execution_bindings("space-test").unwrap(),
                store.runtime_commands("space-test").unwrap(),
            ),
            before_bound_stop,
            "binding-fenced StopSession must have zero side effects"
        );

        for binding_id in ["binding-a", "binding-b"] {
            let mut release_context = context(
                "shared-agent",
                "work_binding.release",
                &format!("release-{binding_id}"),
                1,
            );
            release_context.authenticated_actor.kind = ActorKind::AgentMember;
            store
                .release_work_execution_binding(&release_context, binding_id, "t-release")
                .unwrap();
        }
        let accepted = store
            .prepare_runtime_command(
                &operator_context,
                &operator_command,
                current_unix_ms(),
                "t-stop-after-release",
            )
            .expect("explicit release makes the exact StopSession admissible");
        assert_eq!(accepted.projection.status, RuntimeCommandStatus::Accepted);
        assert!(store
            .fabric_work_execution_bindings("space-test")
            .unwrap()
            .iter()
            .all(|binding| binding.status == WorkExecutionBindingStatus::Released));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_session_rejects_every_provider_runtime_effect_with_zero_delta() {
        let (store, root) = fabric_store();
        store
            .create_agent_identity(
                &context("host", "identity.create", "identity-terminal", 0),
                identity("terminal"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "session-terminal", 0),
                session("session-terminal", "terminal"),
            )
            .unwrap();
        store
            .transition_agent_session(
                &service_context("session.close", "session-terminal-close", 1),
                "session-terminal",
                AgentSessionStatus::Closed,
                "t-closed",
            )
            .unwrap();
        let closed = store
            .fabric_agent_sessions("space-test")
            .unwrap()
            .pop()
            .unwrap();
        let operations_before = store.canonical_operations().unwrap();
        for (operation, kind) in [
            ("start", RuntimeCommandKind::StartSession),
            ("resume", RuntimeCommandKind::ResumeSession),
            ("turn", RuntimeCommandKind::DispatchProvider),
            ("input", RuntimeCommandKind::DispatchProvider),
            ("interrupt", RuntimeCommandKind::CancelProviderTurn),
            ("stop", RuntimeCommandKind::StopSession),
        ] {
            let (command, context) =
                runtime_command_fixture(&format!("terminal-{operation}"), kind, &closed, operation);
            store
                .prepare_runtime_command(&context, &command, current_unix_ms(), "t-rejected")
                .expect_err("terminal AgentSession must reject runtime effects");
            assert_eq!(store.canonical_operations().unwrap(), operations_before);
            assert!(store.runtime_commands("space-test").unwrap().is_empty());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
