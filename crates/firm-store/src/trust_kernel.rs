use crate::{
    ensure_member_lifecycle_revision, ensure_member_provenance_unchanged,
    ensure_provider_compatibility_cause_unchanged, latest_by_id, HarnessStore, StoreError,
    StoreResult,
};
use firm_core::agentfirm_api::{
    integration_plan_module_v1, ActorKind, ActorRef, AgentIdentity, AgentMember,
    AgentMemberOrganizationStatus, AgentSession, AgentSessionControlState, AgentSessionStatus,
    AgentTeamMigrationBundle, AgentTeamPurgeRequest, AgentTeamPurgeTombstone,
    CanonicalMessageDelivery, CanonicalMessageDeliveryStatus, CanonicalMutationEvent,
    CanonicalOperation, CanonicalWorkDelivery, ControlCommandEnvelope, DeliveryClaim,
    DeliveryReconcileOutcome, FailureAnalysis, GateEvaluation, GateRequirement,
    GateRequirementSource, GateVerdict, GateWaiver, GateWaiverState, MemberCoordinationStatus,
    MemberExecutionDriver, MemberRun, MemberRuntimeStatus, MemberWorkspaceBinding, Message,
    MessageRecipientKind, MessageSubjectKind, MessageSubscription, MessageSubscriptionKind,
    MessageSubscriptionStatus, MutationContext, NativeContinuationActivation,
    NativeContinuationPhase, NativeSessionRef, ProviderInvocation, ProviderReceipt,
    RuntimeActivity, RuntimeCommandKind, RuntimeCommandPhase, RuntimeCommandPrecondition,
    RuntimeCommandRecord, RuntimeCommandStatus, RuntimeDriverRef, RuntimeEffectCertainty,
    RuntimePostconditionStatus, RuntimeRecoveryResolution, RuntimeResidency,
    RuntimeSafePointRequirement, SubscriptionCursor, TeamMembership, TeamMembershipRole,
    TeamMembershipStatus, TeamMessageDeliveryClaim, TrustError, TrustErrorCode, WorkDelivery,
    WorkDeliveryStatus, WorkExecutionBinding, WorkExecutionBindingStatus, WorkFinding,
    WorkModuleBinding, WorkReport, WorkReportKind, WorkspaceLifecycle, WorkspaceMode,
    WorkspaceOwnership, WorkspaceSafetyProof,
};
use firm_core::collaboration::{
    CollaborationMessageAuthority, MessageAdmissionAuthority, PeerTeamMessageAdmissionAuthority,
};
use firm_core::{
    AgentTeam, AgentTeamStatus, ExecutionNodeStatus,
    MemberCoordinationStatus as LegacyMemberCoordinationStatus, MemberRunStatus,
    ProviderRuntimeProjection, TeamActorKind, TeamActorRef, Validate, Work, WorkCommandContext,
    WorkDelegationRevision,
};
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

/// The exact canonical half of one current MemberRun admission. The legacy
/// runtime projection and this canonical projection are validated together by
/// the Store before either side is mutated.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalMemberRunAdmission {
    pub context: MutationContext,
    pub run: MemberRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentTeamMemberLifecycleTransition {
    Close,
    Reopen,
    Retire,
    ResumeNativeSession,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CurrentTeamMemberLifecycleResult {
    pub runtime_projection: ProviderRuntimeProjection,
    pub canonical: CanonicalMutationResult<MemberRun>,
}

fn canonical_runtime_status(status: MemberRunStatus) -> MemberRuntimeStatus {
    match status {
        MemberRunStatus::Starting => MemberRuntimeStatus::Starting,
        MemberRunStatus::Idle => MemberRuntimeStatus::Idle,
        MemberRunStatus::Queued => MemberRuntimeStatus::Queued,
        MemberRunStatus::Running => MemberRuntimeStatus::Running,
        MemberRunStatus::Waiting => MemberRuntimeStatus::Waiting,
        MemberRunStatus::Disconnected => MemberRuntimeStatus::Disconnected,
        MemberRunStatus::Reviewing => MemberRuntimeStatus::Reviewing,
        MemberRunStatus::Blocked => MemberRuntimeStatus::Blocked,
        MemberRunStatus::Completed => MemberRuntimeStatus::Completed,
        MemberRunStatus::Failed => MemberRuntimeStatus::Failed,
        MemberRunStatus::Stopped => MemberRuntimeStatus::Stopped,
    }
}

fn canonical_coordination_status(
    status: LegacyMemberCoordinationStatus,
) -> MemberCoordinationStatus {
    match status {
        LegacyMemberCoordinationStatus::Active => MemberCoordinationStatus::Active,
        LegacyMemberCoordinationStatus::Closed => MemberCoordinationStatus::Closed,
        LegacyMemberCoordinationStatus::Retired => MemberCoordinationStatus::Retired,
    }
}

fn canonical_native_session(native: &firm_core::NativeSessionRef) -> StoreResult<NativeSessionRef> {
    serde_json::from_value(serde_json::to_value(native)?).map_err(StoreError::Json)
}

pub(crate) fn current_member_lifecycle_matches(
    canonical: &MemberRun,
    runtime: &ProviderRuntimeProjection,
) -> StoreResult<bool> {
    Ok(current_member_lifecycle_mismatch_fields(canonical, runtime)?.is_empty())
}

pub(crate) fn current_member_lifecycle_mismatch_fields(
    canonical: &MemberRun,
    runtime: &ProviderRuntimeProjection,
) -> StoreResult<Vec<&'static str>> {
    let canonical_native = runtime
        .native_session
        .as_ref()
        .map(canonical_native_session)
        .transpose()?;
    let mut mismatches = Vec::new();
    if canonical.team_run_id != runtime.team_run_id {
        mismatches.push("team_run_id");
    }
    if canonical.agent_member_id != runtime.agent_member_id {
        mismatches.push("agent_member_id");
    }
    if canonical.role_snapshot != runtime.role {
        mismatches.push("role");
    }
    if canonical.runtime_generation != runtime.runtime_generation {
        mismatches.push("runtime_generation");
    }
    if canonical.coordination_status != canonical_coordination_status(runtime.coordination_status) {
        mismatches.push("coordination_status");
    }
    if canonical.runtime_status != canonical_runtime_status(runtime.status) {
        mismatches.push("runtime_status");
    }
    if canonical.native_session != canonical_native {
        mismatches.push("native_session");
    }
    if canonical.started_at != runtime.started_at {
        mismatches.push("started_at");
    }
    if canonical.last_event_at != runtime.last_event_at {
        mismatches.push("last_event_at");
    }
    if canonical.finished_at != runtime.finished_at {
        mismatches.push("finished_at");
    }
    Ok(mismatches)
}

fn current_member_sync_payload(projection: &MemberRun) -> Value {
    serde_json::json!({
        "runtime_generation": projection.runtime_generation,
        "coordination_status": projection.coordination_status,
        "runtime_status": projection.runtime_status,
        "native_session": projection.native_session,
        "started_at": projection.started_at,
        "last_event_at": projection.last_event_at,
        "finished_at": projection.finished_at,
    })
}

#[derive(Debug)]
pub(crate) struct PreparedCurrentMemberSync {
    context: MutationContext,
    projection: MemberRun,
    transition: &'static str,
    side_records: Vec<Value>,
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

fn runtime_command_capability(kind: RuntimeCommandKind) -> &'static str {
    match kind {
        RuntimeCommandKind::AuthorMessage => "message.author",
        RuntimeCommandKind::StartSession => "agent_session.start",
        RuntimeCommandKind::StopSession => "agent_session.stop",
        RuntimeCommandKind::ResumeSession => "agent_session.resume",
        RuntimeCommandKind::DispatchProvider => "provider.dispatch",
        RuntimeCommandKind::CancelProviderTurn => "provider.cancel",
        RuntimeCommandKind::OpenRuntime => "runtime.open",
        RuntimeCommandKind::ResumeNativeSession => "runtime.native_session.resume",
        RuntimeCommandKind::ReleaseRuntime => "runtime.release",
        RuntimeCommandKind::CloseMember => "member.close",
        RuntimeCommandKind::ReopenMember => "member.reopen",
        RuntimeCommandKind::RetireMember => "member.retire",
        RuntimeCommandKind::DeleteNativeSession => "runtime.native_session.delete",
        RuntimeCommandKind::StartCycle => "cycle.start",
        RuntimeCommandKind::InjectCurrentCycle => "cycle.inject_current",
        RuntimeCommandKind::QueueAtNativeBoundary => "cycle.queue_native_boundary",
        RuntimeCommandKind::InterruptCurrentCycle => "cycle.interrupt_current",
        RuntimeCommandKind::CancelPendingInput => "cycle.pending_input.cancel",
        RuntimeCommandKind::InspectContinuation => "continuation.inspect",
        RuntimeCommandKind::ActivateContinuation => "continuation.activate",
        RuntimeCommandKind::InhibitContinuation => "continuation.inhibit",
        RuntimeCommandKind::ResumeContinuation => "continuation.resume",
        RuntimeCommandKind::ReplaceContinuationCondition => "continuation.condition.replace",
        RuntimeCommandKind::ClearContinuation => "continuation.clear",
        RuntimeCommandKind::QuiesceExecutionLane => "execution_lane.quiesce",
        RuntimeCommandKind::DrainRuntime => "runtime.drain",
        RuntimeCommandKind::StopBackgroundTask => "background_task.stop",
        RuntimeCommandKind::TransferExecutionDriver => "driver.transfer",
        RuntimeCommandKind::InspectCommandEffect => "command_effect.inspect",
        RuntimeCommandKind::ReconcileUnknownEffect => "command_effect.reconcile",
        RuntimeCommandKind::ReattachLiveRuntime => "runtime.reattach",
        RuntimeCommandKind::AbortIfNotApplied => "command_effect.abort_if_not_applied",
    }
}

fn runtime_command_requires_exact_binding(kind: RuntimeCommandKind) -> bool {
    !matches!(kind, RuntimeCommandKind::AuthorMessage)
}

fn runtime_binding_for_session(
    session: &AgentSession,
) -> firm_core::agentfirm_api::RuntimeCommandBinding {
    firm_core::agentfirm_api::RuntimeCommandBinding {
        target_session_id: Some(session.id.clone()),
        target_runtime_generation: Some(session.runtime_generation),
        target_driver_generation: Some(session.control_state.driver_generation),
        target_driver: session.control_state.driver_ref.clone(),
        native_session_ref: session.native_session_ref.clone(),
        composition_fingerprint: session.control_state.composition_fingerprint.clone(),
        capability_fingerprint: session.control_state.capability_fingerprint.clone(),
        permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
        ..Default::default()
    }
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

/// Digest the source-authoring policy half of a peer-Team Message authority.
/// This capability is intentionally independent from target delivery.
pub fn peer_team_source_policy_digest(authority: &PeerTeamMessageAdmissionAuthority) -> String {
    canonical_json_fingerprint(&serde_json::json!({
        "company_id": authority.company_id,
        "source_team_id": authority.source_team_id,
        "target_execution_space_id": authority.target_execution_space_id,
        "target_team_id": authority.target_team_id,
        "target_team_revision": authority.target_team_revision,
        "target_node_id": authority.target_node_id,
        "target_membership_id": authority.target_membership_id,
        "target_membership_generation": authority.target_membership_generation,
        "target_agent_member_id": authority.target_agent_member_id,
        "policy_ref": authority.source_policy_ref,
        "policy_revision": authority.source_policy_revision,
        "required_capability": authority.source_required_capability,
    }))
}

/// Digest the target durable subscription policy half of a peer-Team Message
/// authority. It grants no source authoring authority. A Team target freezes
/// the `team-inbox:` policy shape; a direct TeamMembership target freezes the
/// `direct:` subscription policy shape, matching what team/membership
/// admission originally stored.
pub fn peer_team_target_policy_digest(authority: &PeerTeamMessageAdmissionAuthority) -> String {
    if authority.target_membership_id.is_some() {
        // Byte-equal to the durable `direct:` subscription policy digest
        // written by membership admission; exact recipient binding is fenced
        // by the subscription id, membership_ref, and generation checks.
        return canonical_json_fingerprint(&serde_json::json!({
            "team_id": authority.target_team_id,
            "kind": "direct_from_active_team_members",
        }));
    }
    canonical_json_fingerprint(&serde_json::json!({
        "team_id": authority.target_team_id,
        "target_node_id": authority.target_node_id,
        "authorization_policy_ref": authority.target_authorization_policy_ref,
        "policy_revision": authority.target_policy_revision,
        "required_capability": authority.target_required_capability,
    }))
}

/// Digest every frozen source and target field of the peer-Team Message
/// admission authority. Revalidation still treats the two capabilities as
/// separate grants; this digest only prevents cross-wiring or widening.
pub fn peer_team_message_authority_digest(authority: &PeerTeamMessageAdmissionAuthority) -> String {
    canonical_json_fingerprint(&serde_json::json!({
        "company_id": authority.company_id,
        "source_execution_space_id": authority.source_execution_space_id,
        "source_team_id": authority.source_team_id,
        "source_team_revision": authority.source_team_revision,
        "source_membership_id": authority.source_membership_id,
        "source_membership_generation": authority.source_membership_generation,
        "source_agent_member_id": authority.source_agent_member_id,
        "source_session_id": authority.source_session_id,
        "source_session_generation": authority.source_session_generation,
        "source_node_id": authority.source_node_id,
        "source_node_daemon_id": authority.source_node_daemon_id,
        "source_node_daemon_generation": authority.source_node_daemon_generation,
        "target_execution_space_id": authority.target_execution_space_id,
        "target_team_id": authority.target_team_id,
        "target_team_revision": authority.target_team_revision,
        "target_node_id": authority.target_node_id,
        "target_membership_id": authority.target_membership_id,
        "target_membership_generation": authority.target_membership_generation,
        "target_agent_member_id": authority.target_agent_member_id,
        "source_policy_ref": authority.source_policy_ref,
        "source_policy_revision": authority.source_policy_revision,
        "source_policy_digest": authority.source_policy_digest,
        "source_required_capability": authority.source_required_capability,
        "target_subscription_id": authority.target_subscription_id,
        "target_subscription_revision": authority.target_subscription_revision,
        "target_authorization_policy_ref": authority.target_authorization_policy_ref,
        "target_policy_revision": authority.target_policy_revision,
        "target_policy_digest": authority.target_policy_digest,
        "target_required_capability": authority.target_required_capability,
    }))
}

fn membership_subscriptions(
    execution_space_id: &str,
    membership: &TeamMembership,
    status: MessageSubscriptionStatus,
    revision: u64,
    changed_at: &str,
) -> StoreResult<Vec<MessageSubscription>> {
    let revoked_at = (status == MessageSubscriptionStatus::Revoked).then(|| changed_at.to_string());
    let direct = MessageSubscription {
        id: format!("direct:{}:{}", membership.agent_member_id, membership.id),
        subscriber_kind: MessageSubjectKind::AgentMember,
        subscriber_ref: membership.agent_member_id.clone(),
        execution_space_id: execution_space_id.to_string(),
        target_team_id: Some(membership.team_id.clone()),
        target_node_id: membership.node_id.clone(),
        source_kind: MessageSubscriptionKind::Agent,
        source_ref: "active_team_members".into(),
        delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
        history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
        membership_ref: Some(membership.id.clone()),
        authorization_policy_ref: "team.direct.active-members".into(),
        policy_revision: 1,
        policy_digest: canonical_json_fingerprint(&serde_json::json!({
            "team_id": membership.team_id,
            "kind": "direct_from_active_team_members"
        })),
        status,
        revision,
        created_by: membership.created_by.clone(),
        created_at: membership.joined_at.clone(),
        revoked_at: revoked_at.clone(),
    };
    let team = MessageSubscription {
        id: format!("team:{}:{}", membership.team_id, membership.id),
        subscriber_kind: MessageSubjectKind::AgentMember,
        subscriber_ref: membership.agent_member_id.clone(),
        execution_space_id: execution_space_id.to_string(),
        target_team_id: Some(membership.team_id.clone()),
        target_node_id: membership.node_id.clone(),
        source_kind: MessageSubscriptionKind::Team,
        source_ref: membership.team_id.clone(),
        delivery_mode: if membership.role == TeamMembershipRole::Observer {
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly
        } else {
            firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle
        },
        history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
        membership_ref: Some(membership.id.clone()),
        authorization_policy_ref: "team.channel.membership".into(),
        policy_revision: 1,
        policy_digest: canonical_json_fingerprint(&serde_json::json!({
            "team_id": membership.team_id,
            "kind": "team_channel"
        })),
        status,
        revision,
        created_by: membership.created_by.clone(),
        created_at: membership.joined_at.clone(),
        revoked_at,
    };
    Ok(vec![direct, team])
}

fn team_inbox_subscription(
    execution_space_id: &str,
    team: &AgentTeam,
    status: MessageSubscriptionStatus,
    revision: u64,
    created_by: &ActorRef,
    updated_at: &str,
) -> MessageSubscription {
    let authorization_policy_ref = "collaboration.peer_message_deliver".to_string();
    let required_capability = "collaboration.peer_message_deliver";
    MessageSubscription {
        id: format!("team-inbox:{}", team.id),
        subscriber_kind: MessageSubjectKind::Team,
        subscriber_ref: team.id.clone(),
        execution_space_id: execution_space_id.to_string(),
        target_team_id: Some(team.id.clone()),
        target_node_id: team.node_id.clone(),
        source_kind: MessageSubscriptionKind::AllAuthorized,
        source_ref: "authorized_peer_teams".into(),
        delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
        history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
        membership_ref: None,
        authorization_policy_ref: authorization_policy_ref.clone(),
        policy_revision: 1,
        policy_digest: canonical_json_fingerprint(&serde_json::json!({
            "team_id": team.id,
            "target_node_id": team.node_id,
            "authorization_policy_ref": authorization_policy_ref,
            "policy_revision": 1,
            "required_capability": required_capability,
        })),
        status,
        revision,
        created_by: created_by.clone(),
        created_at: team.created_at.clone(),
        revoked_at: (status != MessageSubscriptionStatus::Active).then(|| updated_at.to_string()),
    }
}

/// Digest the immutable content fields of an authored Message. The source
/// NodeDaemon, target persistence, and read projections all recompute this
/// exact shape; it is the cross-process content identity of the Message.
pub fn message_content_fingerprint(message: &Message) -> String {
    canonical_json_fingerprint(&serde_json::json!({
        "sender_actor_ref": message.sender_actor_ref,
        "sender_agent_member_id": message.sender_agent_member_id,
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
        if work.accountable_team_id.as_deref() != Some(team_id) || work.version != work_revision {
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

    fn validate_trust_member_run_authority_unlocked(
        &self,
        context: &MutationContext,
        run: &MemberRun,
        team_run: &firm_core::AgentTeamRun,
    ) -> StoreResult<()> {
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
        if run.team_run_id != team_run.id {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "MemberRun does not belong to the admitted TeamRun",
                "member_run",
                &run.id,
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
                    "MemberRun references a missing AgentMember in the selected Execution Space",
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
                ));
            }
            AgentMemberOrganizationStatus::Retired => {
                return Err(trust_error(
                    TrustErrorCode::AgentMemberRetired,
                    "retired AgentMember cannot start a MemberRun",
                    "agent_member",
                    &member.id,
                    Some(member.version),
                ));
            }
        }
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
        let exact_membership = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team.id
                    && membership.agent_member_id == run.agent_member_id
                    && membership.state == TeamMembershipStatus::Active
            })
            .count();
        if team.status != AgentTeamStatus::Active || exact_membership != 1 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "MemberRun requires one exact active durable TeamMembership on an Active Team",
                "member_run",
                &run.id,
                None,
            ));
        }
        Ok(())
    }

    /// Validate every proposed canonical MemberRun against one frozen TeamRun
    /// and exact Execution Space while the caller holds the Store write lock.
    /// This is deliberately stricter than idempotent standalone create: current
    /// admission must materialize new, absent canonical rows.
    pub(crate) fn validate_new_trust_member_runs_unlocked(
        &self,
        execution_space_id: &str,
        team_run: &firm_core::AgentTeamRun,
        admissions: &[CanonicalMemberRunAdmission],
    ) -> StoreResult<()> {
        let existing = self.trust_operation_envelopes_unlocked()?;
        let mut proposed_ids = BTreeSet::new();
        let mut proposed_idempotency = BTreeSet::new();
        for admission in admissions {
            let context = &admission.context;
            let run = &admission.run;
            if context.execution_space_id != execution_space_id {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun admission changed Execution Space",
                    "member_run",
                    &run.id,
                    None,
                ));
            }
            self.validate_trust_member_run_authority_unlocked(context, run, team_run)?;
            if !proposed_ids.insert(run.id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::VersionConflict,
                    "MemberRun admission contains a duplicate id",
                    "member_run",
                    &run.id,
                    Some(0),
                ));
            }
            let idempotency_identity = (
                context.execution_space_id.clone(),
                context.authenticated_actor.kind,
                context.authenticated_actor.id.clone(),
                context.command_name.clone(),
                context.idempotency_key.clone(),
            );
            if !proposed_idempotency.insert(idempotency_identity.clone()) {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "MemberRun admission contains a duplicate idempotency key",
                    "member_run",
                    &run.id,
                    None,
                ));
            }
            if existing.iter().any(|envelope| {
                envelope.execution_space_id == execution_space_id
                    && envelope.operation.event.aggregate_kind == "member_run"
                    && envelope.operation.event.aggregate_id == run.id
            }) {
                return Err(trust_error(
                    TrustErrorCode::VersionConflict,
                    "MemberRun already exists in the selected Execution Space",
                    "member_run",
                    &run.id,
                    Some(1),
                ));
            }
            if existing.iter().any(|envelope| {
                envelope.execution_space_id == idempotency_identity.0
                    && envelope.authenticated_actor_kind == idempotency_identity.1
                    && envelope.authenticated_actor_id == idempotency_identity.2
                    && envelope.command_name == idempotency_identity.3
                    && envelope.operation.event.idempotency_key == idempotency_identity.4
            }) {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "MemberRun admission idempotency key already exists",
                    "member_run",
                    &run.id,
                    None,
                ));
            }
            // Prove the complete canonical payload is serializable before the
            // caller performs the first legacy-ledger append.
            serde_json::to_value(run)?;
        }
        Ok(())
    }

    /// Commit a previously validated set of new MemberRuns in one atomic
    /// replacement of the canonical trust ledger. The caller must retain the
    /// Store write lock from validation through this call.
    pub(crate) fn commit_new_trust_member_runs_unlocked(
        &self,
        admissions: &[CanonicalMemberRunAdmission],
    ) -> StoreResult<Vec<CanonicalMutationResult<MemberRun>>> {
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let first_store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let mut results = Vec::with_capacity(admissions.len());
        for (next_store_sequence, admission) in (first_store_sequence..).zip(admissions) {
            let context = &admission.context;
            let run = &admission.run;
            let payload = serde_json::to_value(run)?;
            let fingerprint = context
                .request_fingerprint
                .clone()
                .unwrap_or_else(|| canonical_json_fingerprint(&payload));
            let event = CanonicalMutationEvent {
                id: format!("trust-event-{next_store_sequence}"),
                aggregate_kind: "member_run".to_string(),
                aggregate_id: run.id.clone(),
                sequence: 1,
                store_sequence: next_store_sequence,
                transition: "created".to_string(),
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor: context.authenticated_actor.clone(),
                authority_actor: context.authority_actor.clone(),
                causation_ref: None,
                idempotency_key: context.idempotency_key.clone(),
                canonical_request_fingerprint: fingerprint,
                payload,
                created_at: now_string(),
            };
            let operation = CanonicalOperation {
                event: event.clone(),
                resulting_projection: serde_json::to_value(run)?,
                immutable_side_records: Vec::new(),
                initial_outbox_records: Vec::new(),
            };
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: context.command_name.clone(),
                operation,
            });
            results.push(CanonicalMutationResult {
                projection: run.clone(),
                event,
                replayed: false,
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(results)
    }

    pub(crate) fn prepare_current_member_runtime_sync_unlocked(
        &self,
        execution_space_id: &str,
        runtime: &ProviderRuntimeProjection,
    ) -> StoreResult<Option<PreparedCurrentMemberSync>> {
        self.prepare_current_member_runtime_sync_with_generation_unlocked(
            execution_space_id,
            runtime,
            false,
        )
    }

    fn prepare_current_member_runtime_sync_with_generation_unlocked(
        &self,
        execution_space_id: &str,
        runtime: &ProviderRuntimeProjection,
        allow_reopen_generation_advance: bool,
    ) -> StoreResult<Option<PreparedCurrentMemberSync>> {
        let envelope = self
            .latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .remove(&runtime.id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "MEMBER_RUN_MATERIALIZATION_INCOMPLETE: TeamRun {} declares MemberRun {} but no canonical MemberRun exists",
                    runtime.team_run_id, runtime.id
                ))
            })?;
        let current = event_projection::<MemberRun>(&envelope)?;
        let generation_matches = current.runtime_generation == runtime.runtime_generation
            || (allow_reopen_generation_advance
                && current.coordination_status == MemberCoordinationStatus::Closed
                && runtime.coordination_status == LegacyMemberCoordinationStatus::Active
                && runtime.runtime_generation == current.runtime_generation.saturating_add(1));
        if current.team_run_id != runtime.team_run_id
            || current.agent_member_id != runtime.agent_member_id
            || current.role_snapshot != runtime.role
            || !generation_matches
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} MemberRun {} cannot synchronize a mismatched canonical projection in Execution Space {}",
                runtime.team_run_id, runtime.id, execution_space_id
            )));
        }
        if current_member_lifecycle_matches(&current, runtime)? {
            return Ok(None);
        }
        let mut next = current.clone();
        next.coordination_status = canonical_coordination_status(runtime.coordination_status);
        next.runtime_status = canonical_runtime_status(runtime.status);
        next.runtime_generation = runtime.runtime_generation;
        next.native_session = runtime
            .native_session
            .as_ref()
            .map(canonical_native_session)
            .transpose()?;
        next.started_at = runtime.started_at.clone();
        next.last_event_at = runtime.last_event_at.clone();
        next.finished_at = runtime.finished_at.clone();
        next.version = current.version.saturating_add(1);
        serde_json::to_value(&next)?;
        Ok(Some(PreparedCurrentMemberSync {
            context: MutationContext {
                execution_space_id: execution_space_id.to_string(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: "node-daemon:member-projection-sync".to_string(),
                },
                authority_actor: Some(ActorRef {
                    kind: ActorKind::AgentMember,
                    id: runtime.agent_member_id.clone(),
                }),
                command_name: "team_run.member_projection.sync".to_string(),
                idempotency_key: format!("team-run-member-sync:{}:{}", runtime.id, next.version),
                expected_version: current.version,
                request_fingerprint: None,
            },
            projection: next,
            transition: "runtime_projection_synchronized",
            side_records: Vec::new(),
        }))
    }

    pub(crate) fn commit_prepared_current_member_sync_unlocked(
        &self,
        prepared: PreparedCurrentMemberSync,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.commit_trust_projection_unlocked(
            &prepared.context,
            "member_run",
            &prepared.projection.id,
            prepared.transition,
            current_member_sync_payload(&prepared.projection),
            &prepared.projection,
            prepared.side_records,
            Vec::new(),
        )
    }

    /// Explicit reconstruction seam for Legacy/import tests. Current Team
    /// Member admission must use the combined TeamRun admission APIs.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn legacy_import_create_trust_member_run_projection(
        &self,
        context: &MutationContext,
        run: MemberRun,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
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
        self.validate_trust_member_run_authority_unlocked(context, &run, &team_run)?;
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

    pub fn transition_current_team_member_lifecycle(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        transition: CurrentTeamMemberLifecycleTransition,
        updated_at: &str,
    ) -> StoreResult<CurrentTeamMemberLifecycleResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(member_run_id)
        .ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "MemberRun not found",
                "member_run",
                member_run_id,
                None,
            )
        })?;
        let team_run = latest_by_id(
            self.read_jsonl::<firm_core::AgentTeamRun>("team_runs.jsonl")?,
            |run| run.id.clone(),
        )
        .remove(&current.team_run_id)
        .ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "MemberRun references a missing TeamRun",
                "member_run",
                member_run_id,
                None,
            )
        })?;
        let execution_space_id = self.current_team_run_execution_space_unlocked(&team_run)?;
        if execution_space_id != context.execution_space_id {
            return Err(StoreError::Conflict(format!(
                "EXECUTION_SPACE_SCOPE_MISMATCH: TeamRun {} belongs to Execution Space {}, not {}",
                team_run.id, execution_space_id, context.execution_space_id
            )));
        }
        let canonical_current = self
            .latest_trust_envelopes_unlocked(&execution_space_id, "member_run")?
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
        if !current_member_lifecycle_matches(&canonical_current, &current)? {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} MemberRun {} lifecycle projections diverge",
                team_run.id, member_run_id
            )));
        }

        let already_at_requested_result = match transition {
            CurrentTeamMemberLifecycleTransition::Close => {
                current.coordination_status == LegacyMemberCoordinationStatus::Closed
                    && current.status == MemberRunStatus::Stopped
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.as_deref() == Some(updated_at)
            }
            CurrentTeamMemberLifecycleTransition::Retire => {
                current.coordination_status == LegacyMemberCoordinationStatus::Retired
                    && current.status == MemberRunStatus::Stopped
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.as_deref() == Some(updated_at)
            }
            CurrentTeamMemberLifecycleTransition::Reopen => {
                current.coordination_status == LegacyMemberCoordinationStatus::Active
                    && current.status == MemberRunStatus::Queued
                    && current.started_at == updated_at
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.is_none()
            }
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession => {
                current.coordination_status == LegacyMemberCoordinationStatus::Active
                    && current.status == MemberRunStatus::Starting
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.is_none()
            }
        };
        if already_at_requested_result {
            let payload = current_member_sync_payload(&canonical_current);
            let fingerprint = context
                .request_fingerprint
                .clone()
                .unwrap_or_else(|| canonical_json_fingerprint(&payload));
            if let Some(replay) = self.replay_trust_projection_unlocked(
                context,
                "member_run",
                member_run_id,
                &fingerprint,
            )? {
                return Ok(CurrentTeamMemberLifecycleResult {
                    runtime_projection: current,
                    canonical: replay,
                });
            }
        }

        let mut next = current.clone();
        let transition_name = match transition {
            CurrentTeamMemberLifecycleTransition::Close => {
                if !current.coordination_is_active() {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Close requires an active MemberRun",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                next.coordination_status = LegacyMemberCoordinationStatus::Closed;
                next.status = MemberRunStatus::Stopped;
                next.finished_at = Some(updated_at.to_string());
                "closed"
            }
            CurrentTeamMemberLifecycleTransition::Retire => {
                if current.coordination_is_retired() {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "MemberRun is already retired",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                next.coordination_status = LegacyMemberCoordinationStatus::Retired;
                next.status = MemberRunStatus::Stopped;
                next.finished_at = Some(updated_at.to_string());
                "retired"
            }
            CurrentTeamMemberLifecycleTransition::Reopen => {
                if current.coordination_status != LegacyMemberCoordinationStatus::Closed {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Reopen requires a closed MemberRun",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                let session = current.native_session.as_ref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::NativeSessionMissing,
                        "reopen requires a resumable NativeSessionRef",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    )
                })?;
                if !session.supports_resume
                    || matches!(
                        session.availability,
                        firm_core::NativeSessionAvailability::Missing
                            | firm_core::NativeSessionAvailability::Incompatible
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::NativeSessionIncompatible,
                        "NativeSessionRef is not safely resumable",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                next.runtime_generation = current.runtime_generation.saturating_add(1);
                next.coordination_status = LegacyMemberCoordinationStatus::Active;
                next.status = MemberRunStatus::Queued;
                next.started_at = updated_at.to_string();
                next.finished_at = None;
                "reopened"
            }
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession => {
                if current.coordination_status != LegacyMemberCoordinationStatus::Active
                    || !matches!(
                        current.status,
                        MemberRunStatus::Disconnected
                            | MemberRunStatus::Failed
                            | MemberRunStatus::Stopped
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Resume native session requires an active, disconnected, failed, or stopped MemberRun",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                let session = current.native_session.as_ref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::NativeSessionMissing,
                        "resume native session requires a resumable NativeSessionRef",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    )
                })?;
                if !session.supports_resume
                    || matches!(
                        session.availability,
                        firm_core::NativeSessionAvailability::Missing
                            | firm_core::NativeSessionAvailability::Incompatible
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::NativeSessionIncompatible,
                        "NativeSessionRef is not safely resumable",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                // Resuming a still-active MemberRun reattaches its exact
                // frozen provider-native session. It is not a coordination
                // reopen and therefore does not mint a new runtime
                // generation.
                next.status = MemberRunStatus::Starting;
                next.finished_at = None;
                "native_session_resume_requested"
            }
        };
        next.last_event_at = Some(updated_at.to_string());
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let allow_reopen_generation_advance =
            transition == CurrentTeamMemberLifecycleTransition::Reopen;
        let mut prepared = self
            .prepare_current_member_runtime_sync_with_generation_unlocked(
                &execution_space_id,
                &next,
                allow_reopen_generation_advance,
            )?
            .ok_or_else(|| {
                StoreError::Conflict("MemberRun lifecycle transition made no change".to_string())
            })?;
        prepared.context = context.clone();
        prepared.transition = transition_name;
        if matches!(
            transition,
            CurrentTeamMemberLifecycleTransition::Close
                | CurrentTeamMemberLifecycleTransition::Retire
        ) {
            for mut delivery in self.trust_work_deliveries(&execution_space_id)? {
                if delivery.recipient_member_run_id != member_run_id {
                    continue;
                }
                if transition == CurrentTeamMemberLifecycleTransition::Close
                    && delivery.status == WorkDeliveryStatus::Queued
                {
                    delivery.freeze_generation = Some(next.runtime_generation);
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    prepared.side_records.push(serde_json::to_value(delivery)?);
                } else if transition == CurrentTeamMemberLifecycleTransition::Retire
                    && matches!(
                        delivery.status,
                        WorkDeliveryStatus::Queued | WorkDeliveryStatus::Claimed
                    )
                {
                    delivery.status = WorkDeliveryStatus::Invalidated;
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    prepared.side_records.push(serde_json::to_value(delivery)?);
                }
            }
        }
        required(&context.execution_space_id, "execution_space_id")?;
        required(&context.authenticated_actor.id, "authenticated_actor.id")?;
        required(&context.command_name, "command_name")?;
        required(&context.idempotency_key, "idempotency_key")?;
        let payload = current_member_sync_payload(&prepared.projection);
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&payload));
        if self
            .replay_trust_projection_unlocked::<MemberRun>(
                context,
                "member_run",
                member_run_id,
                &fingerprint,
            )?
            .is_some()
        {
            return Err(StoreError::Conflict(
                "MEMBER_RUN_IDEMPOTENT_REPLAY_STATE_MISMATCH: prior lifecycle operation exists but current runtime projection does not match its result"
                    .to_string(),
            ));
        }
        if context.expected_version != canonical_current.version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                format!(
                    "expected version {}, current version is {}",
                    context.expected_version, canonical_current.version
                ),
                "member_run",
                member_run_id,
                Some(canonical_current.version),
            ));
        }
        serde_json::to_value(&next)?;
        self.append_jsonl_unlocked("member_runs.jsonl", &next)?;
        let canonical = self.commit_prepared_current_member_sync_unlocked(prepared)?;
        Ok(CurrentTeamMemberLifecycleResult {
            runtime_projection: next,
            canonical,
        })
    }

    /// Advance one current provider runtime generation while keeping the
    /// canonical MemberRun and its runtime projection coherent under the same
    /// Store write lock. Generic projection CAS deliberately cannot change a
    /// generation: reopen/recovery must use this combined authority boundary.
    ///
    /// The two physical ledgers are validated and serialized before the first
    /// write. They are not backed by a cross-file crash journal; a storage
    /// failure between the Legacy JSONL append and canonical atomic replace is
    /// therefore detected as an incomplete current TeamRun on restart and
    /// fails closed rather than being silently repaired.
    pub fn compare_and_advance_member_run_generation(
        &self,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the generation transition",
                expected.id
            )));
        }
        let execution_space_id = self.require_current_member_mutation_scope_unlocked(&current)?;
        ensure_member_provenance_unchanged(&current, next)?;
        ensure_member_lifecycle_revision(&current, next)?;
        ensure_provider_compatibility_cause_unchanged(&current, next)?;
        if next.runtime_generation != current.runtime_generation.saturating_add(1) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_GENERATION_TRANSITION_REQUIRED: ProviderRuntimeProjection {} must advance runtime_generation exactly once through combined Store authority",
                current.id
            )));
        }
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;

        let canonical_envelope = self
            .latest_trust_envelopes_unlocked(&execution_space_id, "member_run")?
            .remove(&current.id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "MEMBER_RUN_MATERIALIZATION_INCOMPLETE: TeamRun {} declares MemberRun {} but no canonical MemberRun exists",
                    current.team_run_id, current.id
                ))
            })?;
        let mut canonical = event_projection::<MemberRun>(&canonical_envelope)?;
        if canonical.team_run_id != current.team_run_id
            || canonical.agent_member_id != current.agent_member_id
            || canonical.role_snapshot != current.role
            || canonical.runtime_generation != current.runtime_generation
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} MemberRun {} cannot advance a mismatched canonical generation in Execution Space {}",
                current.team_run_id, current.id, execution_space_id
            )));
        }

        canonical.coordination_status = match next.coordination_status {
            LegacyMemberCoordinationStatus::Active => MemberCoordinationStatus::Active,
            LegacyMemberCoordinationStatus::Closed => MemberCoordinationStatus::Closed,
            LegacyMemberCoordinationStatus::Retired => MemberCoordinationStatus::Retired,
        };
        canonical.runtime_status = match next.status {
            MemberRunStatus::Starting => MemberRuntimeStatus::Starting,
            MemberRunStatus::Idle => MemberRuntimeStatus::Idle,
            MemberRunStatus::Queued => MemberRuntimeStatus::Queued,
            MemberRunStatus::Running => MemberRuntimeStatus::Running,
            MemberRunStatus::Waiting => MemberRuntimeStatus::Waiting,
            MemberRunStatus::Disconnected => MemberRuntimeStatus::Disconnected,
            MemberRunStatus::Reviewing => MemberRuntimeStatus::Reviewing,
            MemberRunStatus::Blocked => MemberRuntimeStatus::Blocked,
            MemberRunStatus::Completed => MemberRuntimeStatus::Completed,
            MemberRunStatus::Failed => MemberRuntimeStatus::Failed,
            MemberRunStatus::Stopped => MemberRuntimeStatus::Stopped,
        };
        canonical.runtime_generation = next.runtime_generation;
        canonical.native_session = next
            .native_session
            .as_ref()
            .map(canonical_native_session)
            .transpose()?;
        canonical.version = canonical.version.saturating_add(1);
        canonical.started_at = next.started_at.clone();
        canonical.last_event_at = next.last_event_at.clone();
        canonical.finished_at = next.finished_at.clone();

        let context = MutationContext {
            execution_space_id,
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: "node-daemon:member-generation".to_string(),
            },
            authority_actor: Some(ActorRef {
                kind: ActorKind::AgentMember,
                id: current.agent_member_id.clone(),
            }),
            command_name: "team_run.advance_member_generation".to_string(),
            idempotency_key: format!(
                "team-run-member-generation:{}:{}",
                current.id, next.runtime_generation
            ),
            expected_version: canonical.version.saturating_sub(1),
            request_fingerprint: None,
        };
        let payload = serde_json::json!({
            "member_run_id": current.id,
            "team_run_id": current.team_run_id,
            "runtime_generation": next.runtime_generation,
            "coordination_status": canonical.coordination_status,
            "runtime_status": canonical.runtime_status,
        });
        // Prove both rows serialize before the first durable mutation.
        serde_json::to_value(next)?;
        serde_json::to_value(&canonical)?;
        self.append_jsonl_unlocked("member_runs.jsonl", next)?;
        self.commit_trust_projection_unlocked(
            &context,
            "member_run",
            &canonical.id,
            "generation_advanced",
            payload,
            &canonical,
            Vec::new(),
            Vec::new(),
        )?;
        Ok(())
    }

    /// Write the settled provider-native Session binding onto a trust MemberRun.
    /// Fresh starts cannot know the provider thread id at MemberRun creation, so
    /// the binding lands later as its own CAS + generation-fenced mutation.
    /// Coordination status, runtime status, and runtime generation are untouched.
    /// The write is idempotent for the same native id (an identical rebind
    /// carries the same value) and rejects a conflicting rebind to another id.
    pub fn bind_member_run_native_session(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        expected_generation: u64,
        native_session: NativeSessionRef,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(
            &native_session.native_session_id,
            "NativeSessionRef.native_session_id",
        )?;
        required(&native_session.provider, "NativeSessionRef.provider")?;
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
        if run.coordination_status != MemberCoordinationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an active MemberRun can bind a provider-native Session",
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        if run.runtime_generation != expected_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                format!(
                    "MemberRun runtime generation is {}, the settled binding observed {expected_generation}",
                    run.runtime_generation
                ),
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        if let Some(current) = run.native_session.as_ref() {
            if current.native_session_id != native_session.native_session_id {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun already binds another provider-native Session",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
        }
        run.native_session = Some(native_session.clone());
        run.version += 1;
        run.last_event_at = Some(updated_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "member_run",
            member_run_id,
            "native_session_bound",
            serde_json::json!({
                "member_run_id": member_run_id,
                "runtime_generation": expected_generation,
                "native_session": native_session,
                "updated_at": updated_at,
            }),
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
        let host_agent_member_id = self
            .team_host_membership(&context.execution_space_id, &team.id, true)?
            .agent_member_id;
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        if message.sender.kind == ActorKind::AgentMember
            && message.sender.id != host_agent_member_id
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
                || work.accountable_team_id.as_deref() != Some(team.id.as_str())
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
                && (context.authenticated_actor.id == host_agent_member_id
                    || context.authority_actor.as_ref().is_some_and(|authority| {
                        authority.kind == ActorKind::AgentMember
                            && authority.id == host_agent_member_id
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
            if recipient.id == host_agent_member_id && matching.is_empty() {
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

    /// Prove that a provider-facing effect still targets the one live
    /// execution driver for this exact AgentSession generation.  This is a
    /// read performed while the caller holds the Store write lock; admission
    /// and the fence observation therefore cannot race another canonical
    /// control-state mutation.
    fn require_live_runtime_binding_unlocked(
        &self,
        session: &AgentSession,
        binding: &firm_core::agentfirm_api::RuntimeCommandBinding,
        allow_native_session_attachment: bool,
        resource_kind: &str,
        resource_id: &str,
        current_version: Option<u64>,
    ) -> StoreResult<()> {
        let composition_matches = session
            .control_state
            .composition_fingerprint
            .as_deref()
            .is_some_and(|current| {
                !current.trim().is_empty()
                    && binding.composition_fingerprint.as_deref() == Some(current)
            });
        let capability_matches = session
            .control_state
            .capability_fingerprint
            .as_deref()
            .is_some_and(|current| {
                !current.trim().is_empty()
                    && binding.capability_fingerprint.as_deref() == Some(current)
            });
        let native_session_matches = binding.native_session_ref == session.native_session_ref
            || (allow_native_session_attachment
                && binding.native_session_ref.is_none()
                && session.native_session_ref.as_ref().is_some_and(|native| {
                    native.provider == session.provider_kind
                        && !native.native_session_id.trim().is_empty()
                }));
        if binding.target_session_id.as_deref() != Some(session.id.as_str())
            || binding.target_runtime_generation != Some(session.runtime_generation)
            || session.control_state.driver_generation == 0
            || binding.target_driver_generation != Some(session.control_state.driver_generation)
            || binding.target_driver != session.control_state.driver_ref
            || !native_session_matches
            || binding.permission_envelope_ref.as_deref()
                != Some(session.permission_envelope_ref.as_str())
            || !composition_matches
            || !capability_matches
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider effect does not bind the exact current session/runtime/driver/native-session/composition/capability/permission state",
                resource_kind,
                resource_id,
                current_version,
            ));
        }

        // NodeDaemon is always the Runtime Supervisor, even when the current
        // next-cycle driver is a TeamSupervisor or provider continuation.
        self.require_current_node_daemon_unlocked(
            &session.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &ActorRef {
                kind: ActorKind::Service,
                id: session.node_daemon_id.clone(),
            },
            resource_kind,
            resource_id,
        )?;

        match (
            &session.control_state.execution_driver,
            &binding.target_driver,
        ) {
            (
                MemberExecutionDriver::HostDriven,
                RuntimeDriverRef::NodeDaemon {
                    node_daemon_id,
                    node_daemon_generation,
                },
            ) if node_daemon_id == &session.node_daemon_id
                && *node_daemon_generation == session.node_daemon_generation
                && !matches!(
                    session.control_state.continuation.activation,
                    NativeContinuationActivation::Armed { .. }
                ) => {}
            (
                MemberExecutionDriver::HostDriven,
                RuntimeDriverRef::TeamSupervisor {
                    team_run_id,
                    team_supervisor_id,
                    team_supervisor_generation,
                },
            ) => {
                let lease = self
                    .latest_team_supervisor_lease(team_run_id)?
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::SupervisorGenerationFenced,
                            "runtime driver TeamSupervisor lease is missing",
                            resource_kind,
                            resource_id,
                            current_version,
                        )
                    })?;
                let team_run = self
                    .team_runs()?
                    .into_iter()
                    .rev()
                    .find(|run| run.id == *team_run_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::SupervisorGenerationFenced,
                            "runtime driver TeamRun is missing",
                            resource_kind,
                            resource_id,
                            current_version,
                        )
                    })?;
                if lease.supervisor_id != *team_supervisor_id
                    || lease.generation != *team_supervisor_generation
                    || lease.status != firm_core::TeamSupervisorLeaseStatus::Active
                    || lease.expires_unix_ms <= current_unix_ms()
                    || lease.execution_space_id != session.execution_space_id
                    || lease.node_id != session.node_id
                    || lease.node_daemon_id != session.node_daemon_id
                    || lease.node_daemon_generation != session.node_daemon_generation
                    || team_run.execution_node_id != session.node_id
                    || team_run.project_binding_id != lease.project_binding_id
                    || matches!(
                        session.control_state.continuation.activation,
                        NativeContinuationActivation::Armed { .. }
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::SupervisorGenerationFenced,
                        "runtime effect used a stale, foreign, expired, or parent-fenced TeamSupervisor generation",
                        resource_kind,
                        resource_id,
                        current_version,
                    ));
                }
            }
            (
                MemberExecutionDriver::ProviderDriven,
                RuntimeDriverRef::ProviderContinuation {
                    provider,
                    continuation_id,
                    continuation_revision,
                    runtime_generation,
                },
            ) => {
                let continuation = &session.control_state.continuation;
                let activation_matches = matches!(
                    continuation.activation,
                    NativeContinuationActivation::Armed {
                        runtime_generation: armed_runtime_generation,
                        driver_generation: armed_driver_generation,
                    } if armed_runtime_generation == session.runtime_generation
                        && armed_driver_generation == session.control_state.driver_generation
                );
                if provider != &session.provider_kind
                    || *runtime_generation != session.runtime_generation
                    || continuation.definition.continuation_ref.as_deref()
                        != Some(continuation_id.as_str())
                    || continuation.definition.revision != *continuation_revision
                    || continuation.definition.phase != NativeContinuationPhase::Active
                    || !activation_matches
                {
                    return Err(trust_error(
                        TrustErrorCode::MemberRunGenerationFenced,
                        "provider continuation is not the exact active and armed continuation for this runtime/driver generation",
                        resource_kind,
                        resource_id,
                        current_version,
                    ));
                }
            }
            (MemberExecutionDriver::UserDriven, _) => {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "Harness cannot drive provider effects for a user-driven external runtime",
                    resource_kind,
                    resource_id,
                    current_version,
                ));
            }
            _ => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "runtime driver reference is unknown or incompatible with the declared execution driver",
                    resource_kind,
                    resource_id,
                    current_version,
                ));
            }
        }
        Ok(())
    }

    /// Evaluate the semantic predicate carried by a RuntimeCommand against the
    /// same AgentSession snapshot used for driver fencing.  A predicate is not
    /// documentation: if the Store cannot prove it from canonical control
    /// state, the provider effect is rejected before crossing the boundary.
    fn require_runtime_command_precondition_unlocked(
        session: &AgentSession,
        command: RuntimeCommandKind,
        precondition: &RuntimeCommandPrecondition,
        allow_command_poststate: bool,
        resource_kind: &str,
        resource_id: &str,
        current_version: Option<u64>,
    ) -> StoreResult<()> {
        let fenced = |message: &str| {
            trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                message,
                resource_kind,
                resource_id,
                current_version,
            )
        };

        let expected_version_advanced_by_this_command = allow_command_poststate
            && precondition
                .expected_session_version
                .is_some_and(|expected| {
                    session.version == expected.saturating_add(1)
                        && matches!(
                            (command, session.lifecycle),
                            (RuntimeCommandKind::StopSession, AgentSessionStatus::Closed)
                                | (RuntimeCommandKind::ResumeSession, AgentSessionStatus::Cold)
                        )
                });
        if precondition
            .expected_session_version
            .is_some_and(|expected| expected != session.version)
            && !expected_version_advanced_by_this_command
        {
            return Err(fenced(
                "RuntimeCommand expected_session_version no longer matches the canonical AgentSession",
            ));
        }
        if precondition
            .expected_residency
            .is_some_and(|expected| expected != session.control_state.runtime_residency)
        {
            return Err(fenced(
                "RuntimeCommand expected_residency no longer matches the canonical AgentSession",
            ));
        }
        if precondition
            .expected_activity
            .is_some_and(|expected| expected != session.control_state.activity)
        {
            return Err(fenced(
                "RuntimeCommand expected_activity no longer matches the canonical AgentSession",
            ));
        }
        if precondition
            .expected_execution_driver
            .is_some_and(|expected| expected != session.control_state.execution_driver)
        {
            return Err(fenced(
                "RuntimeCommand expected_execution_driver no longer matches the canonical AgentSession",
            ));
        }

        if let Some(expected) = precondition.expected_cycle_ref.as_ref() {
            if expected.revision.is_some() || expected.fingerprint.is_some() {
                return Err(fenced(
                    "RuntimeCommand cycle revision/fingerprint cannot be proven from canonical AgentSession control state",
                ));
            }
            if session.current_turn_id.as_deref() != Some(expected.id.as_str()) {
                return Err(fenced(
                    "RuntimeCommand expected_cycle_ref no longer matches the current provider cycle",
                ));
            }
        }

        if let Some(expected) = precondition.expected_continuation_ref.as_ref() {
            let definition = &session.control_state.continuation.definition;
            if expected.fingerprint.is_some()
                || definition.continuation_ref.as_deref() != Some(expected.id.as_str())
                || expected
                    .revision
                    .is_some_and(|revision| definition.revision != Some(revision))
            {
                return Err(fenced(
                    "RuntimeCommand expected_continuation_ref cannot be proven against the current continuation definition",
                ));
            }
        }
        if precondition
            .expected_continuation_phase
            .is_some_and(|expected| expected != session.control_state.continuation.definition.phase)
        {
            return Err(fenced(
                "RuntimeCommand expected_continuation_phase no longer matches the canonical continuation definition",
            ));
        }

        let safe_point_satisfied = match precondition.safe_point {
            // Unknown is the serde-default for legacy callers. It makes no
            // claim and therefore contributes no positive proof.
            RuntimeSafePointRequirement::Unknown | RuntimeSafePointRequirement::Immediate => true,
            RuntimeSafePointRequirement::CurrentCycle => {
                session.lifecycle == AgentSessionStatus::Active
                    && session.current_turn_id.is_some()
                    && matches!(
                        session.control_state.activity,
                        RuntimeActivity::Running
                            | RuntimeActivity::WaitingInput
                            | RuntimeActivity::Interrupting
                    )
            }
            RuntimeSafePointRequirement::CycleBoundary => {
                session.control_state.activity == RuntimeActivity::Idle
                    && !matches!(
                        session.lifecycle,
                        AgentSessionStatus::Closed | AgentSessionStatus::RecoveryRequired
                    )
            }
            RuntimeSafePointRequirement::RuntimeIdle => {
                session.control_state.runtime_residency == RuntimeResidency::Attached
                    && session.control_state.activity == RuntimeActivity::Idle
                    && !matches!(
                        session.lifecycle,
                        AgentSessionStatus::Closed | AgentSessionStatus::RecoveryRequired
                    )
            }
            // AgentSession intentionally does not mirror provider child/job
            // or durable-flush state. Only a verified adapter receipt can
            // prove full execution-lane quiescence.
            RuntimeSafePointRequirement::ExecutionLaneQuiesced => false,
        };
        if !safe_point_satisfied {
            return Err(fenced(
                "RuntimeCommand safe_point is not proven by the current canonical AgentSession state",
            ));
        }

        // One-driver authority is independent from a syntactically exact
        // RuntimeDriverRef. A provider continuation may be the live driver,
        // but that never authorizes Harness to start a second top-level cycle.
        if matches!(
            command,
            RuntimeCommandKind::DispatchProvider | RuntimeCommandKind::StartCycle
        ) && session.control_state.execution_driver != MemberExecutionDriver::HostDriven
        {
            return Err(fenced(
                "Harness cannot start a provider cycle while the AgentSession is provider-driven or user-driven",
            ));
        }

        Ok(())
    }

    fn hydrate_agent_team_compatibility_projection(
        &self,
        execution_space_id: &str,
        mut team: AgentTeam,
    ) -> StoreResult<AgentTeam> {
        let memberships = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .filter(|membership| membership.team_id == team.id)
            .collect::<Vec<_>>();
        let hosts = memberships
            .iter()
            .filter(|membership| membership.role == TeamMembershipRole::Host)
            .collect::<Vec<_>>();
        if hosts.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentTeam compatibility read failed closed because Host Membership authority is ambiguous",
                "agent_team",
                &team.id,
                Some(team.revision),
            ));
        }
        team.mission_id = team.legacy_mission_id.clone().unwrap_or_default();
        team.host_agent_id = hosts[0].agent_member_id.clone();
        team.member_ids = memberships
            .into_iter()
            .filter(|membership| {
                membership.role != TeamMembershipRole::Host
                    && membership.state == TeamMembershipStatus::Active
            })
            .map(|membership| membership.agent_member_id)
            .collect();
        Ok(team)
    }

    /// Durable AgentTeams are canonical trust aggregates. Mission linkage is
    /// optional migration provenance and never participates in identity or
    /// creation authority.
    pub fn agent_teams(&self, execution_space_id: &str) -> StoreResult<Vec<AgentTeam>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_team")?
            .values()
            .map(|envelope| {
                event_projection::<AgentTeam>(envelope).and_then(|team| {
                    self.hydrate_agent_team_compatibility_projection(execution_space_id, team)
                })
            })
            .collect()
    }

    /// Scope-preserving Company/read projection. Duplicate ids across spaces
    /// are retained as distinct rows and must never be used as mutation input.
    pub fn all_agent_teams(&self) -> StoreResult<Vec<AgentTeam>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "agent_team" {
                latest.insert(
                    (
                        envelope.execution_space_id.clone(),
                        envelope.operation.event.aggregate_id.clone(),
                    ),
                    envelope,
                );
            }
        }
        latest
            .into_iter()
            .map(|((execution_space_id, _), envelope)| {
                event_projection::<AgentTeam>(&envelope).and_then(|team| {
                    self.hydrate_agent_team_compatibility_projection(&execution_space_id, team)
                })
            })
            .collect()
    }

    pub fn agent_team_scope(&self, team_id: &str) -> StoreResult<Option<String>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .rev()
            .find(|envelope| {
                envelope.operation.event.aggregate_kind == "agent_team"
                    && envelope.operation.event.aggregate_id == team_id
            })
            .map(|envelope| envelope.execution_space_id))
    }

    pub fn create_agent_team(
        &self,
        context: &MutationContext,
        team: AgentTeam,
        memberships: Vec<TeamMembership>,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        team.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let request_payload = serde_json::json!({"team": team, "memberships": memberships});
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team",
            &team.id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        if team.revision != 1
            || team.status != AgentTeamStatus::Active
            || context.expected_version != 0
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new AgentTeam must be Active at revision 1 with absent CAS",
                "agent_team",
                &team.id,
                Some(0),
            ));
        }
        let node = self
            .latest_execution_nodes()?
            .into_iter()
            .find(|node| node.id == team.node_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam requires its immutable placement Node to exist",
                    "agent_team",
                    &team.id,
                    None,
                )
            })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentTeam requires an Active placement Node",
                "agent_team",
                &team.id,
                None,
            ));
        }
        let members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        let mut membership_ids = BTreeSet::new();
        let mut member_ids = BTreeSet::new();
        let mut active_hosts = 0usize;
        for membership in &memberships {
            required(&membership.id, "TeamMembership.id")?;
            required(
                &membership.agent_member_id,
                "TeamMembership.agent_member_id",
            )?;
            if !membership_ids.insert(membership.id.clone())
                || !member_ids.insert(membership.agent_member_id.clone())
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam creation contains a duplicate Membership or AgentMember",
                    "agent_team",
                    &team.id,
                    None,
                ));
            }
            if membership.team_id != team.id
                || membership.node_id != team.node_id
                || membership.state != TeamMembershipStatus::Active
                || membership.membership_generation != 1
                || membership.revision != 1
                || membership.left_at.is_some()
                || membership.created_by != context.authenticated_actor
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "initial TeamMembership must be active generation/revision 1 on the Team Node and created by the authenticated actor",
                    "team_membership",
                    &membership.id,
                    None,
                ));
            }
            let member = members.get(&membership.agent_member_id).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing AgentMember",
                    "team_membership",
                    &membership.id,
                    None,
                )
            })?;
            if member.organization_status != AgentMemberOrganizationStatus::Active {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "initial TeamMembership requires an Active AgentMember",
                    "team_membership",
                    &membership.id,
                    Some(member.version),
                ));
            }
            active_hosts += usize::from(membership.role == TeamMembershipRole::Host);
        }
        if active_hosts != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "an Active AgentTeam requires exactly one active Host TeamMembership",
                "agent_team",
                &team.id,
                None,
            ));
        }
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        if committed.iter().any(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && ((envelope.operation.event.aggregate_kind == "agent_team"
                    && envelope.operation.event.aggregate_id == team.id)
                    || (envelope.operation.event.aggregate_kind == "team_membership"
                        && membership_ids.contains(&envelope.operation.event.aggregate_id)))
        }) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentTeam or one of its initial TeamMembership ids already exists",
                "agent_team",
                &team.id,
                Some(0),
            ));
        }
        let mut store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let team_event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "agent_team".into(),
            aggregate_id: team.id.clone(),
            sequence: 1,
            store_sequence,
            transition: "created".into(),
            expected_version: 0,
            resulting_version: team.revision,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: request_fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation: CanonicalOperation {
                event: team_event.clone(),
                resulting_projection: serde_json::to_value(&team)?,
                immutable_side_records: vec![serde_json::to_value(team_inbox_subscription(
                    &context.execution_space_id,
                    &team,
                    MessageSubscriptionStatus::Active,
                    1,
                    &context.authenticated_actor,
                    &team.created_at,
                ))?],
                initial_outbox_records: Vec::new(),
            },
        });
        for membership in &memberships {
            store_sequence += 1;
            let payload = serde_json::to_value(membership)?;
            let membership_event = CanonicalMutationEvent {
                id: format!("trust-event-{store_sequence}"),
                aggregate_kind: "team_membership".into(),
                aggregate_id: membership.id.clone(),
                sequence: 1,
                store_sequence,
                transition: "joined_with_team".into(),
                expected_version: 0,
                resulting_version: membership.revision,
                performed_by_actor: context.authenticated_actor.clone(),
                authority_actor: context.authority_actor.clone(),
                causation_ref: Some(team_event.id.clone()),
                idempotency_key: format!(
                    "{}:initial-membership:{}",
                    context.idempotency_key, membership.id
                ),
                canonical_request_fingerprint: canonical_json_fingerprint(&payload),
                payload,
                created_at: now_string(),
            };
            let subscriptions = membership_subscriptions(
                &context.execution_space_id,
                membership,
                MessageSubscriptionStatus::Active,
                1,
                &membership.joined_at,
            )?
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: format!("{}:initial-membership", context.command_name),
                operation: CanonicalOperation {
                    event: membership_event,
                    resulting_projection: serde_json::to_value(membership)?,
                    immutable_side_records: subscriptions,
                    initial_outbox_records: Vec::new(),
                },
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: team,
            event: team_event,
            replayed: false,
        })
    }

    /// Atomically import one reviewed legacy Team projection without inferring
    /// identities or changing ids. Ambiguous Host/member maps fail before the
    /// trust ledger is mutated.
    pub fn migrate_legacy_agent_team_same_ids(
        &self,
        context: &MutationContext,
        bundle: AgentTeamMigrationBundle,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        bundle
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if context.expected_version != 0
            || bundle.source_fingerprint
                != canonical_json_fingerprint(&serde_json::to_value(&bundle.source)?)
            || bundle
                .memberships
                .iter()
                .any(|membership| membership.created_by != context.authenticated_actor)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "legacy Team migration requires exact source fingerprint, version 0 and authenticated membership creator",
                "agent_team",
                &bundle.target.id,
                Some(0),
            ));
        }
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::to_value(&bundle)?;
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team",
            &bundle.target.id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        for member_id in bundle.identity_id_map.values() {
            let member = members.get(member_id).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "legacy Team migration references a missing same-ID AgentMember",
                    "agent_team",
                    &bundle.target.id,
                    None,
                )
            })?;
            if bundle.target.status == AgentTeamStatus::Active
                && member.organization_status != AgentMemberOrganizationStatus::Active
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Active migrated Team requires every same-ID AgentMember to be Active",
                    "agent_team",
                    &bundle.target.id,
                    Some(member.version),
                ));
            }
        }
        let node = self
            .execution_nodes()?
            .into_iter()
            .find(|node| node.id == bundle.target.node_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "legacy Team migration references a missing immutable Node",
                    "agent_team",
                    &bundle.target.id,
                    None,
                )
            })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "legacy Team migration requires an Active immutable Node placement",
                "agent_team",
                &bundle.target.id,
                None,
            ));
        }
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let membership_ids = bundle
            .memberships
            .iter()
            .map(|membership| membership.id.as_str())
            .collect::<BTreeSet<_>>();
        if committed.iter().any(|envelope| {
            envelope.operation.event.aggregate_id == bundle.target.id
                && envelope.operation.event.aggregate_kind == "agent_team"
                || (envelope.operation.event.aggregate_kind == "team_membership"
                    && membership_ids.contains(envelope.operation.event.aggregate_id.as_str()))
        }) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "legacy Team migration target id already exists",
                "agent_team",
                &bundle.target.id,
                Some(0),
            ));
        }
        let mut store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let team_event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "agent_team".into(),
            aggregate_id: bundle.target.id.clone(),
            sequence: 1,
            store_sequence,
            transition: "migrated_same_ids".into(),
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: bundle.migration_id.clone(),
            canonical_request_fingerprint: request_fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        let subscription_status = if bundle.target.status == AgentTeamStatus::Active {
            MessageSubscriptionStatus::Active
        } else {
            MessageSubscriptionStatus::Paused
        };
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation: CanonicalOperation {
                event: team_event.clone(),
                resulting_projection: serde_json::to_value(&bundle.target)?,
                immutable_side_records: vec![serde_json::to_value(team_inbox_subscription(
                    &context.execution_space_id,
                    &bundle.target,
                    subscription_status,
                    1,
                    &context.authenticated_actor,
                    &bundle.target.updated_at,
                ))?],
                initial_outbox_records: Vec::new(),
            },
        });
        for membership in &bundle.memberships {
            store_sequence += 1;
            let membership_payload = serde_json::to_value(membership)?;
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: format!("{}:membership", context.command_name),
                operation: CanonicalOperation {
                    event: CanonicalMutationEvent {
                        id: format!("trust-event-{store_sequence}"),
                        aggregate_kind: "team_membership".into(),
                        aggregate_id: membership.id.clone(),
                        sequence: 1,
                        store_sequence,
                        transition: "migrated_same_id".into(),
                        expected_version: 0,
                        resulting_version: 1,
                        performed_by_actor: context.authenticated_actor.clone(),
                        authority_actor: context.authority_actor.clone(),
                        causation_ref: Some(team_event.id.clone()),
                        idempotency_key: format!("{}:{}", bundle.migration_id, membership.id),
                        canonical_request_fingerprint: canonical_json_fingerprint(
                            &membership_payload,
                        ),
                        payload: membership_payload,
                        created_at: now_string(),
                    },
                    resulting_projection: serde_json::to_value(membership)?,
                    immutable_side_records: membership_subscriptions(
                        &context.execution_space_id,
                        membership,
                        subscription_status,
                        1,
                        &bundle.target.updated_at,
                    )?
                    .into_iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
                    initial_outbox_records: Vec::new(),
                },
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: bundle.target,
            event: team_event,
            replayed: false,
        })
    }

    pub fn transition_agent_team(
        &self,
        context: &MutationContext,
        team_id: &str,
        next_status: AgentTeamStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "team_id": team_id,
            "next_status": next_status,
            "updated_at": updated_at,
        });
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team",
            team_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let mut current = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_team")?
            .remove(team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam not found",
                    "agent_team",
                    team_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentTeam>(&envelope))?;
        if context.expected_version != current.revision {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentTeam lifecycle CAS does not match the current revision",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        let allowed = matches!(
            (current.status, next_status),
            (AgentTeamStatus::Active, AgentTeamStatus::Inactive)
                | (AgentTeamStatus::Active, AgentTeamStatus::Trashed)
                | (AgentTeamStatus::Inactive, AgentTeamStatus::Active)
                | (AgentTeamStatus::Inactive, AgentTeamStatus::Trashed)
                | (AgentTeamStatus::Trashed, AgentTeamStatus::Inactive)
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentTeam lifecycle transition is not allowed",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        let members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        let mut memberships = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| membership.team_id == team_id)
            .collect::<Vec<_>>();
        let retained_hosts = memberships
            .iter()
            .filter(|membership| membership.role == TeamMembershipRole::Host)
            .collect::<Vec<_>>();
        if retained_hosts.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Inactive/Trashed/Restore requires exactly one retained Host role",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        let host_member = members
            .get(&retained_hosts[0].agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "retained Host TeamMembership references a missing AgentMember",
                    "agent_team",
                    team_id,
                    Some(current.revision),
                )
            })?;
        let authorized = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        ) || (context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == host_member.id);
        if !authorized {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentTeam lifecycle transition requires its retained Host or control-plane authority",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        if current.status == AgentTeamStatus::Trashed
            && next_status == AgentTeamStatus::Inactive
            && host_member.organization_status == AgentMemberOrganizationStatus::Retired
        {
            return Err(trust_error(
                TrustErrorCode::AgentMemberRetired,
                "Trashed AgentTeam cannot restore with a Retired retained Host",
                "agent_team",
                team_id,
                Some(current.revision),
            ));
        }
        if next_status == AgentTeamStatus::Active {
            let active_hosts = memberships
                .iter()
                .filter(|membership| {
                    membership.role == TeamMembershipRole::Host
                        && membership.state == TeamMembershipStatus::Active
                })
                .collect::<Vec<_>>();
            if active_hosts.len() != 1
                || members
                    .get(&active_hosts[0].agent_member_id)
                    .is_none_or(|member| {
                        member.organization_status != AgentMemberOrganizationStatus::Active
                    })
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam activation requires exactly one active Host Membership backed by an Active AgentMember",
                    "agent_team",
                    team_id,
                    Some(current.revision),
                ));
            }
            if memberships.iter().any(|membership| {
                membership.state == TeamMembershipStatus::Active
                    && members
                        .get(&membership.agent_member_id)
                        .is_none_or(|member| {
                            member.organization_status != AgentMemberOrganizationStatus::Active
                        })
            }) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam activation found an active Membership without an Active AgentMember",
                    "agent_team",
                    team_id,
                    Some(current.revision),
                ));
            }
        }
        let mut changed_memberships = Vec::new();
        if matches!(
            next_status,
            AgentTeamStatus::Inactive | AgentTeamStatus::Trashed
        ) && current.status != AgentTeamStatus::Trashed
        {
            for membership in &mut memberships {
                if membership.state != TeamMembershipStatus::Inactive {
                    membership.state = TeamMembershipStatus::Inactive;
                    membership.revision += 1;
                    changed_memberships.push(membership.clone());
                }
            }
        }
        current.status = next_status;
        current.revision += 1;
        current.updated_at = updated_at.to_string();
        current.trashed_at =
            (next_status == AgentTeamStatus::Trashed).then(|| updated_at.to_string());
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;

        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let previous_team_event = committed
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "agent_team"
                    && envelope.operation.event.aggregate_id == team_id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence)
            .map(|envelope| envelope.operation.event.clone())
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam canonical event is missing",
                    "agent_team",
                    team_id,
                    None,
                )
            })?;
        let current_subscriptions = self
            .fabric_message_subscriptions(&context.execution_space_id)?
            .into_iter()
            .map(|subscription| (subscription.id.clone(), subscription))
            .collect::<BTreeMap<_, _>>();
        let current_team_subscription = current_subscriptions
            .get(&format!("team-inbox:{team_id}"))
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam durable Team-subject subscription is missing",
                    "message_subscription",
                    &format!("team-inbox:{team_id}"),
                    None,
                )
            })?;
        let team_subscription = team_inbox_subscription(
            &context.execution_space_id,
            &current,
            if next_status == AgentTeamStatus::Active {
                MessageSubscriptionStatus::Active
            } else {
                MessageSubscriptionStatus::Paused
            },
            current_team_subscription.revision + 1,
            &current_team_subscription.created_by,
            updated_at,
        );
        let mut store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let team_event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "agent_team".into(),
            aggregate_id: team_id.to_string(),
            sequence: previous_team_event.sequence + 1,
            store_sequence,
            transition: match next_status {
                AgentTeamStatus::Active => "activated",
                AgentTeamStatus::Inactive if previous_team_event.transition == "trashed" => {
                    "restored"
                }
                AgentTeamStatus::Inactive => "deactivated",
                AgentTeamStatus::Trashed => "trashed",
            }
            .into(),
            expected_version: context.expected_version,
            resulting_version: current.revision,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: request_fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation: CanonicalOperation {
                event: team_event.clone(),
                resulting_projection: serde_json::to_value(&current)?,
                immutable_side_records: vec![serde_json::to_value(team_subscription)?],
                initial_outbox_records: Vec::new(),
            },
        });
        for membership in &changed_memberships {
            store_sequence += 1;
            let previous = committed
                .iter()
                .filter(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.operation.event.aggregate_kind == "team_membership"
                        && envelope.operation.event.aggregate_id == membership.id
                })
                .max_by_key(|envelope| envelope.operation.event.sequence)
                .map(|envelope| envelope.operation.event.clone())
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "TeamMembership canonical event is missing",
                        "team_membership",
                        &membership.id,
                        None,
                    )
                })?;
            let subscriptions = membership_subscriptions(
                &context.execution_space_id,
                membership,
                MessageSubscriptionStatus::Paused,
                current_subscriptions
                    .values()
                    .filter(|subscription| {
                        subscription.membership_ref.as_deref() == Some(membership.id.as_str())
                    })
                    .map(|subscription| subscription.revision)
                    .max()
                    .unwrap_or(0)
                    + 1,
                updated_at,
            )?
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
            let membership_payload = serde_json::json!({
                "team_event_id": team_event.id,
                "state": membership.state,
                "updated_at": updated_at,
            });
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: format!("{}:membership", context.command_name),
                operation: CanonicalOperation {
                    event: CanonicalMutationEvent {
                        id: format!("trust-event-{store_sequence}"),
                        aggregate_kind: "team_membership".into(),
                        aggregate_id: membership.id.clone(),
                        sequence: previous.sequence + 1,
                        store_sequence,
                        transition: "team_deactivated".into(),
                        expected_version: previous.resulting_version,
                        resulting_version: membership.revision,
                        performed_by_actor: context.authenticated_actor.clone(),
                        authority_actor: context.authority_actor.clone(),
                        causation_ref: Some(team_event.id.clone()),
                        idempotency_key: format!(
                            "{}:membership:{}",
                            context.idempotency_key, membership.id
                        ),
                        canonical_request_fingerprint: canonical_json_fingerprint(
                            &membership_payload,
                        ),
                        payload: membership_payload,
                        created_at: now_string(),
                    },
                    resulting_projection: serde_json::to_value(membership)?,
                    immutable_side_records: subscriptions,
                    initial_outbox_records: Vec::new(),
                },
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: current,
            event: team_event,
            replayed: false,
        })
    }

    /// Record purge authorization after every recoverable Team lifecycle and
    /// runtime reference is closed. This method never deletes related rows;
    /// physical legacy-ledger deletion remains outside DEV-35.
    pub fn record_agent_team_purge_tombstone(
        &self,
        context: &MutationContext,
        request: AgentTeamPurgeRequest,
    ) -> StoreResult<CanonicalMutationResult<AgentTeamPurgeTombstone>> {
        self.init()?;
        request
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if request.requested_by != context.authenticated_actor || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "Team purge tombstone requires the exact authenticated approved requester and version 0",
                "agent_team_purge_tombstone",
                &request.tombstone_id,
                Some(0),
            ));
        }
        let _lock = self.acquire_write_lock()?;
        let payload = serde_json::to_value(&request)?;
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "agent_team_purge_tombstone",
            &request.tombstone_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let team = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .find(|team| team.id == request.team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Team purge references a missing AgentTeam",
                    "agent_team",
                    &request.team_id,
                    None,
                )
            })?;
        if team.status != AgentTeamStatus::Trashed
            || team.revision != request.expected_team_revision
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team purge requires the exact current Trashed Team revision",
                "agent_team",
                &request.team_id,
                Some(team.revision),
            ));
        }
        let memberships = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| membership.team_id == team.id)
            .collect::<Vec<_>>();
        let member_ids = memberships
            .iter()
            .map(|membership| membership.agent_member_id.as_str())
            .collect::<BTreeSet<_>>();
        let has_active_reference = memberships
            .iter()
            .any(|membership| membership.state != TeamMembershipStatus::Inactive)
            || self.team_runs()?.iter().any(|run| {
                run.agent_team_id == team.id
                    && !matches!(
                        run.status,
                        firm_core::TeamRunStatus::Completed
                            | firm_core::TeamRunStatus::Failed
                            | firm_core::TeamRunStatus::Cancelled
                    )
            })
            || self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .iter()
                .any(|binding| {
                    binding.team_id == team.id
                        && matches!(
                            binding.status,
                            WorkExecutionBindingStatus::Offered
                                | WorkExecutionBindingStatus::Accepted
                                | WorkExecutionBindingStatus::Active
                        )
                })
            || self
                .fabric_agent_sessions(&context.execution_space_id)?
                .iter()
                .any(|session| {
                    member_ids.contains(session.agent_member_id.as_str())
                        && session.lifecycle != AgentSessionStatus::Closed
                })
            || self
                .fabric_message_deliveries(&context.execution_space_id)?
                .iter()
                .any(|delivery| {
                    delivery.target_team_id.as_deref() == Some(team.id.as_str())
                        && matches!(
                            delivery.status,
                            CanonicalMessageDeliveryStatus::Queued
                                | CanonicalMessageDeliveryStatus::Routed
                                | CanonicalMessageDeliveryStatus::Claimed
                                | CanonicalMessageDeliveryStatus::ProviderReceived
                        )
                });
        if has_active_reference {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team purge is blocked until memberships, runs, bindings, sessions and deliveries are closed",
                "agent_team",
                &request.team_id,
                Some(team.revision),
            ));
        }
        let tombstone = AgentTeamPurgeTombstone {
            id: request.tombstone_id.clone(),
            team_id: team.id,
            team_revision: team.revision,
            approval_ref: request.approval_ref,
            export_manifest_ref: request.export_manifest_ref,
            restore_window_closed_at: request.restore_window_closed_at,
            recorded_by: request.requested_by,
            recorded_at: request.requested_at,
        };
        self.commit_trust_projection_unlocked(
            context,
            "agent_team_purge_tombstone",
            &tombstone.id,
            "recorded_no_delete",
            payload,
            &tombstone,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn update_agent_team_profile(
        &self,
        context: &MutationContext,
        team_id: &str,
        name: &str,
        description: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentTeam>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(name, "AgentTeam.name")?;
        required(description, "AgentTeam.description")?;
        let mut team = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_team")?
            .remove(team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentTeam not found",
                    "agent_team",
                    team_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentTeam>(&envelope))?;
        team.name = name.to_string();
        team.description = description.to_string();
        team.revision += 1;
        team.updated_at = updated_at.to_string();
        team.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.commit_trust_projection_unlocked(
            context,
            "agent_team",
            team_id,
            "profile_updated",
            serde_json::json!({
                "name": name,
                "description": description,
                "updated_at": updated_at,
            }),
            &team,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Explicit pre-activation step used after Inactive/Restore. Activating a
    /// membership never starts or resumes an AgentSession.
    pub fn activate_team_membership(
        &self,
        context: &MutationContext,
        membership_id: &str,
        updated_at: &str,
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
        if membership.state != TeamMembershipStatus::Inactive {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an Inactive TeamMembership can be activated",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let team = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .find(|team| team.id == membership.team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing AgentTeam",
                    "team_membership",
                    membership_id,
                    None,
                )
            })?;
        if team.status != AgentTeamStatus::Inactive {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "TeamMembership activation is allowed only while its Team is Inactive",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let retained_host =
            self.team_host_membership(&context.execution_space_id, &membership.team_id, false)?;
        let authorized = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        ) || (context.authenticated_actor.kind == ActorKind::AgentMember
            && (context.authenticated_actor.id == membership.agent_member_id
                || context.authenticated_actor.id == retained_host.agent_member_id));
        if !authorized {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership activation requires the Member, retained Host, or control-plane authority",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == membership.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing AgentMember",
                    "team_membership",
                    membership_id,
                    None,
                )
            })?;
        if member.organization_status != AgentMemberOrganizationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "TeamMembership activation requires an Active AgentMember",
                "team_membership",
                membership_id,
                Some(member.version),
            ));
        }
        if membership.role == TeamMembershipRole::Host
            && self
                .fabric_team_memberships(&context.execution_space_id)?
                .iter()
                .any(|row| {
                    row.team_id == membership.team_id
                        && row.id != membership.id
                        && row.role == TeamMembershipRole::Host
                        && row.state == TeamMembershipStatus::Active
                })
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only one Host TeamMembership may be active",
                "team_membership",
                membership_id,
                Some(membership.revision),
            ));
        }
        let subscriptions = membership_subscriptions(
            &context.execution_space_id,
            &membership,
            MessageSubscriptionStatus::Active,
            self.fabric_message_subscriptions(&context.execution_space_id)?
                .into_iter()
                .filter(|subscription| {
                    subscription.membership_ref.as_deref() == Some(membership_id)
                })
                .map(|subscription| subscription.revision)
                .max()
                .unwrap_or(0)
                + 1,
            updated_at,
        )?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
        membership.state = TeamMembershipStatus::Active;
        membership.revision += 1;
        membership.left_at = None;
        self.commit_trust_projection_unlocked(
            context,
            "team_membership",
            membership_id,
            "activated",
            serde_json::json!({"updated_at": updated_at}),
            &membership,
            subscriptions,
            Vec::new(),
        )
    }

    /// AF-ADR-014 compatibility projection. There is no AgentIdentity writer:
    /// every row is derived from the sole durable AgentMember with the same id.
    pub fn fabric_agent_identities(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<AgentIdentity>> {
        Ok(self
            .trust_agent_members(execution_space_id)?
            .into_iter()
            .map(|member| AgentIdentity {
                id: member.id,
                display_name: member.name,
                organization_status: member.organization_status,
                permission_ceiling: member.permission_ceiling,
                version: member.version,
                created_at: member.created_at,
                updated_at: member.updated_at,
            })
            .collect())
    }

    #[deprecated(note = "AgentIdentity is a same-id read-only AgentMember projection")]
    pub fn create_agent_identity(
        &self,
        _context: &MutationContext,
        identity: AgentIdentity,
    ) -> StoreResult<CanonicalMutationResult<AgentIdentity>> {
        Err(trust_error(
            TrustErrorCode::InvalidStateTransition,
            "AGENT_IDENTITY_READ_ONLY: create the sole durable AgentMember instead",
            "agent_identity",
            &identity.id,
            None,
        ))
    }

    /// Explicit one-way AF-ADR-014 migration. The legacy projection id is
    /// preserved exactly while the only written durable aggregate is
    /// AgentMember; no AgentIdentity event or ledger is created.
    pub fn migrate_legacy_agent_identity_same_id(
        &self,
        context: &MutationContext,
        identity: AgentIdentity,
    ) -> StoreResult<CanonicalMutationResult<AgentIdentity>> {
        required(&identity.id, "AgentIdentity.id")?;
        if identity.version != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "legacy AgentIdentity migration requires an explicit version-1 same-ID source",
                "agent_member",
                &identity.id,
                Some(identity.version),
            ));
        }
        let member = AgentMember {
            id: identity.id.clone(),
            name: identity.display_name.clone(),
            description: "Migrated same-ID AgentMember authority".into(),
            role: "agent".into(),
            capabilities: Vec::new(),
            skill_refs: Vec::new(),
            provider_profile_ref: None,
            model_preference: None,
            workspace_policy: "legacy-explicit-migration".into(),
            permission_ceiling: identity.permission_ceiling,
            organization_status: identity.organization_status,
            version: 1,
            created_by: context.authenticated_actor.clone(),
            created_at: identity.created_at.clone(),
            updated_at: identity.updated_at.clone(),
        };
        let migrated = self.create_trust_agent_member(context, member)?;
        let projection = self
            .fabric_agent_identities(&context.execution_space_id)?
            .into_iter()
            .find(|candidate| candidate.id == identity.id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "same-ID AgentIdentity projection was not reconstructable after migration",
                    "agent_member",
                    &identity.id,
                    Some(1),
                )
            })?;
        Ok(CanonicalMutationResult {
            projection,
            event: migrated.event,
            replayed: migrated.replayed,
        })
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
        required(&session.agent_member_id, "AgentSession.agent_member_id")?;
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
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == session.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession references a missing AgentMember",
                    "agent_session",
                    &session.id,
                    None,
                )
            })?;
        if member.organization_status != AgentMemberOrganizationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession requires an Active AgentMember",
                "agent_session",
                &session.id,
                None,
            ));
        }
        if session.effective_permission_ceiling > member.permission_ceiling {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentSession effective permission exceeds the frozen AgentMember ceiling",
                "agent_session",
                &session.id,
                None,
            ));
        }
        let current_count = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .filter(|row| {
                row.agent_member_id == session.agent_member_id
                    && row.lifecycle != AgentSessionStatus::Closed
            })
            .count();
        if current_count != 0 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentMember already has a current AgentSession; explicit stop or recovery is required",
                "agent_member",
                &session.agent_member_id,
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
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            if envelope.operation.event.aggregate_kind == "team_membership" {
                let membership = event_projection::<TeamMembership>(&envelope)?;
                latest.insert(membership.id.clone(), membership);
            }
            for value in envelope
                .operation
                .initial_outbox_records
                .iter()
                .chain(&envelope.operation.immutable_side_records)
            {
                if let Ok(membership) = serde_json::from_value::<TeamMembership>(value.clone()) {
                    latest.insert(membership.id.clone(), membership);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn team_host_membership(
        &self,
        execution_space_id: &str,
        team_id: &str,
        require_active: bool,
    ) -> StoreResult<TeamMembership> {
        let matching = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team_id
                    && membership.role == TeamMembershipRole::Host
                    && (!require_active || membership.state == TeamMembershipStatus::Active)
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "AgentTeam requires exactly one {}Host TeamMembership; found {}",
                    if require_active { "active " } else { "" },
                    matching.len()
                ),
                "agent_team",
                team_id,
                None,
            ));
        }
        Ok(matching.into_iter().next().expect("length checked"))
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
            &membership.agent_member_id,
            "TeamMembership.agent_member_id",
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
        let team = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .find(|team| team.id == membership.team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing durable AgentTeam",
                    "team_membership",
                    &membership.id,
                    None,
                )
            })?;
        if team.status != AgentTeamStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new TeamMembership requires an Active AgentTeam",
                "team_membership",
                &membership.id,
                Some(team.revision),
            ));
        }
        if team.node_id != membership.node_id {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership must remain on the Team's immutable Node",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        if membership.created_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership.created_by must equal the authenticated actor",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == membership.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamMembership references a missing AgentMember",
                    "team_membership",
                    &membership.id,
                    None,
                )
            })?;
        if member.organization_status != AgentMemberOrganizationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "TeamMembership requires an Active AgentMember",
                "team_membership",
                &membership.id,
                Some(member.version),
            ));
        }
        // Membership is a generation-fenced collaboration binding.  The
        // cardinality check and the append deliberately share this Store
        // write lock so two concurrent joins cannot both observe an empty
        // active set and create ambiguous authority.
        let prior_memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        if prior_memberships.iter().any(|row| {
            row.team_id == membership.team_id
                && row.agent_member_id == membership.agent_member_id
                && row.state == TeamMembershipStatus::Active
        }) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team and AgentMember already have an active TeamMembership generation",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        let expected_generation = prior_memberships
            .iter()
            .filter(|row| {
                row.team_id == membership.team_id
                    && row.agent_member_id == membership.agent_member_id
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
        if membership.role == TeamMembershipRole::Host
            && prior_memberships.iter().any(|row| {
                row.team_id == membership.team_id
                    && row.role == TeamMembershipRole::Host
                    && row.state == TeamMembershipStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "an Active AgentTeam cannot have more than one active Host TeamMembership",
                "team_membership",
                &membership.id,
                None,
            ));
        }
        let subscriptions = membership_subscriptions(
            &context.execution_space_id,
            &membership,
            MessageSubscriptionStatus::Active,
            1,
            &membership.joined_at,
        )?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
        self.commit_trust_projection_unlocked(
            context,
            "team_membership",
            &membership.id,
            "joined",
            serde_json::to_value(&membership)?,
            &membership,
            subscriptions,
            Vec::new(),
        )
    }

    pub fn fabric_message_subscriptions(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MessageSubscription>> {
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            if envelope.operation.event.aggregate_kind == "message_subscription" {
                let subscription = event_projection::<MessageSubscription>(&envelope)?;
                latest.insert(subscription.id.clone(), subscription);
            }
            for value in envelope
                .operation
                .initial_outbox_records
                .iter()
                .chain(&envelope.operation.immutable_side_records)
            {
                if let Ok(subscription) =
                    serde_json::from_value::<MessageSubscription>(value.clone())
                {
                    latest.insert(subscription.id.clone(), subscription);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn create_message_subscription(
        &self,
        context: &MutationContext,
        subscription: MessageSubscription,
    ) -> StoreResult<CanonicalMutationResult<MessageSubscription>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(&subscription.id, "MessageSubscription.id")?;
        required(
            &subscription.subscriber_ref,
            "MessageSubscription.subscriber_ref",
        )?;
        required(
            &subscription.target_node_id,
            "MessageSubscription.target_node_id",
        )?;
        if subscription.execution_space_id != context.execution_space_id
            || subscription.revision != 1
            || subscription.status != MessageSubscriptionStatus::Active
            || subscription.created_by != context.authenticated_actor
            || context.expected_version != 0
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new MessageSubscription must be active revision 1 in the authenticated Execution Space",
                "message_subscription",
                &subscription.id,
                Some(0),
            ));
        }
        match subscription.subscriber_kind {
            MessageSubjectKind::AgentMember => {
                let member = self
                    .trust_agent_members(&context.execution_space_id)?
                    .into_iter()
                    .find(|member| member.id == subscription.subscriber_ref)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "AgentMember subscription references a missing AgentMember",
                            "message_subscription",
                            &subscription.id,
                            None,
                        )
                    })?;
                if member.organization_status != AgentMemberOrganizationStatus::Active {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "AgentMember subscription requires an Active AgentMember",
                        "message_subscription",
                        &subscription.id,
                        Some(member.version),
                    ));
                }
            }
            MessageSubjectKind::Team => {
                let target_team_id = subscription.target_team_id.as_deref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Team-subject subscription requires target_team_id",
                        "message_subscription",
                        &subscription.id,
                        None,
                    )
                })?;
                let team = self
                    .agent_teams(&context.execution_space_id)?
                    .into_iter()
                    .find(|team| team.id == target_team_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "Team-subject subscription references a missing AgentTeam",
                            "message_subscription",
                            &subscription.id,
                            None,
                        )
                    })?;
                if subscription.subscriber_ref != team.id
                    || subscription.target_node_id != team.node_id
                    || subscription.membership_ref.is_some()
                    || subscription.source_kind != MessageSubscriptionKind::AllAuthorized
                    || subscription.source_ref != "authorized_peer_teams"
                    || subscription.authorization_policy_ref != "collaboration.peer_message_deliver"
                    || team.status != AgentTeamStatus::Active
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Team-subject subscription must name one Active Team/Node and cannot preselect a membership",
                        "message_subscription",
                        &subscription.id,
                        Some(team.revision),
                    ));
                }
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "message_subscription",
            &subscription.id,
            "created",
            serde_json::to_value(&subscription)?,
            &subscription,
            Vec::new(),
            Vec::new(),
        )
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
        if membership.role == TeamMembershipRole::Host
            && self
                .agent_teams(&context.execution_space_id)?
                .into_iter()
                .find(|team| team.id == membership.team_id)
                .is_some_and(|team| team.status == AgentTeamStatus::Active)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "the sole active Host Membership cannot leave an Active AgentTeam; deactivate the Team first",
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
        let host_id = self
            .team_host_membership(&context.execution_space_id, &membership.team_id, false)?
            .agent_member_id;
        let authorized = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        ) || (context.authenticated_actor.kind == ActorKind::AgentMember
            && (context.authenticated_actor.id == membership.agent_member_id
                || context.authenticated_actor.id == host_id));
        if !authorized {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "TeamMembership leave requires the exact stable AgentMember",
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

    /// Transfer one quiescent AgentSession to the current NodeDaemon
    /// generation without changing the provider-native session identity or
    /// the AgentSession runtime generation. The daemon and driver generations
    /// are independent fences: advancing them invalidates every old provider
    /// command while preserving exact WorkExecutionBindings to this session.
    ///
    /// A session that may have owned a provider process can move only after
    /// the predecessor lease was explicitly released. Lease expiry alone is
    /// not evidence that writable children were drained.
    #[allow(clippy::too_many_arguments)] // exact old/new daemon and session fences stay explicit at this mutation boundary
    pub fn reattach_agent_session_to_node_daemon(
        &self,
        context: &MutationContext,
        session_id: &str,
        expected_runtime_generation: u64,
        expected_predecessor_daemon_generation: u64,
        successor_daemon_id: &str,
        successor_daemon_generation: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "session_id": session_id,
            "expected_runtime_generation": expected_runtime_generation,
            "expected_predecessor_daemon_generation": expected_predecessor_daemon_generation,
            "successor_daemon_id": successor_daemon_id,
            "successor_daemon_generation": successor_daemon_generation,
            "updated_at": updated_at,
        });
        let request_fingerprint = canonical_json_fingerprint(&request_payload);
        let mut commit_context = context.clone();
        commit_context.request_fingerprint = Some(request_fingerprint.clone());
        if let Some(replay) = self.replay_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }

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
            successor_daemon_id,
            successor_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            session_id,
        )?;
        if session.runtime_generation != expected_runtime_generation
            || session.node_daemon_generation != expected_predecessor_daemon_generation
            || expected_predecessor_daemon_generation >= successor_daemon_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "AgentSession reattach used a stale session or daemon generation",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        let lane_is_quiescent = matches!(
            session.lifecycle,
            AgentSessionStatus::Cold | AgentSessionStatus::Idle | AgentSessionStatus::Interrupted
        ) && session.current_turn_id.is_none()
            && session.queued_input_count == 0
            && matches!(
                session.control_state.runtime_residency,
                RuntimeResidency::Detached | RuntimeResidency::Attached
            )
            && session.control_state.activity == RuntimeActivity::Idle
            && session.control_state.handoff_state
                == firm_core::agentfirm_api::DriverHandoffState::None
            && matches!(
                session.control_state.continuation.activation,
                NativeContinuationActivation::Disarmed
            );
        if !lane_is_quiescent {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession reattach requires a quiescent, continuation-disarmed lane with no queued native input",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        let ambiguous_effect = self
            .runtime_commands(&context.execution_space_id)?
            .into_iter()
            .any(|command| {
                command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && command.target_node_daemon_generation
                        == expected_predecessor_daemon_generation
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
                    && matches!(
                        command.status,
                        RuntimeCommandStatus::Accepted
                            | RuntimeCommandStatus::Quiesced
                            | RuntimeCommandStatus::RecoveryRequired
                    )
            });
        if ambiguous_effect {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession reattach requires reconciliation of every predecessor RuntimeCommand",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let predecessor_was_released = self
            .read_jsonl::<firm_core::NodeDaemonLease>("node_daemon_leases.jsonl")?
            .into_iter()
            .rfind(|lease| {
                lease.node_id == session.node_id
                    && lease.daemon_id == session.node_daemon_id
                    && lease.generation == expected_predecessor_daemon_generation
            })
            .is_some_and(|lease| lease.status == firm_core::NodeDaemonLeaseStatus::Released);
        let predecessor_may_have_owned_runtime = session.native_session_ref.is_some()
            || session.control_state.runtime_residency != RuntimeResidency::Detached;
        if predecessor_may_have_owned_runtime && !predecessor_was_released {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentSession reattach requires an explicit predecessor NodeDaemon release; lease expiry is not a provider-drain receipt",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let predecessor_daemon_id = session.node_daemon_id.clone();
        session.node_daemon_id = successor_daemon_id.to_string();
        session.node_daemon_generation = successor_daemon_generation;
        session.control_state.runtime_residency = RuntimeResidency::Detached;
        session.control_state.activity = RuntimeActivity::Idle;
        session.control_state.driver_generation = session
            .control_state
            .driver_generation
            .saturating_add(1)
            .max(1);
        session.control_state.driver_ref = RuntimeDriverRef::NodeDaemon {
            node_daemon_id: successor_daemon_id.to_string(),
            node_daemon_generation: successor_daemon_generation,
        };
        session.control_state.handoff_state = firm_core::agentfirm_api::DriverHandoffState::None;
        session.control_state.continuation.activation = NativeContinuationActivation::Disarmed;
        session.control_state.last_reconciled_at = Some(updated_at.to_string());
        session.version += 1;
        session.last_active_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            "node_daemon_reattached",
            serde_json::json!({
                "predecessor_daemon_id": predecessor_daemon_id,
                "predecessor_daemon_generation": expected_predecessor_daemon_generation,
                "successor_daemon_id": successor_daemon_id,
                "successor_daemon_generation": successor_daemon_generation,
                "runtime_generation": session.runtime_generation,
                "driver_generation": session.control_state.driver_generation,
                "updated_at": updated_at,
            }),
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Write the settled provider-native Session binding onto the canonical
    /// AgentSession. A fresh-start session is materialized before the provider
    /// thread exists (`native_session_ref` starts unset), so the settled binding
    /// lands later as its own CAS + generation-fenced mutation. Lifecycle and
    /// runtime generation are untouched. The write is idempotent for the same
    /// native id and rejects a conflicting rebind to another id.
    pub fn bind_agent_session_native_session(
        &self,
        context: &MutationContext,
        session_id: &str,
        expected_generation: u64,
        native_session_ref: NativeSessionRef,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(
            &native_session_ref.native_session_id,
            "NativeSessionRef.native_session_id",
        )?;
        required(&native_session_ref.provider, "NativeSessionRef.provider")?;
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
        if session.lifecycle == AgentSessionStatus::Closed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "a closed AgentSession cannot bind a provider-native Session",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if session.runtime_generation != expected_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                format!(
                    "AgentSession runtime generation is {}, the settled binding observed {expected_generation}",
                    session.runtime_generation
                ),
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if let Some(current) = session.native_session_ref.as_ref() {
            if current.native_session_id != native_session_ref.native_session_id {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession already binds another provider-native Session",
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
        }
        session.native_session_ref = Some(native_session_ref.clone());
        session.version += 1;
        self.commit_trust_projection_unlocked(
            context,
            "agent_session",
            session_id,
            "native_session_bound",
            serde_json::json!({
                "session_id": session_id,
                "runtime_generation": expected_generation,
                "native_session_ref": native_session_ref,
            }),
            &session,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Replace the bounded runtime-control projection for one exact session
    /// generation.  This is not a runtime event stream: it is the current
    /// fencing state used to decide whether a later provider effect is still
    /// authorized. Driver or composition changes require a provably quiet
    /// lane and advance the driver generation exactly once.
    pub fn bind_agent_session_control_state(
        &self,
        context: &MutationContext,
        session_id: &str,
        expected_runtime_generation: u64,
        next_control_state: AgentSessionControlState,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "session_id": session_id,
            "expected_runtime_generation": expected_runtime_generation,
            "next_control_state": next_control_state,
            "updated_at": updated_at,
        });
        let request_fingerprint = canonical_json_fingerprint(&request_payload);
        let mut commit_context = context.clone();
        commit_context.request_fingerprint = Some(request_fingerprint.clone());
        if let Some(replay) = self.replay_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }

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
        if session.runtime_generation != expected_runtime_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "control-state mutation used a stale AgentSession runtime generation",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        let ambiguous = self
            .runtime_commands(&context.execution_space_id)?
            .into_iter()
            .any(|command| {
                command.target_session_id.as_deref() == Some(session.id.as_str())
                    && command.target_session_generation == Some(session.runtime_generation)
                    && command.effect_certainty == RuntimeEffectCertainty::Unknown
                    && matches!(
                        command.status,
                        RuntimeCommandStatus::Accepted
                            | RuntimeCommandStatus::Quiesced
                            | RuntimeCommandStatus::RecoveryRequired
                    )
            });
        if ambiguous {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "control-state mutation requires reconciliation of every ambiguous RuntimeCommand",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let driver_changed = session.control_state.execution_driver
            != next_control_state.execution_driver
            || session.control_state.driver_ref != next_control_state.driver_ref
            || session.control_state.driver_generation != next_control_state.driver_generation;
        let composition_changed = session.control_state.composition_fingerprint
            != next_control_state.composition_fingerprint
            || session.control_state.capability_fingerprint
                != next_control_state.capability_fingerprint;
        if driver_changed || composition_changed {
            let lane_is_quiet = session.current_turn_id.is_none()
                && (session.control_state.runtime_residency == RuntimeResidency::Detached
                    || session.control_state.activity == RuntimeActivity::Idle);
            if !lane_is_quiet {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "driver/composition transfer requires a provably Detached or Idle execution lane",
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
            if next_control_state.driver_generation
                != session.control_state.driver_generation.saturating_add(1)
                || next_control_state.handoff_state
                    != firm_core::agentfirm_api::DriverHandoffState::None
            {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "driver/composition transfer must advance the driver generation exactly once and finish the handoff",
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
        } else if next_control_state.driver_generation == 0 {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "an active control-state binding requires a non-zero driver generation",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }

        let mut candidate = session.clone();
        candidate.control_state = next_control_state.clone();
        match candidate.control_state.execution_driver {
            MemberExecutionDriver::UserDriven => {
                if candidate.control_state.driver_generation == 0
                    || candidate.control_state.driver_ref != RuntimeDriverRef::Unknown
                    || matches!(
                        candidate.control_state.continuation.activation,
                        NativeContinuationActivation::Armed { .. }
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::MemberRunGenerationFenced,
                        "user-driven runtimes must remain non-driven by Harness and continuation-disarmed",
                        "agent_session",
                        session_id,
                        Some(session.version),
                    ));
                }
            }
            MemberExecutionDriver::HostDriven | MemberExecutionDriver::ProviderDriven => {
                let candidate_binding = runtime_binding_for_session(&candidate);
                self.require_live_runtime_binding_unlocked(
                    &candidate,
                    &candidate_binding,
                    false,
                    "agent_session",
                    session_id,
                    Some(session.version),
                )?;
            }
        }

        session.control_state = next_control_state;
        session.version += 1;
        self.commit_trust_projection_unlocked(
            &commit_context,
            "agent_session",
            session_id,
            "control_state_bound",
            request_payload,
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
            .materialized_fabric_work_deliveries_unlocked(execution_space_id)?
            .into_values()
            .collect())
    }

    fn materialized_fabric_work_deliveries_unlocked(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<BTreeMap<String, CanonicalWorkDelivery>> {
        let mut latest = self.latest_fabric_side_records_unlocked(
            execution_space_id,
            |row: &CanonicalWorkDelivery| row.id.clone(),
        )?;
        let works = self.latest_works_unlocked()?;
        let sessions = self.fabric_agent_sessions(execution_space_id)?;
        for binding in self.fabric_work_execution_bindings(execution_space_id)? {
            if binding.status != WorkExecutionBindingStatus::Active
                || latest.contains_key(&binding.delivery_id)
            {
                continue;
            }
            let Some(work) = works.get(&binding.work_id) else {
                continue;
            };
            let Some(session) = sessions
                .iter()
                .find(|session| session.id == binding.agent_session_id)
            else {
                continue;
            };
            if work.version != binding.work_revision
                || session.agent_member_id != binding.agent_member_id
                || session.runtime_generation != binding.agent_session_generation
                || session.lifecycle == AgentSessionStatus::Closed
            {
                continue;
            }
            latest.insert(
                binding.delivery_id.clone(),
                CanonicalWorkDelivery {
                    id: binding.delivery_id.clone(),
                    work_id: binding.work_id.clone(),
                    work_revision: binding.work_revision,
                    work_execution_binding_id: binding.id.clone(),
                    recipient_agent_member_id: binding.agent_member_id.clone(),
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
                },
            );
        }
        Ok(latest)
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
            .materialized_fabric_work_deliveries_unlocked(&context.execution_space_id)?
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
            || session.agent_member_id != delivery.recipient_agent_member_id
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
        let invocation_binding = runtime_binding_for_session(&session);
        self.require_live_runtime_binding_unlocked(
            &session,
            &invocation_binding,
            false,
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
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
            recipient_agent_member_id: delivery.recipient_agent_member_id.clone(),
            recipient_session_id: delivery.recipient_session_id.clone(),
            recipient_session_generation: delivery.recipient_session_generation,
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
            provider: session.provider_kind.clone(),
            dispatch_mode,
            binding: invocation_binding,
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
            || membership.agent_member_id != binding.agent_member_id
            || session.agent_member_id != binding.agent_member_id
            || session.node_id != membership.node_id
            || session.runtime_generation != binding.agent_session_generation
            || session.lifecycle == AgentSessionStatus::Closed
            || work.version != binding.work_revision
            || work.accountable_team_id.as_deref() != Some(membership.team_id.as_str())
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
                recipient_agent_member_id: binding.agent_member_id.clone(),
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
            && context.authenticated_actor.id == binding.agent_member_id;
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
        self.author_message_with_admission_authority(context, message, None)
    }

    /// Compatibility entry point for persisted pre-DEV-35 daemon payloads.
    /// New callers serialize [`MessageAdmissionAuthority`] explicitly.
    pub fn author_message_with_collaboration_authority(
        &self,
        context: &MutationContext,
        message: Message,
        collaboration_authority: Option<&CollaborationMessageAuthority>,
    ) -> StoreResult<CanonicalMutationResult<Message>> {
        let authority = collaboration_authority
            .cloned()
            .map(MessageAdmissionAuthority::WorkDelegation);
        self.author_message_with_admission_authority(context, message, authority.as_ref())
    }

    pub fn author_message_with_admission_authority(
        &self,
        context: &MutationContext,
        message: Message,
        admission_authority: Option<&MessageAdmissionAuthority>,
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
        if let Some(sender_agent_member_id) = message.sender_agent_member_id.as_deref() {
            let sender_sessions = self
                .fabric_agent_sessions(&context.execution_space_id)?
                .into_iter()
                .filter(|session| {
                    session.agent_member_id == sender_agent_member_id
                        && session.node_id == message.source_node_id
                        && session.node_daemon_generation == message.source_authority_generation
                        && session.lifecycle != AgentSessionStatus::Closed
                        && message.sender_session_id.as_deref() == Some(session.id.as_str())
                })
                .count();
            if sender_sessions != 1 || message.sender_actor_ref.id != sender_agent_member_id {
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
        let collaboration_authority = match admission_authority {
            Some(MessageAdmissionAuthority::WorkDelegation(authority)) => Some(authority),
            _ => None,
        };
        let peer_authority = match admission_authority {
            Some(MessageAdmissionAuthority::PeerTeam(authority)) => Some(authority),
            _ => None,
        };
        if let Some(authority) = peer_authority {
            self.validate_peer_team_message_admission_unlocked(
                context,
                &message,
                authority,
                &sessions,
                &memberships,
            )?;
        }
        if let Some(authority) = collaboration_authority {
            let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "cross-Team Message lacks frozen CollaborationScope",
                    "message",
                    &message.id,
                    None,
                )
            })?;
            let expected_authority_digest = canonical_json_fingerprint(&serde_json::json!({
                "company_id": authority.company_id,
                "delegation_id": authority.delegation_id,
                "delegation_revision": authority.delegation_revision,
                "source_work_ref": authority.source_work_ref,
                "target_work_ref": authority.target_work_ref,
                "target_placement": authority.target_placement,
                "source_owner_ref": authority.source_owner_ref,
                "source_host_ref": authority.source_host_ref,
                "target_host_ref": authority.target_host_ref,
                "inbound_policy_snapshot": authority.inbound_policy_snapshot,
            }));
            let source_work = self
                .latest_works()?
                .into_iter()
                .find(|work| work.id == authority.source_work_ref.work_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "Delegation source Work is not current in the source Execution Space",
                        "message",
                        &message.id,
                        None,
                    )
                })?;
            let source_team_revision = self
                .teams()?
                .iter()
                .filter(|team| team.id == authority.source_work_ref.team_id)
                .count() as u64;
            let exact_source_scope = authority.authority_digest == expected_authority_digest
                && authority.delegation_revision > 0
                && scope.delegation_id.as_deref() == Some(authority.delegation_id.as_str())
                && scope.expected_delegation_revision == Some(authority.delegation_revision)
                && scope.source_work_ref.as_ref() == Some(&authority.source_work_ref)
                && scope.target_work_ref.as_ref() == Some(&authority.target_work_ref)
                && scope.source_team_id == authority.source_work_ref.team_id
                && scope.target_team_id == authority.target_placement.team_id
                && message.team_id.as_deref() == Some(authority.source_work_ref.team_id.as_str())
                && message.work_id.as_deref() == Some(authority.source_work_ref.work_id.as_str())
                && authority.source_work_ref.execution_space_id == context.execution_space_id
                && authority.source_work_ref.node_id == message.source_node_id
                && authority.source_work_ref.placement_generation == 1
                && authority.source_work_ref.team_revision == source_team_revision
                && source_work.id == authority.source_work_ref.work_id
                && source_work.accountable_team_id.as_deref()
                    == Some(authority.source_work_ref.team_id.as_str())
                && source_work.version == authority.source_work_ref.work_revision;
            let current_owner_bindings = self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .into_iter()
                .filter(|binding| {
                    binding.work_id == source_work.id
                        && binding.work_revision == source_work.version
                        && binding.team_id == authority.source_work_ref.team_id
                        && binding.agent_member_id == authority.source_owner_ref.id
                        && sessions.iter().any(|session| {
                            session.id == binding.agent_session_id
                                && session.runtime_generation == binding.agent_session_generation
                                && session.node_daemon_generation
                                    == message.source_authority_generation
                                && session.lifecycle != AgentSessionStatus::Closed
                        })
                        && binding.status == WorkExecutionBindingStatus::Active
                })
                .collect::<Vec<_>>();
            let exact_owner_binding = message.sender_actor_ref == authority.source_owner_ref
                && message.sender_agent_member_id.as_deref()
                    == Some(authority.source_owner_ref.id.as_str())
                && current_owner_bindings.len() == 1
                && message.sender_session_id.as_deref()
                    == Some(current_owner_bindings[0].agent_session_id.as_str());
            let exact_source_host = message.sender_actor_ref == authority.source_host_ref
                && message.sender_agent_member_id.as_deref()
                    == Some(authority.source_host_ref.id.as_str())
                && memberships
                    .iter()
                    .filter(|membership| {
                        membership.team_id == authority.source_work_ref.team_id
                            && membership.agent_member_id == authority.source_host_ref.id
                            && membership.role == TeamMembershipRole::Host
                            && membership.state == TeamMembershipStatus::Active
                    })
                    .count()
                    == 1
                && current_owner_bindings.len() == 1;
            if !exact_source_scope || (!exact_owner_binding && !exact_source_host) {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "cross-Team Message requires exact current Delegation, source Work, and source owner binding or Host membership",
                    "message",
                    &message.id,
                    Some(source_work.version),
                ));
            }
        } else if message.collaboration_scope.is_some() && peer_authority.is_none() {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "cross-Team Message authoring requires server-frozen Delegation authority",
                "message",
                &message.id,
                None,
            ));
        }
        if let Some(team_id) = message.team_id.as_deref() {
            let sender_is_member =
                message
                    .sender_agent_member_id
                    .as_deref()
                    .is_some_and(|sender| {
                        memberships.iter().any(|membership| {
                            membership.team_id == team_id
                                && membership.agent_member_id == sender
                                && membership.state == TeamMembershipStatus::Active
                        })
                    });
            let control_plane_sender = message.sender_agent_member_id.is_none()
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
        let mut delivered_subjects = BTreeSet::new();
        // A peer-Team direct Message binds the recipient membership in the
        // collaboration target Team, not the source Team (the author's scope).
        // Same-Space peer authoring resolves the target direct subscription in
        // this store; a remote target leaves delivery creation to its own Node.
        let peer_target_team_id = peer_authority.map(|authority| authority.target_team_id.as_str());
        for recipient in &message.recipients {
            let matching = subscriptions.iter().filter(|subscription| {
                subscription.status == MessageSubscriptionStatus::Active
                    && match recipient.kind {
                        MessageRecipientKind::AgentMember => {
                            subscription.subscriber_kind == MessageSubjectKind::AgentMember
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind == MessageSubscriptionKind::Agent
                                && if let Some(team_id) = message.team_id.as_deref() {
                                    subscription.membership_ref.as_deref().is_some_and(
                                        |membership_id| {
                                            memberships.iter().any(|membership| {
                                                membership.id == membership_id
                                                    && membership.state
                                                        == TeamMembershipStatus::Active
                                                    && membership.team_id
                                                        == peer_target_team_id.unwrap_or(team_id)
                                            })
                                        },
                                    )
                                } else {
                                    subscription.membership_ref.is_none()
                                        && message.sender_agent_member_id.as_deref()
                                            == Some(subscription.source_ref.as_str())
                                }
                        }
                        MessageRecipientKind::Team => {
                            subscription.subscriber_kind == MessageSubjectKind::Team
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind
                                    == MessageSubscriptionKind::AllAuthorized
                                && subscription.source_ref == "authorized_peer_teams"
                                && subscription.target_team_id.as_deref()
                                    == Some(recipient.id.as_str())
                        }
                        MessageRecipientKind::ControlPlaneActor => false,
                    }
            });
            for subscription in matching {
                let subject_key = (
                    subscription.subscriber_kind,
                    subscription.subscriber_ref.clone(),
                );
                if !delivered_subjects.insert(subject_key) {
                    continue;
                }
                let resolved_team_membership_id = (subscription.subscriber_kind
                    == MessageSubjectKind::AgentMember)
                    .then(|| subscription.membership_ref.clone())
                    .flatten();
                let recipient_agent_member_id = (subscription.subscriber_kind
                    == MessageSubjectKind::AgentMember)
                    .then(|| subscription.subscriber_ref.clone());
                delivery_rows.push(CanonicalMessageDelivery {
                    id: format!("{}:{}", message.id, subscription.id),
                    message_id: message.id.clone(),
                    subscription_id: subscription.id.clone(),
                    subscription_revision: subscription.revision,
                    subscription_policy_digest: subscription.policy_digest.clone(),
                    recipient_kind: subscription.subscriber_kind,
                    recipient_ref: subscription.subscriber_ref.clone(),
                    target_team_id: subscription.target_team_id.clone(),
                    target_node_id: subscription.target_node_id.clone(),
                    resolved_team_membership_id,
                    recipient_agent_member_id,
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

    fn validate_peer_team_message_admission_unlocked(
        &self,
        context: &MutationContext,
        message: &Message,
        authority: &PeerTeamMessageAdmissionAuthority,
        sessions: &[AgentSession],
        memberships: &[TeamMembership],
    ) -> StoreResult<()> {
        let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team Message lacks frozen CollaborationScope",
                "message",
                &message.id,
                None,
            )
        })?;
        let expected_source_policy_digest = peer_team_source_policy_digest(authority);
        let expected_target_policy_digest = peer_team_target_policy_digest(authority);
        let expected_authority_digest = peer_team_message_authority_digest(authority);
        let source_teams = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .filter(|team| team.id == authority.source_team_id)
            .collect::<Vec<_>>();
        if source_teams.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team source Team is missing or ambiguous",
                "message",
                &message.id,
                None,
            ));
        }
        let source_team = &source_teams[0];
        let exact_membership = memberships
            .iter()
            .filter(|membership| {
                membership.id == authority.source_membership_id
                    && membership.team_id == authority.source_team_id
                    && membership.agent_member_id == authority.source_agent_member_id
                    && membership.node_id == authority.source_node_id
                    && membership.membership_generation == authority.source_membership_generation
                    && membership.state == TeamMembershipStatus::Active
            })
            .count()
            == 1;
        let exact_session = sessions
            .iter()
            .filter(|session| {
                session.id == authority.source_session_id
                    && session.agent_member_id == authority.source_agent_member_id
                    && session.node_id == authority.source_node_id
                    && session.node_daemon_id == authority.source_node_daemon_id
                    && session.node_daemon_generation == authority.source_node_daemon_generation
                    && session.runtime_generation == authority.source_session_generation
                    && session.lifecycle != AgentSessionStatus::Closed
            })
            .count()
            == 1;
        let exact_active_member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .filter(|member| {
                member.id == authority.source_agent_member_id
                    && member.organization_status == AgentMemberOrganizationStatus::Active
            })
            .count()
            == 1;
        let member_target = authority.target_membership_id.is_some()
            || authority.target_membership_generation.is_some()
            || authority.target_agent_member_id.is_some();
        let member_target_complete = authority.target_membership_id.is_some()
            && authority.target_membership_generation.is_some()
            && authority.target_agent_member_id.is_some();
        let exact_team_recipient = message.recipients.len() == 1
            && message.recipients[0].kind == MessageRecipientKind::Team
            && message.recipients[0].id == authority.target_team_id
            && message.target_ref == message.recipients[0];
        let exact_member_recipient = message.recipients.len() == 1
            && message.recipients[0].kind == MessageRecipientKind::AgentMember
            && Some(message.recipients[0].id.as_str())
                == authority.target_agent_member_id.as_deref()
            && message.target_ref == message.recipients[0];
        let exact_target_subscription = if member_target {
            member_target_complete
                && authority.target_membership_generation != Some(0)
                && authority.target_authorization_policy_ref == "team.direct.active-members"
                && authority.target_subscription_id
                    == format!(
                        "direct:{}:{}",
                        authority
                            .target_agent_member_id
                            .as_deref()
                            .unwrap_or_default(),
                        authority
                            .target_membership_id
                            .as_deref()
                            .unwrap_or_default()
                    )
                && exact_member_recipient
        } else {
            authority.target_authorization_policy_ref == "collaboration.peer_message_deliver"
                && authority.target_subscription_id
                    == format!("team-inbox:{}", authority.target_team_id)
                && exact_team_recipient
        };
        if authority.source_required_capability != "message.peer_team.author"
            || authority.target_required_capability != "collaboration.peer_message_deliver"
            || authority.target_subscription_revision == 0
            || authority.source_policy_revision == 0
            || authority.target_policy_revision == 0
            || authority.source_membership_generation == 0
            || authority.source_session_generation == 0
            || authority.source_node_daemon_generation == 0
            || authority.target_team_revision == 0
            || authority.source_policy_digest != expected_source_policy_digest
            || authority.target_policy_digest != expected_target_policy_digest
            || authority.authority_digest != expected_authority_digest
            || authority.source_execution_space_id != context.execution_space_id
            || authority.source_execution_space_id != message.source_execution_space_id
            || authority.source_team_revision != source_team.revision
            || source_team.status != AgentTeamStatus::Active
            || source_team.node_id != authority.source_node_id
            || message.source_node_id != authority.source_node_id
            || message.source_node_daemon_id != authority.source_node_daemon_id
            || message.source_authority_generation != authority.source_node_daemon_generation
            || message.sender_agent_member_id.as_deref()
                != Some(authority.source_agent_member_id.as_str())
            || message.sender_session_id.as_deref() != Some(authority.source_session_id.as_str())
            || message.sender_actor_ref.kind != ActorKind::AgentMember
            || message.sender_actor_ref.id != authority.source_agent_member_id
            || message.team_id.as_deref() != Some(authority.source_team_id.as_str())
            || scope.source_team_id != authority.source_team_id
            || scope.target_team_id != authority.target_team_id
            || scope.delegation_id.is_some()
            || scope.expected_delegation_revision.is_some()
            || scope.source_work_ref.is_some()
            || scope.target_work_ref.is_some()
            || !exact_target_subscription
            || !exact_membership
            || !exact_session
            || !exact_active_member
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team Message is outside the exact membership, session, daemon, policy, capability, or target placement authority",
                "message",
                &message.id,
                None,
            ));
        }
        // A Message Work link is context only (DOC-106): it cannot assign,
        // accept, close, or transfer Work. When present it must resolve to a
        // current Work accountable to the source Team so the author cannot
        // invent cross-Team provenance.
        if let Some(work_id) = message.work_id.as_deref() {
            let work_matches = self.latest_works()?.into_iter().any(|work| {
                work.id == work_id
                    && work.accountable_team_id.as_deref()
                        == Some(authority.source_team_id.as_str())
            });
            if !work_matches {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "peer-Team Message Work link must name a current Work of the source Team",
                    "message",
                    &message.id,
                    None,
                ));
            }
        }
        // When source and target share this Execution Space, the target fence
        // is revalidated against the same durable store at author time; a
        // genuinely remote target revalidates on its own Node before any
        // delivery mutation.
        if authority.target_execution_space_id == context.execution_space_id {
            self.revalidate_peer_team_delivery_subscription(
                &context.execution_space_id,
                authority,
            )?;
        }
        Ok(())
    }

    /// Revalidate the target half of a frozen peer-Team authority against the
    /// durable target subscription. A Team target revalidates the shared
    /// `team-inbox:` Team-subject subscription; a direct TeamMembership target
    /// revalidates that membership's durable `direct:` subscription plus the
    /// exact membership generation. This grants target delivery only;
    /// callers must separately prove source admission before route creation.
    pub fn revalidate_peer_team_delivery_subscription(
        &self,
        execution_space_id: &str,
        authority: &PeerTeamMessageAdmissionAuthority,
    ) -> StoreResult<MessageSubscription> {
        let member_target = authority.target_membership_id.is_some()
            || authority.target_membership_generation.is_some()
            || authority.target_agent_member_id.is_some();
        let expected_policy_ref = if member_target {
            "team.direct.active-members"
        } else {
            "collaboration.peer_message_deliver"
        };
        if authority.target_required_capability != "collaboration.peer_message_deliver"
            || authority.target_authorization_policy_ref != expected_policy_ref
            || authority.target_execution_space_id != execution_space_id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team authority does not carry the target delivery capability",
                "message_subscription",
                &authority.target_subscription_id,
                None,
            ));
        }
        let teams = self
            .agent_teams(execution_space_id)?
            .into_iter()
            .filter(|team| team.id == authority.target_team_id)
            .collect::<Vec<_>>();
        let subscriptions = self
            .fabric_message_subscriptions(execution_space_id)?
            .into_iter()
            .filter(|subscription| subscription.id == authority.target_subscription_id)
            .collect::<Vec<_>>();
        if teams.len() != 1 || subscriptions.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team target Team or durable subscription is missing or ambiguous",
                "message_subscription",
                &authority.target_subscription_id,
                None,
            ));
        }
        let team = &teams[0];
        let subscription = &subscriptions[0];
        let expected_policy_digest = peer_team_target_policy_digest(authority);
        let subscription_matches = if member_target {
            subscription.subscriber_kind == MessageSubjectKind::AgentMember
                && Some(subscription.subscriber_ref.as_str())
                    == authority.target_agent_member_id.as_deref()
                && subscription.membership_ref.as_deref()
                    == authority.target_membership_id.as_deref()
                && subscription.source_kind == MessageSubscriptionKind::Agent
                && subscription.source_ref == "active_team_members"
        } else {
            subscription.subscriber_kind == MessageSubjectKind::Team
                && subscription.subscriber_ref == authority.target_team_id
                && subscription.source_kind == MessageSubscriptionKind::AllAuthorized
                && subscription.source_ref == "authorized_peer_teams"
                && subscription.membership_ref.is_none()
        };
        if team.status != AgentTeamStatus::Active
            || team.node_id != authority.target_node_id
            || team.revision != authority.target_team_revision
            || !subscription_matches
            || subscription.target_team_id.as_deref() != Some(authority.target_team_id.as_str())
            || subscription.target_node_id != authority.target_node_id
            || subscription.status != MessageSubscriptionStatus::Active
            || subscription.revision != authority.target_subscription_revision
            || subscription.authorization_policy_ref != authority.target_authorization_policy_ref
            || subscription.policy_revision != authority.target_policy_revision
            || subscription.policy_digest != authority.target_policy_digest
            || authority.target_policy_digest != expected_policy_digest
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "peer-Team target delivery rejected stale or cross-wired Team/subscription policy authority",
                "message_subscription",
                &authority.target_subscription_id,
                Some(subscription.revision),
            ));
        }
        if member_target {
            let members = self
                .trust_agent_members(execution_space_id)?
                .into_iter()
                .filter(|member| {
                    Some(member.id.as_str()) == authority.target_agent_member_id.as_deref()
                        && member.organization_status == AgentMemberOrganizationStatus::Active
                })
                .count();
            let memberships = self
                .fabric_team_memberships(execution_space_id)?
                .into_iter()
                .filter(|membership| {
                    Some(membership.id.as_str()) == authority.target_membership_id.as_deref()
                        && membership.team_id == authority.target_team_id
                        && membership.node_id == authority.target_node_id
                        && Some(membership.agent_member_id.as_str())
                            == authority.target_agent_member_id.as_deref()
                        && Some(membership.membership_generation)
                            == authority.target_membership_generation
                        && membership.state == TeamMembershipStatus::Active
                })
                .count();
            if members != 1 || memberships != 1 {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "peer-Team direct delivery requires one exact active target TeamMembership generation",
                    "message_subscription",
                    &authority.target_subscription_id,
                    Some(subscription.revision),
                ));
            }
        }
        Ok(subscription.clone())
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
        let (reference, collaboration_authority, peer_authority) = match operation
            .closed_body()
            .map_err(|error| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    format!("Remote Message route is invalid: {error}"),
                    "message",
                    &message.id,
                    None,
                )
            })? {
            firm_fabric::ClosedOperationBody::Message(reference) => (reference, None, None),
            firm_fabric::ClosedOperationBody::CollaborationBusiness(reference)
                if reference.business_kind == "peer_message_deliver"
                    && reference.required_capability == "collaboration.peer_message_deliver" =>
            {
                let message_reference = serde_json::from_value::<firm_fabric::MessageReference>(
                    reference
                        .payload
                        .get("message_reference")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::InvalidStateTransition,
                                "peer_message_deliver lacks server-frozen message_reference",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        format!("peer_message_deliver payload is not a MessageReference: {error}"),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                let admission_authority = serde_json::from_value::<MessageAdmissionAuthority>(
                    reference
                        .payload
                        .get("message_admission_authority")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::UnauthorizedActor,
                                "peer_message_deliver lacks canonical Message admission authority",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        format!(
                            "peer_message_deliver Message admission authority is invalid: {error}"
                        ),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                let MessageAdmissionAuthority::PeerTeam(authority) = admission_authority else {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "peer_message_deliver requires PeerTeam admission authority",
                        "message",
                        &message.id,
                        None,
                    ));
                };
                (message_reference, None, Some(authority))
            }
            firm_fabric::ClosedOperationBody::CollaborationBusiness(reference)
                if reference.business_kind == "team_message_deliver"
                    && reference.required_capability == "collaboration.team_message_deliver" =>
            {
                let message_reference = serde_json::from_value::<firm_fabric::MessageReference>(
                    reference
                        .payload
                        .get("message_reference")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::InvalidStateTransition,
                                "team_message_deliver lacks server-frozen message_reference",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        format!("team_message_deliver payload is not a MessageReference: {error}"),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                let authority = serde_json::from_value::<CollaborationMessageAuthority>(
                    reference
                        .payload
                        .get("delegation_authority")
                        .cloned()
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::UnauthorizedActor,
                                "team_message_deliver lacks central Delegation authority",
                                "message",
                                &message.id,
                                None,
                            )
                        })?,
                )
                .map_err(|error| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        format!("team_message_deliver Delegation authority is invalid: {error}"),
                        "message",
                        &message.id,
                        None,
                    )
                })?;
                (message_reference, Some(authority), None)
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
        if let Some(authority) = collaboration_authority.as_ref() {
            let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "cross-Team Message lacks CollaborationScope",
                    "message",
                    &message.id,
                    None,
                )
            })?;
            let expected_authority_digest = canonical_json_fingerprint(&serde_json::json!({
                "company_id": authority.company_id,
                "delegation_id": authority.delegation_id,
                "delegation_revision": authority.delegation_revision,
                "source_work_ref": authority.source_work_ref,
                "target_work_ref": authority.target_work_ref,
                "target_placement": authority.target_placement,
                "source_owner_ref": authority.source_owner_ref,
                "source_host_ref": authority.source_host_ref,
                "target_host_ref": authority.target_host_ref,
                "inbound_policy_snapshot": authority.inbound_policy_snapshot,
            }));
            let target_work = self
                .latest_works()?
                .into_iter()
                .find(|work| work.id == authority.target_work_ref.work_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "Delegation target Work is not present on the target Node",
                        "message",
                        &message.id,
                        None,
                    )
                })?;
            let target_teams = self.teams()?;
            let target_team_revision = target_teams
                .iter()
                .filter(|team| team.id == authority.target_placement.team_id)
                .count() as u64;
            let target_team = target_teams
                .into_iter()
                .rev()
                .find(|team| team.id == authority.target_placement.team_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "Delegation target Team is not present on the target Node",
                        "message",
                        &message.id,
                        None,
                    )
                })?;
            if authority.authority_digest != expected_authority_digest
                || authority.delegation_revision == 0
                || scope.delegation_id.as_deref() != Some(authority.delegation_id.as_str())
                || scope.expected_delegation_revision != Some(authority.delegation_revision)
                || scope.source_work_ref.as_ref() != Some(&authority.source_work_ref)
                || scope.target_work_ref.as_ref() != Some(&authority.target_work_ref)
                || scope.source_team_id != authority.source_work_ref.team_id
                || scope.target_team_id != authority.target_placement.team_id
                || operation.expected_target_revision != Some(authority.delegation_revision)
                || operation.target_node_id != authority.target_placement.node_id
                || target_team.node_id != target_node_id
                || target_team_revision != authority.target_placement.team_revision
                || target_team.id != authority.target_work_ref.team_id
                || target_work.accountable_team_id.as_deref() != Some(target_team.id.as_str())
                || target_work.id != authority.target_work_ref.work_id
                || target_work.version != authority.target_work_ref.work_revision
            {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "target Message application rejected stale or widened Delegation/Work authority",
                    "message",
                    &message.id,
                    None,
                ));
            }
        } else if let Some(authority) = peer_authority.as_ref() {
            let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "peer-Team Message lacks CollaborationScope",
                    "message",
                    &message.id,
                    None,
                )
            })?;
            let exact_recipient = if authority.target_membership_id.is_some() {
                message.recipients.len() == 1
                    && message.recipients[0].kind == MessageRecipientKind::AgentMember
                    && Some(message.recipients[0].id.as_str())
                        == authority.target_agent_member_id.as_deref()
                    && message.target_ref == message.recipients[0]
            } else {
                message.recipients.len() == 1
                    && message.recipients[0].kind == MessageRecipientKind::Team
                    && message.recipients[0].id == authority.target_team_id
                    && message.target_ref == message.recipients[0]
            };
            if authority.authority_digest != peer_team_message_authority_digest(authority)
                || authority.source_policy_digest != peer_team_source_policy_digest(authority)
                || authority.target_policy_digest != peer_team_target_policy_digest(authority)
                || authority.company_id != operation.company_id
                || authority.source_required_capability != "message.peer_team.author"
                || authority.target_required_capability != "collaboration.peer_message_deliver"
                || authority.source_execution_space_id != message.source_execution_space_id
                || authority.source_team_id != scope.source_team_id
                || authority.source_team_revision == 0
                || authority.source_membership_generation == 0
                || authority.source_session_generation == 0
                || authority.source_agent_member_id != message.sender_actor_ref.id
                || message.sender_actor_ref.kind != ActorKind::AgentMember
                || message.sender_agent_member_id.as_deref()
                    != Some(authority.source_agent_member_id.as_str())
                || message.sender_session_id.as_deref()
                    != Some(authority.source_session_id.as_str())
                || message.team_id.as_deref() != Some(authority.source_team_id.as_str())
                || scope.target_team_id != authority.target_team_id
                || scope.delegation_id.is_some()
                || scope.expected_delegation_revision.is_some()
                || scope.source_work_ref.is_some()
                || scope.target_work_ref.is_some()
                || authority.source_node_id != message.source_node_id
                || authority.source_node_daemon_id != message.source_node_daemon_id
                || authority.source_node_daemon_generation != message.source_authority_generation
                || authority.target_execution_space_id != context.execution_space_id
                || authority.target_node_id != target_node_id
                || operation.actor.actor_kind != firm_fabric::ActorKind::Service
                || operation.actor.actor_id != authority.source_node_id
                || operation.actor.session_id
                    != format!(
                        "{}:{}",
                        authority.source_node_daemon_id, authority.source_node_daemon_generation
                    )
                || operation.source_gateway_generation.unwrap_or_default() == 0
                || operation
                    .authorization_context
                    .get("business_actor_kind")
                    .map(String::as_str)
                    != Some("agent_member")
                || operation.authorization_context.get("business_actor_id")
                    != Some(&authority.source_agent_member_id)
                || operation
                    .authorization_context
                    .get("business_actor_session_id")
                    != Some(&authority.source_session_id)
                || operation.actor_runtime_generation != Some(authority.source_session_generation)
                || operation.expected_target_revision
                    != Some(authority.target_subscription_revision)
                || operation.authorization_context.get("target_team_id")
                    != Some(&authority.target_team_id)
                || operation.authorization_context.get("target_team_revision")
                    != Some(&authority.target_team_revision.to_string())
                || operation.authorization_context.get("required_capability")
                    != Some(&authority.target_required_capability)
                || !exact_recipient
            {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "target Message application rejected widened, cross-wired, or stale peer-Team authority",
                    "message",
                    &message.id,
                    None,
                ));
            }
            self.revalidate_peer_team_delivery_subscription(
                &context.execution_space_id,
                authority,
            )?;
        } else if message.collaboration_scope.is_some() {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "cross-Team Message route requires server-frozen Message admission authority",
                "message",
                &message.id,
                None,
            ));
        }
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
        let memberships = self.fabric_team_memberships(&context.execution_space_id)?;
        let mut deliveries = if let Some(authority) = peer_authority.as_ref() {
            let subscription = subscriptions
                .iter()
                .find(|subscription| subscription.id == authority.target_subscription_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "revalidated peer-Team subscription disappeared before delivery creation",
                        "message_subscription",
                        &authority.target_subscription_id,
                        None,
                    )
                })?;
            // A Team target stays unresolved in the shared Team Inbox until one
            // exact membership generation claims it; a direct TeamMembership
            // target is bound at admission and needs no claim.
            let member_bound = authority.target_membership_id.is_some();
            vec![CanonicalMessageDelivery {
                id: format!("{}:{}", message.id, subscription.id),
                message_id: message.id.clone(),
                subscription_id: subscription.id.clone(),
                subscription_revision: subscription.revision,
                subscription_policy_digest: subscription.policy_digest.clone(),
                recipient_kind: if member_bound {
                    MessageSubjectKind::AgentMember
                } else {
                    MessageSubjectKind::Team
                },
                recipient_ref: if member_bound {
                    authority.target_agent_member_id.clone().unwrap_or_default()
                } else {
                    authority.target_team_id.clone()
                },
                target_team_id: Some(authority.target_team_id.clone()),
                target_node_id: target_node_id.into(),
                resolved_team_membership_id: authority.target_membership_id.clone(),
                recipient_agent_member_id: authority.target_agent_member_id.clone(),
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
            }]
        } else {
            Vec::new()
        };
        let mut delivered_subjects = BTreeSet::new();
        let routed_recipients = if peer_authority.is_none() {
            message.recipients.as_slice()
        } else {
            &[]
        };
        for recipient in routed_recipients {
            for subscription in subscriptions.iter().filter(|subscription| {
                subscription.status == MessageSubscriptionStatus::Active
                    && subscription.target_node_id == target_node_id
                    && match recipient.kind {
                        MessageRecipientKind::AgentMember => {
                            subscription.subscriber_kind == MessageSubjectKind::AgentMember
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind == MessageSubscriptionKind::Agent
                        }
                        MessageRecipientKind::Team => {
                            subscription.subscriber_kind == MessageSubjectKind::Team
                                && subscription.subscriber_ref == recipient.id
                                && subscription.source_kind
                                    == MessageSubscriptionKind::AllAuthorized
                                && subscription.source_ref == "authorized_peer_teams"
                                && subscription.target_team_id.as_deref()
                                    == Some(recipient.id.as_str())
                        }
                        MessageRecipientKind::ControlPlaneActor => false,
                    }
            }) {
                let subject_key = (
                    subscription.subscriber_kind,
                    subscription.subscriber_ref.clone(),
                );
                if !delivered_subjects.insert(subject_key) {
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
                    match subscription.subscriber_kind {
                        MessageSubjectKind::AgentMember => {
                            let exact_membership = subscription
                                .membership_ref
                                .as_deref()
                                .is_some_and(|membership_id| {
                                    memberships.iter().any(|membership| {
                                        membership.id == membership_id
                                            && membership.team_id == team_id
                                            && membership.agent_member_id
                                                == subscription.subscriber_ref
                                            && membership.node_id == target_node_id
                                            && membership.state == TeamMembershipStatus::Active
                                    })
                                });
                            if !exact_membership {
                                continue;
                            }
                        }
                        MessageSubjectKind::Team => {
                            if subscription.target_team_id.as_deref() != Some(team_id) {
                                continue;
                            }
                        }
                    }
                }
                deliveries.push(CanonicalMessageDelivery {
                    id: format!("{}:{}", message.id, subscription.id),
                    message_id: message.id.clone(),
                    subscription_id: subscription.id.clone(),
                    subscription_revision: subscription.revision,
                    subscription_policy_digest: subscription.policy_digest.clone(),
                    recipient_kind: subscription.subscriber_kind,
                    recipient_ref: subscription.subscriber_ref.clone(),
                    target_team_id: subscription.target_team_id.clone(),
                    target_node_id: target_node_id.into(),
                    resolved_team_membership_id: (subscription.subscriber_kind
                        == MessageSubjectKind::AgentMember)
                        .then(|| subscription.membership_ref.clone())
                        .flatten(),
                    recipient_agent_member_id: (subscription.subscriber_kind
                        == MessageSubjectKind::AgentMember)
                        .then(|| subscription.subscriber_ref.clone()),
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

    /// Resolve one Team-subject delivery to one exact active membership
    /// generation. Admission intentionally has no AgentSession dependency;
    /// provider dispatch may bind the resolved member's current session later.
    pub fn claim_team_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: &TeamMessageDeliveryClaim,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(delivery_id, "delivery_id")?;
        required(&claim.claim_id, "TeamMessageDeliveryClaim.claim_id")?;
        required(
            &claim.team_membership_id,
            "TeamMessageDeliveryClaim.team_membership_id",
        )?;
        if context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "Team delivery claim is a new idempotent routing operation",
                "team_message_delivery_claim",
                &claim.claim_id,
                Some(0),
            ));
        }
        let request_payload = serde_json::json!({
            "delivery_id": delivery_id,
            "claim": claim,
            "updated_at": updated_at,
        });
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "team_message_delivery_claim",
            &claim.claim_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Team MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        let team_id = delivery.target_team_id.clone().ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team-subject delivery is missing target_team_id",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            )
        })?;
        if delivery.recipient_kind != MessageSubjectKind::Team
            || delivery.recipient_ref != team_id
            || delivery.status != CanonicalMessageDeliveryStatus::Queued
            || delivery.resolved_team_membership_id.is_some()
            || delivery.recipient_agent_member_id.is_some()
            || delivery.recipient_session_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only one unresolved queued Team-subject delivery may be membership-claimed",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let current_subscriptions = self
            .fabric_message_subscriptions(&context.execution_space_id)?
            .into_iter()
            .filter(|subscription| subscription.id == delivery.subscription_id)
            .collect::<Vec<_>>();
        if current_subscriptions.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team delivery subscription is missing or ambiguous",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let subscription = &current_subscriptions[0];
        if subscription.subscriber_kind != MessageSubjectKind::Team
            || subscription.subscriber_ref != team_id
            || subscription.target_team_id.as_deref() != Some(team_id.as_str())
            || subscription.target_node_id != delivery.target_node_id
            || subscription.source_kind != MessageSubscriptionKind::AllAuthorized
            || subscription.source_ref != "authorized_peer_teams"
            || subscription.authorization_policy_ref != "collaboration.peer_message_deliver"
            || subscription.status != MessageSubscriptionStatus::Active
            || subscription.revision != delivery.subscription_revision
            || subscription.policy_digest != delivery.subscription_policy_digest
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team delivery claim requires the exact active durable subscription generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &delivery.target_node_id,
            &context.authenticated_actor.id,
            claim.node_daemon_generation,
            &context.authenticated_actor,
            "message_delivery",
            delivery_id,
        )?;
        let team = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .find(|team| team.id == team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Team-subject delivery references a missing AgentTeam",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        if team.status != AgentTeamStatus::Active || team.node_id != delivery.target_node_id {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team-subject delivery requires the exact Active Team placement",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let active_members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .filter(|member| member.organization_status == AgentMemberOrganizationStatus::Active)
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        let eligible_memberships = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team.id
                    && membership.node_id == team.node_id
                    && membership.state == TeamMembershipStatus::Active
                    && membership.role != TeamMembershipRole::Observer
                    && active_members.contains_key(&membership.agent_member_id)
            })
            .collect::<Vec<_>>();
        if eligible_memberships.len() != 1
            || eligible_memberships[0].id != claim.team_membership_id
            || eligible_memberships[0].membership_generation != claim.membership_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team delivery remains queued unless exactly one eligible active Host/Member membership generation exists and matches the claim",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let membership = eligible_memberships.into_iter().next().ok_or_else(|| {
            trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team delivery has no eligible TeamMembership",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            )
        })?;
        let member = active_members
            .get(&membership.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "claimed TeamMembership references no active AgentMember",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        delivery.status = CanonicalMessageDeliveryStatus::Routed;
        delivery.resolved_team_membership_id = Some(membership.id);
        delivery.recipient_agent_member_id = Some(member.id.clone());
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_node_daemon_generation = Some(claim.node_daemon_generation);
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "team_message_delivery_claim",
            &claim.claim_id,
            "team_subject_resolved",
            request_payload,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
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
            || !matches!(
                delivery.status,
                CanonicalMessageDeliveryStatus::Queued | CanonicalMessageDeliveryStatus::Routed
            )
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "only the target NodeDaemon can claim a queued MessageDelivery",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        if delivery.recipient_kind == MessageSubjectKind::Team
            && (delivery.status != CanonicalMessageDeliveryStatus::Routed
                || delivery.claim_id.as_deref() != Some(claim_id)
                || delivery.claimed_node_daemon_generation != Some(daemon_generation)
                || delivery.resolved_team_membership_id.is_none())
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team-subject delivery must first be resolved by the exact membership-generation claim",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let recipient_agent_member_id =
            delivery
                .recipient_agent_member_id
                .as_deref()
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "MessageDelivery has no resolved AgentMember",
                        "message_delivery",
                        delivery_id,
                        Some(delivery.version),
                    )
                })?;
        let current = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_member_id == recipient_agent_member_id
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
        let invocation_binding = runtime_binding_for_session(session);
        self.require_live_runtime_binding_unlocked(
            session,
            &invocation_binding,
            false,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
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
            recipient_agent_member_id: recipient_agent_member_id.to_string(),
            recipient_session_id: session.id.clone(),
            recipient_session_generation: session.runtime_generation,
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
            provider: session.provider_kind.clone(),
            dispatch_mode,
            binding: invocation_binding,
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
            || delivery.recipient_agent_member_id.as_deref()
                != Some(context.authenticated_actor.id.as_str())
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
            recipient_agent_member_id: delivery
                .recipient_agent_member_id
                .clone()
                .expect("recipient checked above"),
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
        if command.postcondition.status != RuntimePostconditionStatus::Unknown {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "a RuntimeCommand may request a postcondition but cannot claim it satisfied before provider observation",
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
        let expected_capability = runtime_command_capability(command.command);
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
            if runtime_command_requires_exact_binding(command.command) {
                self.require_live_runtime_binding_unlocked(
                    &session,
                    &command.binding,
                    false,
                    "runtime_command",
                    &command.id,
                    Some(session.version),
                )?;
            }
            Self::require_runtime_command_precondition_unlocked(
                &session,
                command.command,
                &command.precondition,
                false,
                "runtime_command",
                &command.id,
                Some(session.version),
            )?;
            let actor = &command.authenticated_actor;
            let exact_self =
                actor.kind == ActorKind::AgentMember && actor.id == session.agent_member_id;
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
                    .find(|identity| identity.id == requested.agent_member_id)
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
                RuntimeCommandKind::DispatchProvider
                | RuntimeCommandKind::StartCycle
                | RuntimeCommandKind::InjectCurrentCycle
                | RuntimeCommandKind::QueueAtNativeBoundary => {
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
                RuntimeCommandKind::CancelProviderTurn
                | RuntimeCommandKind::InterruptCurrentCycle
                | RuntimeCommandKind::CancelPendingInput => {
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
                RuntimeCommandKind::ReleaseRuntime
                | RuntimeCommandKind::CloseMember
                | RuntimeCommandKind::QuiesceExecutionLane
                | RuntimeCommandKind::DrainRuntime
                | RuntimeCommandKind::InhibitContinuation => {
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
                }
                RuntimeCommandKind::StartSession
                | RuntimeCommandKind::ResumeSession
                | RuntimeCommandKind::OpenRuntime
                | RuntimeCommandKind::ResumeNativeSession
                | RuntimeCommandKind::ReopenMember
                | RuntimeCommandKind::ReattachLiveRuntime => {
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
                RuntimeCommandKind::RetireMember
                | RuntimeCommandKind::DeleteNativeSession
                | RuntimeCommandKind::InspectContinuation
                | RuntimeCommandKind::ActivateContinuation
                | RuntimeCommandKind::ResumeContinuation
                | RuntimeCommandKind::ReplaceContinuationCondition
                | RuntimeCommandKind::ClearContinuation
                | RuntimeCommandKind::StopBackgroundTask
                | RuntimeCommandKind::TransferExecutionDriver
                | RuntimeCommandKind::InspectCommandEffect
                | RuntimeCommandKind::ReconcileUnknownEffect
                | RuntimeCommandKind::AbortIfNotApplied
                | RuntimeCommandKind::AuthorMessage => {}
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
                                    | RuntimeCommandKind::InterruptCurrentCycle
                                    | RuntimeCommandKind::StopSession,
                                RuntimeCommandKind::DispatchProvider
                                    | RuntimeCommandKind::StartCycle
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
            request_fingerprint: command_fingerprint,
            status: RuntimeCommandStatus::Accepted,
            phase: RuntimeCommandPhase::Prepared,
            effect_certainty: RuntimeEffectCertainty::Unknown,
            postcondition_status: RuntimePostconditionStatus::Unknown,
            binding: command.binding.clone(),
            precondition: command.precondition.clone(),
            postcondition: command.postcondition.clone(),
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
                if runtime_command_requires_exact_binding(record.command) {
                    let session_id = record.target_session_id.as_deref().ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::MemberRunGenerationFenced,
                            "provider-facing RuntimeCommand recovery has no exact target AgentSession",
                            "runtime_command",
                            command_id,
                            Some(record.version),
                        )
                    })?;
                    let session = self
                        .fabric_agent_sessions(&context.execution_space_id)?
                        .into_iter()
                        .find(|session| session.id == session_id)
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::MemberRunGenerationFenced,
                                "RuntimeCommand target AgentSession disappeared before recovery resolution",
                                "runtime_command",
                                command_id,
                                Some(record.version),
                            )
                        })?;
                    if record.target_session_generation != Some(session.runtime_generation)
                        || session.node_id != record.target_node_id
                        || session.node_daemon_id != record.target_node_daemon_id
                        || session.node_daemon_generation != record.target_node_daemon_generation
                    {
                        return Err(trust_error(
                            TrustErrorCode::MemberRunGenerationFenced,
                            "RuntimeCommand recovery no longer owns the exact AgentSession/NodeDaemon generation",
                            "runtime_command",
                            command_id,
                            Some(record.version),
                        ));
                    }
                    self.require_live_runtime_binding_unlocked(
                        &session,
                        &record.binding,
                        matches!(
                            record.command,
                            RuntimeCommandKind::StartSession | RuntimeCommandKind::OpenRuntime
                        ),
                        "runtime_command",
                        command_id,
                        Some(record.version),
                    )?;
                    Self::require_runtime_command_precondition_unlocked(
                        &session,
                        record.command,
                        &record.precondition,
                        true,
                        "runtime_command",
                        command_id,
                        Some(record.version),
                    )?;
                }
                record.status = RuntimeCommandStatus::Applied;
                record.phase = RuntimeCommandPhase::Settled;
                record.effect_certainty = RuntimeEffectCertainty::Applied;
                record.postcondition_status = RuntimePostconditionStatus::Unknown;
                record.failure_code = None;
            }
            RuntimeRecoveryResolution::ConfirmNotApplied => {
                record.status = RuntimeCommandStatus::Failed;
                record.phase = RuntimeCommandPhase::Rejected;
                record.effect_certainty = RuntimeEffectCertainty::NotApplied;
                record.postcondition_status = RuntimePostconditionStatus::Unsatisfied;
                record.failure_code = Some("RECOVERY_CONFIRMED_NOT_APPLIED".into());
            }
            RuntimeRecoveryResolution::KeepRecoveryRequired => {
                record.phase = RuntimeCommandPhase::RecoveryRequired;
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
        self.settle_runtime_command_with_postcondition(
            context,
            command_id,
            status,
            effect_certainty,
            RuntimePostconditionStatus::Unknown,
            result,
            failure_code,
            now,
        )
    }

    /// Settle a provider effect and, when the adapter has separately observed
    /// it, the semantic postcondition requested by the durable command.
    /// Keeping this explicit prevents a transport ACK from being silently
    /// promoted to proof of quiescence, release, or cycle termination.
    #[allow(clippy::too_many_arguments)]
    pub fn settle_runtime_command_with_postcondition(
        &self,
        context: &MutationContext,
        command_id: &str,
        status: RuntimeCommandStatus,
        effect_certainty: RuntimeEffectCertainty,
        postcondition_status: RuntimePostconditionStatus,
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
        if runtime_command_requires_exact_binding(record.command) {
            let session_id = record.target_session_id.as_deref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "provider-facing RuntimeCommand has no exact target AgentSession binding",
                    "runtime_command",
                    command_id,
                    Some(record.version),
                )
            })?;
            let session_generation = record.target_session_generation.ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "provider-facing RuntimeCommand has no exact target runtime generation",
                    "runtime_command",
                    command_id,
                    Some(record.version),
                )
            })?;
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
            self.require_live_runtime_binding_unlocked(
                &session,
                &record.binding,
                matches!(
                    record.command,
                    RuntimeCommandKind::StartSession | RuntimeCommandKind::OpenRuntime
                ),
                "runtime_command",
                command_id,
                Some(record.version),
            )?;
            Self::require_runtime_command_precondition_unlocked(
                &session,
                record.command,
                &record.precondition,
                true,
                "runtime_command",
                command_id,
                Some(record.version),
            )?;
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
        let postcondition_combination_is_valid = match postcondition_status {
            RuntimePostconditionStatus::Satisfied => {
                status == RuntimeCommandStatus::Applied
                    && effect_certainty == RuntimeEffectCertainty::Applied
                    && result.is_some()
            }
            RuntimePostconditionStatus::Unsatisfied => {
                status == RuntimeCommandStatus::Failed
                    && effect_certainty == RuntimeEffectCertainty::NotApplied
            }
            RuntimePostconditionStatus::Unknown => true,
        };
        if !postcondition_combination_is_valid {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "RuntimeCommand postcondition status is not proven by this settlement",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        record.status = status;
        record.phase = match status {
            RuntimeCommandStatus::Applied => RuntimeCommandPhase::Settled,
            RuntimeCommandStatus::Failed => RuntimeCommandPhase::Rejected,
            RuntimeCommandStatus::Quiesced => RuntimeCommandPhase::Observed,
            RuntimeCommandStatus::RecoveryRequired => RuntimeCommandPhase::RecoveryRequired,
            RuntimeCommandStatus::Requested | RuntimeCommandStatus::Accepted => {
                RuntimeCommandPhase::Prepared
            }
        };
        record.effect_certainty = effect_certainty;
        record.postcondition_status = postcondition_status;
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static FABRIC_STORE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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
            agent_member_id: identity_id.into(),
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
            control_state: firm_core::agentfirm_api::AgentSessionControlState {
                driver_generation: 1,
                driver_ref: firm_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
                    node_daemon_id: "daemon-1".into(),
                    node_daemon_generation: 1,
                },
                composition_fingerprint: Some("composition:test".into()),
                capability_fingerprint: Some("capability:test".into()),
                ..Default::default()
            },
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
        let required_capability = runtime_command_capability(kind);
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
            binding: firm_core::agentfirm_api::RuntimeCommandBinding {
                target_session_id: Some(session.id.clone()),
                target_runtime_generation: Some(session.runtime_generation),
                target_driver_generation: Some(session.control_state.driver_generation),
                target_driver: session.control_state.driver_ref.clone(),
                native_session_ref: session.native_session_ref.clone(),
                composition_fingerprint: session.control_state.composition_fingerprint.clone(),
                capability_fingerprint: session.control_state.capability_fingerprint.clone(),
                permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
                ..Default::default()
            },
            precondition: Default::default(),
            postcondition: Default::default(),
            payload_fingerprint: canonical_json_fingerprint(&payload),
            payload,
            issued_at: "t-command".into(),
        };
        let mut context = service_context("node_daemon.runtime.prepare", id, 0);
        context.authority_actor = Some(command.authenticated_actor.clone());
        context.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
        (command, context)
    }

    fn test_runtime_binding(session_id: &str) -> firm_core::agentfirm_api::RuntimeCommandBinding {
        firm_core::agentfirm_api::RuntimeCommandBinding {
            target_session_id: Some(session_id.to_string()),
            target_runtime_generation: Some(1),
            target_driver_generation: Some(1),
            target_driver: firm_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
                node_daemon_id: "daemon-1".into(),
                node_daemon_generation: 1,
            },
            composition_fingerprint: Some("composition:test".into()),
            capability_fingerprint: Some("capability:test".into()),
            permission_envelope_ref: Some("permission-default".into()),
            ..Default::default()
        }
    }

    fn fabric_store() -> (HarnessStore, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "firm-runtime-fabric-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            FABRIC_STORE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
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
            agent_member_id: "membership-agent".into(),
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
        if !store.teams().unwrap().iter().any(|team| team.id == team_id) {
            let mission_id = format!("mission-{team_id}");
            if !store
                .trust_agent_members("space-test")
                .unwrap()
                .iter()
                .any(|member| member.id == "fixture-host")
            {
                store
                    .create_trust_agent_member(
                        &context("host", "agent_member.create", "fixture-host", 0),
                        member("fixture-host"),
                    )
                    .unwrap();
            }
            if !store
                .latest_missions()
                .unwrap()
                .iter()
                .any(|mission| mission.id == mission_id)
            {
                store
                    .append_mission(&firm_core::Mission {
                        id: mission_id.clone(),
                        title: mission_id.clone(),
                        objective: "runtime authority fixture".into(),
                        context: String::new(),
                        desired_outcome: None,
                        status: firm_core::MissionStatus::Running,
                        legacy_wave_ids: Vec::new(),
                        outcome_summary: None,
                        completed_by: None,
                        created_at: "t1".into(),
                        updated_at: "t1".into(),
                        completed_at: None,
                    })
                    .unwrap();
            }
            let existing_members = store.trust_agent_members("space-test").unwrap();
            let preferred_host = if team_id == "source-team"
                && existing_members
                    .iter()
                    .any(|member| member.id == "remote-sender")
            {
                "remote-sender".to_string()
            } else {
                let suffix_host = team_id
                    .strip_prefix("team-")
                    .map(|suffix| format!("host-{suffix}"));
                suffix_host
                    .filter(|candidate| {
                        existing_members
                            .iter()
                            .any(|member| member.id == *candidate)
                    })
                    .unwrap_or_else(|| "fixture-host".into())
            };
            let team = firm_core::AgentTeam {
                id: team_id.into(),
                name: team_id.into(),
                description: "runtime authority fixture".into(),
                legacy_mission_id: Some(mission_id.clone()),
                mission_id,
                host_agent_id: preferred_host.clone(),
                node_id: "11111111-1111-4111-8111-111111111111".into(),
                status: firm_core::AgentTeamStatus::Active,
                revision: 1,
                trashed_at: None,
                member_ids: Vec::new(),
                created_at: "t1".into(),
                updated_at: "t1".into(),
            };
            store
                .create_agent_team(
                    &context(
                        "fixture-host",
                        "agent_team.create",
                        &format!("team-{team_id}"),
                        0,
                    ),
                    team,
                    vec![TeamMembership {
                        id: format!("membership:{team_id}:{preferred_host}"),
                        team_id: team_id.into(),
                        agent_member_id: preferred_host,
                        node_id: "11111111-1111-4111-8111-111111111111".into(),
                        role: TeamMembershipRole::Host,
                        state: TeamMembershipStatus::Active,
                        membership_generation: 1,
                        default_subscription_refs: Vec::new(),
                        created_by: actor("fixture-host"),
                        revision: 1,
                        joined_at: "t1".into(),
                        left_at: None,
                    }],
                )
                .unwrap();
        }
        store
            .legacy_import_append_team_run_projection(&firm_core::AgentTeamRun {
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
            agent_member_id: identity_id.into(),
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
                    accountable_team_id: Some(team_id.into()),
                    assignee_membership_id: None,
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
        append_runtime_team(store, "team-membership-test", "team-run-membership-test");
        store
            .migrate_legacy_agent_identity_same_id(
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
            .filter(|membership| {
                membership.state == TeamMembershipStatus::Active
                    && membership.agent_member_id == "membership-agent"
            })
            .collect::<Vec<_>>();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].membership_generation, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn peer_team_authority_keeps_source_and_target_fences_distinct_then_claims_one_membership() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("operator", "identity.migrate", "peer-sender", 0),
                identity("remote-sender"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "peer-sender-session", 0),
                session("session-peer-sender", "remote-sender"),
            )
            .unwrap();
        append_runtime_team(&store, "source-team", "source-peer-run");
        append_runtime_team(&store, "target-team", "target-peer-run");
        let source_team = store
            .agent_teams("space-test")
            .unwrap()
            .into_iter()
            .find(|team| team.id == "source-team")
            .unwrap();
        let target_team = store
            .agent_teams("space-test")
            .unwrap()
            .into_iter()
            .find(|team| team.id == "target-team")
            .unwrap();
        let source_membership = store
            .team_host_membership("space-test", "source-team", true)
            .unwrap();
        let target_membership = store
            .team_host_membership("space-test", "target-team", true)
            .unwrap();
        let target_subscription = store
            .fabric_message_subscriptions("space-test")
            .unwrap()
            .into_iter()
            .find(|subscription| subscription.id == "team-inbox:target-team")
            .unwrap();
        let source_policy_ref = "peer-team-message-admission.v1".to_string();
        let source_required_capability = "message.peer_team.author".to_string();
        let mut peer = PeerTeamMessageAdmissionAuthority {
            company_id: "company-test".into(),
            source_execution_space_id: "space-test".into(),
            source_team_id: source_team.id.clone(),
            source_team_revision: source_team.revision,
            source_membership_id: source_membership.id.clone(),
            source_membership_generation: source_membership.membership_generation,
            source_agent_member_id: "remote-sender".into(),
            source_session_id: "session-peer-sender".into(),
            source_session_generation: 1,
            source_node_id: source_team.node_id.clone(),
            source_node_daemon_id: "daemon-1".into(),
            source_node_daemon_generation: 1,
            target_execution_space_id: "space-test".into(),
            target_team_id: target_team.id.clone(),
            target_team_revision: target_team.revision,
            target_node_id: target_team.node_id.clone(),
            target_membership_id: None,
            target_membership_generation: None,
            target_agent_member_id: None,
            source_policy_ref,
            source_policy_revision: 1,
            source_policy_digest: String::new(),
            source_required_capability,
            target_subscription_id: target_subscription.id.clone(),
            target_subscription_revision: target_subscription.revision,
            target_authorization_policy_ref: target_subscription.authorization_policy_ref.clone(),
            target_policy_revision: target_subscription.policy_revision,
            target_policy_digest: String::new(),
            target_required_capability: "collaboration.peer_message_deliver".into(),
            authority_digest: String::new(),
        };
        peer.source_policy_digest = peer_team_source_policy_digest(&peer);
        peer.target_policy_digest = peer_team_target_policy_digest(&peer);
        assert_eq!(
            peer.target_policy_digest, target_subscription.policy_digest,
            "the frozen target policy digest is byte-equal to the durable subscription digest"
        );
        peer.authority_digest = peer_team_message_authority_digest(&peer);
        store
            .revalidate_peer_team_delivery_subscription("space-test", &peer)
            .expect("target durable subscription independently revalidates");

        let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::Team,
            id: "target-team".into(),
        }];
        let mut message = Message {
            id: "peer-team-message".into(),
            source_execution_space_id: "space-test".into(),
            source_node_id: source_team.node_id,
            source_node_daemon_id: "daemon-1".into(),
            source_authority_generation: 1,
            sender_actor_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "remote-sender".into(),
            },
            sender_agent_member_id: Some("remote-sender".into()),
            sender_session_id: Some("session-peer-sender".into()),
            address_kind: firm_core::agentfirm_api::MessageAddressKind::TeamChannel,
            target_ref: recipients[0].clone(),
            recipients,
            team_id: Some("source-team".into()),
            team_run_id: None,
            work_id: None,
            collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
                source_team_id: "source-team".into(),
                target_team_id: "target-team".into(),
                delegation_id: None,
                expected_delegation_revision: None,
                source_work_ref: None,
                target_work_ref: None,
            }),
            kind: firm_core::agentfirm_api::MessageKind::Message,
            body: "ordinary peer conversation".into(),
            body_digest: format!("sha256:{:x}", Sha256::digest(b"ordinary peer conversation")),
            correlation_id: "peer-correlation".into(),
            causation_id: None,
            response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
            evidence_refs: Vec::new(),
            content_fingerprint: String::new(),
            schema_version: 1,
            idempotency_key: "peer-team-message".into(),
            created_at: "t-peer".into(),
        };
        message.content_fingerprint = message_content_fingerprint(&message);
        let before_hostile = store.canonical_operations().unwrap();
        let mut cross_wired = peer.clone();
        cross_wired.source_required_capability = "collaboration.peer_message_deliver".into();
        cross_wired.target_required_capability = "message.peer_team.author".into();
        cross_wired.authority_digest = peer_team_message_authority_digest(&cross_wired);
        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-hostile", 0),
                message.clone(),
                Some(&MessageAdmissionAuthority::PeerTeam(cross_wired)),
            )
            .expect_err("source and target capabilities cannot be cross-wired");
        assert_eq!(store.canonical_operations().unwrap(), before_hostile);

        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-team-message", 0),
                message.clone(),
                Some(&MessageAdmissionAuthority::PeerTeam(peer.clone())),
            )
            .unwrap();
        let delivery = store
            .fabric_message_deliveries("space-test")
            .unwrap()
            .into_iter()
            .find(|delivery| delivery.recipient_kind == MessageSubjectKind::Team)
            .unwrap();
        assert_eq!(delivery.recipient_agent_member_id, None);
        assert_eq!(delivery.recipient_session_id, None);
        assert_eq!(
            delivery.subscription_revision,
            peer.target_subscription_revision
        );
        let mut remote_message = message.clone();
        remote_message.id = "peer-team-message-remote".into();
        remote_message.idempotency_key = "peer-team-message-remote".into();
        remote_message.created_at = "t-peer-remote".into();
        remote_message.content_fingerprint = message_content_fingerprint(&remote_message);
        let make_peer_operation =
            |message: &Message, authority: &PeerTeamMessageAdmissionAuthority| {
                let message_reference = firm_fabric::MessageReference {
                    message_id: message.id.clone(),
                    body_digest: message.body_digest.clone(),
                    canonical_message_envelope: Some(serde_json::to_value(message).unwrap()),
                    message_object_ref: None,
                };
                let payload = serde_json::json!({
                    "message_reference": message_reference,
                    "message_admission_authority": MessageAdmissionAuthority::PeerTeam(authority.clone()),
                });
                let body = serde_json::to_value(firm_fabric::CollaborationBusinessReference {
                    business_kind: "peer_message_deliver".into(),
                    required_capability: "collaboration.peer_message_deliver".into(),
                    business_actor_kind: "agent_member".into(),
                    business_actor_id: authority.source_agent_member_id.clone(),
                    target_team_id: authority.target_team_id.clone(),
                    target_team_revision: authority.target_team_revision,
                    placement_generation: 1,
                    expected_revision: authority.target_subscription_revision,
                    payload_digest: canonical_json_fingerprint(&payload),
                    payload,
                })
                .unwrap();
                firm_fabric::RoutedOperation {
                    id: format!("peer-route:{}", message.id),
                    company_id: authority.company_id.clone(),
                    kind: firm_fabric::COLLABORATION_BUSINESS_OPERATION_KIND.into(),
                    source_authority: firm_fabric::OperationSourceAuthority::Node,
                    source_node_id: Some(authority.source_node_id.clone()),
                    target_node_id: authority.target_node_id.clone(),
                    source_gateway_generation: Some(1),
                    source_node_daemon_id: Some(authority.source_node_daemon_id.clone()),
                    source_node_daemon_generation: Some(authority.source_node_daemon_generation),
                    control_plane_generation: 1,
                    source_execution_space_id: Some(authority.source_execution_space_id.clone()),
                    target_execution_space_id: Some(authority.target_execution_space_id.clone()),
                    actor: firm_fabric::AuthenticatedActor {
                        company_id: authority.company_id.clone(),
                        actor_id: authority.source_node_id.clone(),
                        actor_kind: firm_fabric::ActorKind::Service,
                        role_bindings: BTreeSet::from(["fabric_submit".into()]),
                        session_id: format!(
                            "{}:{}",
                            authority.source_node_daemon_id,
                            authority.source_node_daemon_generation
                        ),
                        issued_at_unix_ms: 1,
                        expires_at_unix_ms: 90_000,
                    },
                    actor_runtime_generation: Some(authority.source_session_generation),
                    authorization_context: BTreeMap::from([
                        ("target_team_id".into(), authority.target_team_id.clone()),
                        (
                            "target_team_revision".into(),
                            authority.target_team_revision.to_string(),
                        ),
                        ("placement_generation".into(), "1".into()),
                        (
                            "required_capability".into(),
                            "collaboration.peer_message_deliver".into(),
                        ),
                        ("business_actor_kind".into(), "agent_member".into()),
                        (
                            "business_actor_id".into(),
                            authority.source_agent_member_id.clone(),
                        ),
                        (
                            "business_actor_session_id".into(),
                            authority.source_session_id.clone(),
                        ),
                    ]),
                    idempotency_key: format!("peer-route:{}", message.id),
                    ordering_key: format!("team:{}", authority.target_team_id),
                    correlation_id: message.correlation_id.clone(),
                    causation_id: None,
                    expected_target_revision: Some(authority.target_subscription_revision),
                    body_schema: firm_fabric::COLLABORATION_BUSINESS_OPERATION_SCHEMA.into(),
                    body_digest: firm_fabric::json_digest(&body).unwrap(),
                    body,
                    priority: firm_fabric::OperationPriority::Normal,
                    created_at_unix_ms: 2,
                    expires_at_unix_ms: 90_000,
                    protocol_version: firm_fabric::FABRIC_PROTOCOL_VERSION,
                    schema_version: firm_fabric::FABRIC_SCHEMA_VERSION.into(),
                    canonicalization_version: firm_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
                }
            };
        let mut target_cross_wired = peer.clone();
        target_cross_wired.source_required_capability = "collaboration.peer_message_deliver".into();
        target_cross_wired.target_required_capability = "message.peer_team.author".into();
        target_cross_wired.source_policy_digest =
            peer_team_source_policy_digest(&target_cross_wired);
        target_cross_wired.target_policy_digest =
            peer_team_target_policy_digest(&target_cross_wired);
        target_cross_wired.authority_digest =
            peer_team_message_authority_digest(&target_cross_wired);
        let hostile_operation = make_peer_operation(&remote_message, &target_cross_wired);
        let hostile_context = service_context("remote_message_persist", &hostile_operation.id, 0);
        let before_target_hostile = store.canonical_operations().unwrap();
        store
            .persist_remote_message(
                &hostile_context,
                &hostile_operation,
                remote_message.clone(),
                &peer.target_node_id,
                "daemon-1",
                1,
            )
            .expect_err("target persistence cannot cross-wire source and target capabilities");
        assert_eq!(store.canonical_operations().unwrap(), before_target_hostile);

        let peer_operation = make_peer_operation(&remote_message, &peer);
        let mut peer_context = service_context("remote_message_persist", &peer_operation.id, 0);
        peer_context.request_fingerprint = Some(firm_fabric::json_digest(&peer_operation).unwrap());
        store
            .persist_remote_message(
                &peer_context,
                &peer_operation,
                remote_message.clone(),
                &peer.target_node_id,
                "daemon-1",
                1,
            )
            .expect("target persists one unresolved canonical Team delivery");
        let remote_deliveries = store
            .fabric_message_deliveries("space-test")
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.message_id == remote_message.id)
            .collect::<Vec<_>>();
        assert_eq!(remote_deliveries.len(), 1);
        assert_eq!(
            remote_deliveries[0].recipient_kind,
            MessageSubjectKind::Team
        );
        assert_eq!(remote_deliveries[0].recipient_ref, peer.target_team_id);
        assert_eq!(remote_deliveries[0].resolved_team_membership_id, None);
        assert_eq!(remote_deliveries[0].recipient_agent_member_id, None);
        assert_eq!(remote_deliveries[0].recipient_session_id, None);
        assert_eq!(
            remote_deliveries[0].subscription_revision,
            peer.target_subscription_revision
        );
        store
            .migrate_legacy_agent_identity_same_id(
                &context("operator", "identity.migrate", "peer-extra", 0),
                identity("peer-extra"),
            )
            .unwrap();
        let extra_membership = TeamMembership {
            id: "target-team-extra-membership".into(),
            team_id: "target-team".into(),
            agent_member_id: "peer-extra".into(),
            node_id: peer.target_node_id.clone(),
            role: TeamMembershipRole::Member,
            state: TeamMembershipStatus::Active,
            membership_generation: 1,
            default_subscription_refs: Vec::new(),
            created_by: actor("fixture-host"),
            revision: 1,
            joined_at: "t-peer-extra".into(),
            left_at: None,
        };
        store
            .join_team_membership(
                &context(
                    "fixture-host",
                    "membership.join",
                    "target-team-extra-membership",
                    0,
                ),
                extra_membership.clone(),
            )
            .unwrap();
        let before_ambiguous_claim = store.canonical_operations().unwrap();
        store
            .claim_team_message_delivery(
                &service_context("message.team_claim", "peer-claim-ambiguous", 0),
                &delivery.id,
                &TeamMessageDeliveryClaim {
                    claim_id: "peer-claim-ambiguous".into(),
                    team_membership_id: target_membership.id.clone(),
                    membership_generation: target_membership.membership_generation,
                    node_daemon_generation: 1,
                    claim_expires_at: "t-expiry".into(),
                },
                "t-claim-ambiguous",
            )
            .expect_err("a second eligible membership keeps the Team delivery queued");
        assert_eq!(
            store.canonical_operations().unwrap(),
            before_ambiguous_claim
        );
        store
            .leave_team_membership(
                &context(
                    "fixture-host",
                    "membership.leave",
                    "target-team-extra-membership:leave",
                    1,
                ),
                &extra_membership.id,
                "t-peer-extra-left",
            )
            .unwrap();
        let stale_claim = TeamMessageDeliveryClaim {
            claim_id: "peer-claim-stale".into(),
            team_membership_id: target_membership.id.clone(),
            membership_generation: target_membership.membership_generation + 1,
            node_daemon_generation: 1,
            claim_expires_at: "t-expiry".into(),
        };
        let before_stale_claim = store.canonical_operations().unwrap();
        store
            .claim_team_message_delivery(
                &service_context("message.team_claim", "peer-claim-stale", 0),
                &delivery.id,
                &stale_claim,
                "t-claim-stale",
            )
            .expect_err("stale membership generation is fenced");
        assert_eq!(store.canonical_operations().unwrap(), before_stale_claim);
        let claimed = store
            .claim_team_message_delivery(
                &service_context("message.team_claim", "peer-claim", 0),
                &delivery.id,
                &TeamMessageDeliveryClaim {
                    claim_id: "peer-claim".into(),
                    team_membership_id: target_membership.id,
                    membership_generation: target_membership.membership_generation,
                    node_daemon_generation: 1,
                    claim_expires_at: "t-expiry".into(),
                },
                "t-claim",
            )
            .unwrap();
        assert_eq!(
            claimed.projection.status,
            CanonicalMessageDeliveryStatus::Routed
        );
        assert_eq!(
            claimed.projection.recipient_agent_member_id.as_deref(),
            Some("fixture-host")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn peer_authority_fixture(
        company_id: &str,
        source_team: &firm_core::AgentTeam,
        source_membership: &TeamMembership,
        source_member_id: &str,
        source_session_id: &str,
        target_team: &firm_core::AgentTeam,
        target_subscription: &MessageSubscription,
        member_target: Option<&TeamMembership>,
    ) -> PeerTeamMessageAdmissionAuthority {
        let mut authority = PeerTeamMessageAdmissionAuthority {
            company_id: company_id.into(),
            source_execution_space_id: "space-test".into(),
            source_team_id: source_team.id.clone(),
            source_team_revision: source_team.revision,
            source_membership_id: source_membership.id.clone(),
            source_membership_generation: source_membership.membership_generation,
            source_agent_member_id: source_member_id.into(),
            source_session_id: source_session_id.into(),
            source_session_generation: 1,
            source_node_id: source_team.node_id.clone(),
            source_node_daemon_id: "daemon-1".into(),
            source_node_daemon_generation: 1,
            target_execution_space_id: "space-test".into(),
            target_team_id: target_team.id.clone(),
            target_team_revision: target_team.revision,
            target_node_id: target_team.node_id.clone(),
            target_membership_id: member_target.map(|membership| membership.id.clone()),
            target_membership_generation: member_target
                .map(|membership| membership.membership_generation),
            target_agent_member_id: member_target
                .map(|membership| membership.agent_member_id.clone()),
            source_policy_ref: "peer-team-message-admission.v1".into(),
            source_policy_revision: 1,
            source_policy_digest: String::new(),
            source_required_capability: "message.peer_team.author".into(),
            target_subscription_id: target_subscription.id.clone(),
            target_subscription_revision: target_subscription.revision,
            target_authorization_policy_ref: target_subscription.authorization_policy_ref.clone(),
            target_policy_revision: target_subscription.policy_revision,
            target_policy_digest: String::new(),
            target_required_capability: "collaboration.peer_message_deliver".into(),
            authority_digest: String::new(),
        };
        authority.source_policy_digest = peer_team_source_policy_digest(&authority);
        authority.target_policy_digest = peer_team_target_policy_digest(&authority);
        authority.authority_digest = peer_team_message_authority_digest(&authority);
        authority
    }

    fn peer_message_fixture(
        id: &str,
        source_team: &firm_core::AgentTeam,
        sender_member_id: &str,
        sender_session_id: &str,
        recipient: firm_core::agentfirm_api::MessageRecipientRef,
        work_id: Option<&str>,
    ) -> Message {
        let mut message = Message {
            id: id.into(),
            source_execution_space_id: "space-test".into(),
            source_node_id: source_team.node_id.clone(),
            source_node_daemon_id: "daemon-1".into(),
            source_authority_generation: 1,
            sender_actor_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: sender_member_id.into(),
            },
            sender_agent_member_id: Some(sender_member_id.into()),
            sender_session_id: Some(sender_session_id.into()),
            address_kind: match recipient.kind {
                MessageRecipientKind::Team => {
                    firm_core::agentfirm_api::MessageAddressKind::TeamChannel
                }
                _ => firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
            },
            target_ref: recipient.clone(),
            recipients: vec![recipient],
            team_id: Some(source_team.id.clone()),
            team_run_id: None,
            work_id: work_id.map(str::to_string),
            collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
                source_team_id: source_team.id.clone(),
                target_team_id: "target-team".into(),
                delegation_id: None,
                expected_delegation_revision: None,
                source_work_ref: None,
                target_work_ref: None,
            }),
            kind: firm_core::agentfirm_api::MessageKind::Message,
            body: format!("ordinary peer conversation {id}"),
            body_digest: String::new(),
            correlation_id: format!("correlation-{id}"),
            causation_id: None,
            response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
            evidence_refs: Vec::new(),
            content_fingerprint: String::new(),
            schema_version: 1,
            idempotency_key: id.into(),
            created_at: format!("t-{id}"),
        };
        message.body_digest = format!("sha256:{:x}", Sha256::digest(message.body.as_bytes()));
        message.content_fingerprint = message_content_fingerprint(&message);
        message
    }

    fn seed_peer_message_scope(
        store: &HarnessStore,
    ) -> (
        firm_core::AgentTeam,
        firm_core::AgentTeam,
        TeamMembership,
        TeamMembership,
    ) {
        store
            .migrate_legacy_agent_identity_same_id(
                &context("operator", "identity.migrate", "peer-sender", 0),
                identity("remote-sender"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "peer-sender-session", 0),
                session("session-peer-sender", "remote-sender"),
            )
            .unwrap();
        append_runtime_team(store, "source-team", "source-peer-run");
        append_runtime_team(store, "target-team", "target-peer-run");
        let source_team = store
            .agent_teams("space-test")
            .unwrap()
            .into_iter()
            .find(|team| team.id == "source-team")
            .unwrap();
        let target_team = store
            .agent_teams("space-test")
            .unwrap()
            .into_iter()
            .find(|team| team.id == "target-team")
            .unwrap();
        let source_membership = store
            .team_host_membership("space-test", "source-team", true)
            .unwrap();
        let target_membership = store
            .team_host_membership("space-test", "target-team", true)
            .unwrap();
        (
            source_team,
            target_team,
            source_membership,
            target_membership,
        )
    }

    #[test]
    fn peer_team_direct_membership_target_binds_one_delivery_without_claim() {
        let (store, root) = fabric_store();
        let (source_team, target_team, source_membership, target_membership) =
            seed_peer_message_scope(&store);
        let direct_subscription = store
            .fabric_message_subscriptions("space-test")
            .unwrap()
            .into_iter()
            .find(|subscription| {
                subscription.id
                    == format!(
                        "direct:{}:{}",
                        target_membership.agent_member_id, target_membership.id
                    )
            })
            .expect("durable direct subscription");
        let authority = peer_authority_fixture(
            "company-test",
            &source_team,
            &source_membership,
            "remote-sender",
            "session-peer-sender",
            &target_team,
            &direct_subscription,
            Some(&target_membership),
        );
        assert_eq!(
            authority.target_policy_digest, direct_subscription.policy_digest,
            "direct target policy digest is byte-equal to the durable subscription digest"
        );
        store
            .revalidate_peer_team_delivery_subscription("space-test", &authority)
            .expect("direct target subscription independently revalidates");

        let message = peer_message_fixture(
            "peer-direct-message",
            &source_team,
            "remote-sender",
            "session-peer-sender",
            firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::AgentMember,
                id: target_membership.agent_member_id.clone(),
            },
            None,
        );
        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-direct-message", 0),
                message.clone(),
                Some(&MessageAdmissionAuthority::PeerTeam(authority.clone())),
            )
            .expect("same-Space direct peer authoring");
        let deliveries = store
            .fabric_message_deliveries("space-test")
            .unwrap()
            .into_iter()
            .filter(|delivery| delivery.message_id == message.id)
            .collect::<Vec<_>>();
        assert_eq!(deliveries.len(), 1, "exactly one bound delivery");
        let delivery = &deliveries[0];
        assert_eq!(delivery.recipient_kind, MessageSubjectKind::AgentMember);
        assert_eq!(
            delivery.recipient_agent_member_id.as_deref(),
            Some(target_membership.agent_member_id.as_str())
        );
        assert_eq!(
            delivery.resolved_team_membership_id.as_deref(),
            Some(target_membership.id.as_str())
        );
        assert_eq!(delivery.status, CanonicalMessageDeliveryStatus::Queued);
        assert_eq!(delivery.recipient_session_id, None);
        assert_eq!(delivery.subscription_id, direct_subscription.id);

        // A member-bound delivery is already resolved; the Team Inbox claim
        // path must reject it with zero side effects.
        let before = store.canonical_operations().unwrap();
        store
            .claim_team_message_delivery(
                &service_context("message.team_claim", "peer-direct-claim", 0),
                &delivery.id,
                &TeamMessageDeliveryClaim {
                    claim_id: "peer-direct-claim".into(),
                    team_membership_id: target_membership.id.clone(),
                    membership_generation: target_membership.membership_generation,
                    node_daemon_generation: 1,
                    claim_expires_at: "t-expiry".into(),
                },
                "t-claim",
            )
            .expect_err("a member-bound delivery is not Team-claimable");
        assert_eq!(store.canonical_operations().unwrap(), before);

        // A stale target membership generation is fenced before any delivery.
        let mut stale = authority.clone();
        stale.target_membership_generation = Some(target_membership.membership_generation + 1);
        stale.source_policy_digest = peer_team_source_policy_digest(&stale);
        stale.target_policy_digest = peer_team_target_policy_digest(&stale);
        stale.authority_digest = peer_team_message_authority_digest(&stale);
        store
            .revalidate_peer_team_delivery_subscription("space-test", &stale)
            .expect_err("stale membership generation is fenced");

        // A recipient that disagrees with the frozen authority cannot author.
        let cross_wired = peer_message_fixture(
            "peer-direct-cross-wired",
            &source_team,
            "remote-sender",
            "session-peer-sender",
            firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::AgentMember,
                id: "remote-sender".into(),
            },
            None,
        );
        let before = store.canonical_operations().unwrap();
        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-direct-cross-wired", 0),
                cross_wired,
                Some(&MessageAdmissionAuthority::PeerTeam(authority.clone())),
            )
            .expect_err("recipient cannot diverge from the frozen direct target");
        assert_eq!(store.canonical_operations().unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn peer_team_target_subscription_revision_advances_with_team_lifecycle() {
        let (store, root) = fabric_store();
        let (source_team, target_team, source_membership, _target_membership) =
            seed_peer_message_scope(&store);
        let subscription_id = format!("team-inbox:{}", target_team.id);
        let subscription_at = |store: &HarnessStore| {
            store
                .fabric_message_subscriptions("space-test")
                .unwrap()
                .into_iter()
                .find(|subscription| subscription.id == subscription_id)
                .expect("team inbox subscription")
        };
        let team_at = |store: &HarnessStore| {
            store
                .agent_teams("space-test")
                .unwrap()
                .into_iter()
                .find(|team| team.id == "target-team")
                .expect("target team")
        };
        let authority_at = |store: &HarnessStore| {
            let target_team = team_at(store);
            let subscription = subscription_at(store);
            peer_authority_fixture(
                "company-test",
                &source_team,
                &source_membership,
                "remote-sender",
                "session-peer-sender",
                &target_team,
                &subscription,
                None,
            )
        };
        let initial = authority_at(&store);
        assert_eq!(initial.target_subscription_revision, 1);
        store
            .revalidate_peer_team_delivery_subscription("space-test", &initial)
            .expect("current subscription revision revalidates");

        // Team lifecycle transitions advance the durable subscription
        // revision; an authority frozen at the old revision is permanently
        // stale and must be re-resolved from the Store.
        store
            .transition_agent_team(
                &context(
                    "fixture-host",
                    "team.lifecycle.transition",
                    "target-team-off",
                    1,
                ),
                &target_team.id,
                firm_core::AgentTeamStatus::Inactive,
                "t-off",
            )
            .unwrap();
        assert_eq!(subscription_at(&store).revision, 2);
        store
            .revalidate_peer_team_delivery_subscription("space-test", &initial)
            .expect_err("deactivated Team admits no new peer delivery");
        // Reactivation restores the Host membership generation first, then the
        // Team; the subscription revision advances again.
        let host_membership = store
            .fabric_team_memberships("space-test")
            .unwrap()
            .into_iter()
            .find(|membership| {
                membership.team_id == "target-team" && membership.role == TeamMembershipRole::Host
            })
            .unwrap();
        store
            .activate_team_membership(
                &context(
                    "fixture-host",
                    "team.membership.activate",
                    "target-team-host-on",
                    host_membership.revision,
                ),
                &host_membership.id,
                "t-host-on",
            )
            .unwrap();
        store
            .transition_agent_team(
                &context(
                    "fixture-host",
                    "team.lifecycle.transition",
                    "target-team-on",
                    2,
                ),
                &target_team.id,
                firm_core::AgentTeamStatus::Active,
                "t-on",
            )
            .unwrap();
        assert_eq!(subscription_at(&store).revision, 3);
        store
            .revalidate_peer_team_delivery_subscription("space-test", &initial)
            .expect_err("the revision-1 authority stays stale after reactivation");

        let current = authority_at(&store);
        assert_eq!(current.target_subscription_revision, 3);
        assert_eq!(current.target_team_revision, team_at(&store).revision);
        store
            .revalidate_peer_team_delivery_subscription("space-test", &current)
            .expect("re-resolved current subscription revision revalidates");
        let message = peer_message_fixture(
            "peer-after-reactivation",
            &source_team,
            "remote-sender",
            "session-peer-sender",
            firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::Team,
                id: "target-team".into(),
            },
            None,
        );
        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-after-reactivation", 0),
                message.clone(),
                Some(&MessageAdmissionAuthority::PeerTeam(current)),
            )
            .expect("authoring resumes under the current subscription revision");
        let delivery = store
            .fabric_message_deliveries("space-test")
            .unwrap()
            .into_iter()
            .find(|delivery| delivery.message_id == message.id)
            .expect("one Team Inbox delivery");
        assert_eq!(delivery.subscription_revision, 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn peer_team_claim_replays_exactly_and_resolved_delivery_rejects_new_claims() {
        let (store, root) = fabric_store();
        let (source_team, target_team, source_membership, _target_membership) =
            seed_peer_message_scope(&store);
        let target_subscription = store
            .fabric_message_subscriptions("space-test")
            .unwrap()
            .into_iter()
            .find(|subscription| subscription.id == "team-inbox:target-team")
            .unwrap();
        let authority = peer_authority_fixture(
            "company-test",
            &source_team,
            &source_membership,
            "remote-sender",
            "session-peer-sender",
            &target_team,
            &target_subscription,
            None,
        );
        let message = peer_message_fixture(
            "peer-claim-replay",
            &source_team,
            "remote-sender",
            "session-peer-sender",
            firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::Team,
                id: "target-team".into(),
            },
            None,
        );
        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-claim-replay", 0),
                message.clone(),
                Some(&MessageAdmissionAuthority::PeerTeam(authority)),
            )
            .unwrap();
        let delivery = store
            .fabric_message_deliveries("space-test")
            .unwrap()
            .into_iter()
            .find(|delivery| delivery.message_id == message.id)
            .unwrap();
        let claim = TeamMessageDeliveryClaim {
            claim_id: "peer-claim-exact".into(),
            team_membership_id: store
                .team_host_membership("space-test", "target-team", true)
                .unwrap()
                .id,
            membership_generation: 1,
            node_daemon_generation: 1,
            claim_expires_at: "t-expiry".into(),
        };
        let claimed = store
            .claim_team_message_delivery(
                &service_context("message.team_claim", "peer-claim-exact", 0),
                &delivery.id,
                &claim,
                "t-claim",
            )
            .unwrap();
        assert!(!claimed.replayed);
        // An exact retry returns the original result without a new operation.
        let replayed = store
            .claim_team_message_delivery(
                &service_context("message.team_claim", "peer-claim-exact", 0),
                &delivery.id,
                &claim,
                "t-claim",
            )
            .unwrap();
        assert!(replayed.replayed);
        assert_eq!(replayed.projection.version, claimed.projection.version);
        assert_eq!(
            store
                .canonical_operations()
                .unwrap()
                .iter()
                .filter(|operation| operation.event.aggregate_kind == "team_message_delivery_claim")
                .count(),
            1
        );
        // A different claim on the resolved delivery is side-effect free.
        let before = store.canonical_operations().unwrap();
        store
            .claim_team_message_delivery(
                &service_context("message.team_claim", "peer-claim-second", 0),
                &delivery.id,
                &TeamMessageDeliveryClaim {
                    claim_id: "peer-claim-second".into(),
                    ..claim
                },
                "t-claim-2",
            )
            .expect_err("a resolved Team delivery cannot be claimed twice");
        assert_eq!(store.canonical_operations().unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn peer_team_message_work_link_is_context_bound_to_the_source_team() {
        let (store, root) = fabric_store();
        let (source_team, target_team, source_membership, _target_membership) =
            seed_peer_message_scope(&store);
        let work = insert_runtime_work(&store, "work-context-1", "source-team", "source-peer-run");
        let target_subscription = store
            .fabric_message_subscriptions("space-test")
            .unwrap()
            .into_iter()
            .find(|subscription| subscription.id == "team-inbox:target-team")
            .unwrap();
        let authority = peer_authority_fixture(
            "company-test",
            &source_team,
            &source_membership,
            "remote-sender",
            "session-peer-sender",
            &target_team,
            &target_subscription,
            None,
        );
        let message = peer_message_fixture(
            "peer-work-context",
            &source_team,
            "remote-sender",
            "session-peer-sender",
            firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::Team,
                id: "target-team".into(),
            },
            Some(&work.id),
        );
        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-work-context", 0),
                message.clone(),
                Some(&MessageAdmissionAuthority::PeerTeam(authority.clone())),
            )
            .expect("a context-only Work link of the source Team is preserved");
        let stored = store
            .fabric_messages("space-test")
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == message.id)
            .unwrap();
        assert_eq!(stored.work_id.as_deref(), Some(work.id.as_str()));
        // The Work itself is untouched: no operation, delivery, or phase change.
        assert_eq!(
            store
                .latest_works()
                .unwrap()
                .into_iter()
                .find(|w| w.id == work.id)
                .map(|w| w.version),
            Some(work.version)
        );

        let foreign = peer_message_fixture(
            "peer-work-context-foreign",
            &source_team,
            "remote-sender",
            "session-peer-sender",
            firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::Team,
                id: "target-team".into(),
            },
            Some("work-that-does-not-exist"),
        );
        let before = store.canonical_operations().unwrap();
        store
            .author_message_with_admission_authority(
                &service_context("message.author", "peer-work-context-foreign", 0),
                foreign,
                Some(&MessageAdmissionAuthority::PeerTeam(authority)),
            )
            .expect_err("a Work link must name a current Work of the source Team");
        assert_eq!(store.canonical_operations().unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_id_team_migration_fails_closed_on_alias_and_purge_records_no_delete_tombstone() {
        let (store, root) = fabric_store();
        for id in ["legacy-host", "legacy-member"] {
            store
                .migrate_legacy_agent_identity_same_id(
                    &context("operator", "identity.migrate", &format!("identity-{id}"), 0),
                    identity(id),
                )
                .unwrap();
        }
        let source = firm_core::agentfirm_api::LegacyAgentTeamProjection {
            id: "legacy-team".into(),
            name: "Legacy Team".into(),
            description: "explicit same-ID import".into(),
            mission_id: "legacy-mission".into(),
            host_agent_id: "legacy-host".into(),
            node_id: "11111111-1111-4111-8111-111111111111".into(),
            status: firm_core::agentfirm_api::LegacyAgentTeamStatus::Archived,
            member_ids: vec!["legacy-member".into()],
            created_at: "t1".into(),
            updated_at: "t2".into(),
        };
        let target = AgentTeam {
            id: source.id.clone(),
            name: source.name.clone(),
            description: source.description.clone(),
            node_id: source.node_id.clone(),
            status: AgentTeamStatus::Trashed,
            revision: 1,
            legacy_mission_id: Some(source.mission_id.clone()),
            trashed_at: Some(source.updated_at.clone()),
            mission_id: source.mission_id.clone(),
            host_agent_id: source.host_agent_id.clone(),
            member_ids: source.member_ids.clone(),
            created_at: source.created_at.clone(),
            updated_at: source.updated_at.clone(),
        };
        let migration_actor = actor("operator");
        let memberships = [
            ("legacy-host", TeamMembershipRole::Host),
            ("legacy-member", TeamMembershipRole::Member),
        ]
        .into_iter()
        .map(|(member_id, role)| TeamMembership {
            id: format!("membership:legacy-team:{member_id}"),
            team_id: "legacy-team".into(),
            agent_member_id: member_id.into(),
            node_id: source.node_id.clone(),
            role,
            state: TeamMembershipStatus::Inactive,
            membership_generation: 1,
            default_subscription_refs: Vec::new(),
            created_by: migration_actor.clone(),
            revision: 1,
            joined_at: "t1".into(),
            left_at: Some("t2".into()),
        })
        .collect::<Vec<_>>();
        let source_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(&source).unwrap());
        let mut bundle = AgentTeamMigrationBundle {
            source,
            target,
            memberships,
            identity_id_map: BTreeMap::from([
                ("legacy-host".into(), "legacy-host".into()),
                ("legacy-member".into(), "legacy-member".into()),
            ]),
            migration_id: "migration-legacy-team".into(),
            source_fingerprint,
        };
        let before_alias = store.canonical_operations().unwrap();
        bundle
            .identity_id_map
            .insert("legacy-host".into(), "legacy-member".into());
        store
            .migrate_legacy_agent_team_same_ids(
                &context("operator", "agent_team.migrate", "migration-hostile", 0),
                bundle.clone(),
            )
            .expect_err("identity aliasing fails closed");
        assert_eq!(store.canonical_operations().unwrap(), before_alias);
        bundle
            .identity_id_map
            .insert("legacy-host".into(), "legacy-host".into());
        let migrated = store
            .migrate_legacy_agent_team_same_ids(
                &context("operator", "agent_team.migrate", "migration-legacy-team", 0),
                bundle,
            )
            .unwrap();
        assert_eq!(migrated.projection.id, "legacy-team");
        assert_eq!(migrated.projection.status, AgentTeamStatus::Trashed);
        let rows_before_purge = store.canonical_operations().unwrap().len();
        let tombstone = store
            .record_agent_team_purge_tombstone(
                &context("operator", "agent_team.purge", "purge-legacy-team", 0),
                AgentTeamPurgeRequest {
                    tombstone_id: "purge-legacy-team".into(),
                    team_id: "legacy-team".into(),
                    expected_team_revision: 1,
                    approval_ref: "approval:purge".into(),
                    export_manifest_ref: "export:legacy-team".into(),
                    restore_window_closed_at: "t3".into(),
                    requested_by: migration_actor,
                    requested_at: "t4".into(),
                },
            )
            .unwrap();
        assert_eq!(tombstone.projection.team_id, "legacy-team");
        assert_eq!(
            store.canonical_operations().unwrap().len(),
            rows_before_purge + 1
        );
        assert!(store
            .agent_teams("space-test")
            .unwrap()
            .iter()
            .any(|team| team.id == "legacy-team"));
        assert_eq!(
            store
                .fabric_team_memberships("space-test")
                .unwrap()
                .iter()
                .filter(|membership| membership.team_id == "legacy-team")
                .count(),
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn team_trash_restore_preserves_work_message_membership_and_native_session_records() {
        let (store, root) = fabric_store();
        append_runtime_team(&store, "team-trash", "run-trash");
        store
            .create_agent_session(
                &service_context("session.create", "session-trash-host", 0),
                session("session-trash-host", "fixture-host"),
            )
            .unwrap();
        store
            .bind_agent_session_native_session(
                &service_context("session.native.bind", "native-trash-host", 1),
                "session-trash-host",
                1,
                settled_native_session("native-trash-host"),
            )
            .unwrap();
        insert_runtime_work(&store, "work-trash", "team-trash", "run-trash");
        let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::Team,
            id: "team-trash".into(),
        }];
        let mut message = Message {
            id: "message-trash".into(),
            source_execution_space_id: "space-test".into(),
            source_node_id: "11111111-1111-4111-8111-111111111111".into(),
            source_node_daemon_id: "daemon-1".into(),
            source_authority_generation: 1,
            sender_actor_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "fixture-host".into(),
            },
            sender_agent_member_id: Some("fixture-host".into()),
            sender_session_id: Some("session-trash-host".into()),
            address_kind: firm_core::agentfirm_api::MessageAddressKind::TeamChannel,
            target_ref: recipients[0].clone(),
            recipients,
            team_id: Some("team-trash".into()),
            team_run_id: Some("run-trash".into()),
            work_id: Some("work-trash".into()),
            collaboration_scope: None,
            kind: firm_core::agentfirm_api::MessageKind::Message,
            body: "retain this Team record".into(),
            body_digest: format!("sha256:{:x}", Sha256::digest(b"retain this Team record")),
            correlation_id: "trash-correlation".into(),
            causation_id: None,
            response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
            evidence_refs: Vec::new(),
            content_fingerprint: String::new(),
            schema_version: 1,
            idempotency_key: "message-trash".into(),
            created_at: "t-message".into(),
        };
        message.content_fingerprint = message_content_fingerprint(&message);
        store
            .author_message(
                &service_context("message.author", "message-trash", 0),
                message,
            )
            .unwrap();
        let works_before = store.latest_works().unwrap();
        let messages_before = store.fabric_messages("space-test").unwrap();
        let deliveries_before = store.fabric_message_deliveries("space-test").unwrap();
        let sessions_before = store.fabric_agent_sessions("space-test").unwrap();
        let membership_before = store
            .team_host_membership("space-test", "team-trash", true)
            .unwrap();
        let trashed = store
            .transition_agent_team(
                &context("fixture-host", "agent_team.trash", "team-trash", 1),
                "team-trash",
                AgentTeamStatus::Trashed,
                "t-trash",
            )
            .unwrap();
        assert_eq!(
            trashed.projection.node_id,
            "11111111-1111-4111-8111-111111111111"
        );
        let restored = store
            .transition_agent_team(
                &context("fixture-host", "agent_team.restore", "team-restore", 2),
                "team-trash",
                AgentTeamStatus::Inactive,
                "t-restore",
            )
            .unwrap();
        assert_eq!(restored.projection.status, AgentTeamStatus::Inactive);
        assert_eq!(store.latest_works().unwrap(), works_before);
        assert_eq!(
            store.fabric_messages("space-test").unwrap(),
            messages_before
        );
        assert_eq!(
            store.fabric_message_deliveries("space-test").unwrap(),
            deliveries_before
        );
        assert_eq!(
            store.fabric_agent_sessions("space-test").unwrap(),
            sessions_before
        );
        let retained_membership = store
            .fabric_team_memberships("space-test")
            .unwrap()
            .into_iter()
            .find(|membership| membership.id == membership_before.id)
            .unwrap();
        assert_eq!(retained_membership.state, TeamMembershipStatus::Inactive);
        store
            .activate_team_membership(
                &context(
                    "fixture-host",
                    "membership.activate",
                    "membership-trash-activate",
                    retained_membership.revision,
                ),
                &retained_membership.id,
                "t-membership-active",
            )
            .unwrap();
        store
            .transition_agent_team(
                &context(
                    "fixture-host",
                    "agent_team.activate",
                    "team-trash-active",
                    3,
                ),
                "team-trash",
                AgentTeamStatus::Active,
                "t-active",
            )
            .unwrap();
        assert_eq!(
            store
                .team_host_membership("space-test", "team-trash", true)
                .unwrap()
                .id,
            membership_before.id
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn settled_native_session(id: &str) -> NativeSessionRef {
        NativeSessionRef {
            provider: "codex".into(),
            execution_mode: "codex_app_server".into(),
            native_session_id: id.into(),
            native_locator_kind: "codex_rollout".into(),
            provider_version: None,
            adapter_contract_version: "codex-app-server-v1".into(),
            availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
            supports_resume: true,
            last_verified_at: Some("t2".into()),
            parent_native_session_id: None,
        }
    }

    #[test]
    fn bind_agent_session_native_session_is_cas_generation_fenced_and_idempotent() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "identity-bind-native", 0),
                identity("bind-native"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "session-bind-native", 0),
                session("session-bind-native", "bind-native"),
            )
            .unwrap();
        let native = settled_native_session("thread-settled-1");

        let stale_generation = store
            .bind_agent_session_native_session(
                &service_context("session.native.bind", "bind-native-stale-generation", 1),
                "session-bind-native",
                2,
                native.clone(),
            )
            .expect_err("a settled binding from another runtime generation is fenced");
        assert!(
            stale_generation
                .to_string()
                .contains("MEMBER_RUN_GENERATION_FENCED"),
            "{stale_generation}"
        );
        let stale_version = store
            .bind_agent_session_native_session(
                &service_context("session.native.bind", "bind-native-stale-version", 0),
                "session-bind-native",
                1,
                native.clone(),
            )
            .expect_err("the bind CAS rejects a stale expected version");
        assert!(
            stale_version.to_string().contains("VERSION_CONFLICT"),
            "{stale_version}"
        );

        let bound = store
            .bind_agent_session_native_session(
                &service_context("session.native.bind", "bind-native-session", 1),
                "session-bind-native",
                1,
                native.clone(),
            )
            .expect("first settle binds the native Session");
        assert!(!bound.replayed);
        assert_eq!(bound.projection.version, 2);
        assert_eq!(bound.projection.native_session_ref.as_ref(), Some(&native));
        assert_eq!(bound.projection.lifecycle, AgentSessionStatus::Idle);
        assert_eq!(bound.projection.runtime_generation, 1);

        let replay = store
            .bind_agent_session_native_session(
                &service_context("session.native.bind", "bind-native-session", 1),
                "session-bind-native",
                1,
                native.clone(),
            )
            .expect("the exact same bind replays");
        assert!(replay.replayed);
        assert_eq!(replay.projection.version, 2);
        assert_eq!(replay.event.id, bound.event.id);

        let rewritten = store
            .bind_agent_session_native_session(
                &service_context("session.native.bind", "bind-native-session-again", 2),
                "session-bind-native",
                1,
                native.clone(),
            )
            .expect("rebinding the same native id is idempotent in effect");
        assert_eq!(rewritten.projection.version, 3);
        assert_eq!(
            rewritten
                .projection
                .native_session_ref
                .as_ref()
                .map(|value| value.native_session_id.as_str()),
            Some("thread-settled-1")
        );

        let conflicting = store
            .bind_agent_session_native_session(
                &service_context("session.native.bind", "bind-native-conflict", 3),
                "session-bind-native",
                1,
                settled_native_session("thread-other"),
            )
            .expect_err("a different native id cannot overwrite the binding");
        assert!(
            conflicting
                .to_string()
                .contains("already binds another provider-native Session"),
            "{conflicting}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bind_member_run_native_session_is_cas_generation_fenced_and_idempotent() {
        let (store, root) = fabric_store();
        append_runtime_team(&store, "team-bind-native", "team-run-bind-native");
        let run = MemberRun {
            id: "member-run-bind-native".into(),
            agent_member_id: "fixture-host".into(),
            team_run_id: "team-run-bind-native".into(),
            role_snapshot: "implementer".into(),
            provider_profile_snapshot: Some("codex/codex_app_server".into()),
            requested_controls: serde_json::json!({}),
            effective_controls: serde_json::json!({}),
            coordination_status: MemberCoordinationStatus::Active,
            runtime_status: MemberRuntimeStatus::Idle,
            runtime_generation: 1,
            workspace_binding_id: None,
            native_session: None,
            version: 1,
            started_at: "t1".into(),
            last_event_at: None,
            finished_at: None,
        };
        store
            .legacy_import_create_trust_member_run_projection(
                &context("host", "member_run.create", "member-run-bind-native", 0),
                run,
            )
            .unwrap();
        let native = settled_native_session("thread-settled-2");

        let stale_generation = store
            .bind_member_run_native_session(
                &context(
                    "host",
                    "member_run.native.bind",
                    "bind-run-stale-generation",
                    1,
                ),
                "member-run-bind-native",
                2,
                native.clone(),
                "t2",
            )
            .expect_err("a settled binding from another runtime generation is fenced");
        assert!(
            stale_generation
                .to_string()
                .contains("MEMBER_RUN_GENERATION_FENCED"),
            "{stale_generation}"
        );
        let stale_version = store
            .bind_member_run_native_session(
                &context(
                    "host",
                    "member_run.native.bind",
                    "bind-run-stale-version",
                    0,
                ),
                "member-run-bind-native",
                1,
                native.clone(),
                "t2",
            )
            .expect_err("the bind CAS rejects a stale expected version");
        assert!(
            stale_version.to_string().contains("VERSION_CONFLICT"),
            "{stale_version}"
        );

        let bound = store
            .bind_member_run_native_session(
                &context("host", "member_run.native.bind", "bind-run-native", 1),
                "member-run-bind-native",
                1,
                native.clone(),
                "t2",
            )
            .expect("first settle binds the native Session");
        assert!(!bound.replayed);
        assert_eq!(bound.projection.version, 2);
        assert_eq!(bound.projection.native_session.as_ref(), Some(&native));
        assert_eq!(
            bound.projection.coordination_status,
            MemberCoordinationStatus::Active
        );
        assert_eq!(bound.projection.runtime_status, MemberRuntimeStatus::Idle);
        assert_eq!(bound.projection.runtime_generation, 1);
        assert_eq!(bound.projection.last_event_at.as_deref(), Some("t2"));

        let replay = store
            .bind_member_run_native_session(
                &context("host", "member_run.native.bind", "bind-run-native", 1),
                "member-run-bind-native",
                1,
                native.clone(),
                "t2",
            )
            .expect("the exact same bind replays");
        assert!(replay.replayed);
        assert_eq!(replay.projection.version, 2);
        assert_eq!(replay.event.id, bound.event.id);

        let conflicting = store
            .bind_member_run_native_session(
                &context("host", "member_run.native.bind", "bind-run-conflict", 2),
                "member-run-bind-native",
                1,
                settled_native_session("thread-other"),
                "t3",
            )
            .expect_err("a different native id cannot overwrite the binding");
        assert!(
            conflicting
                .to_string()
                .contains("already binds another provider-native Session"),
            "{conflicting}"
        );
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
            .filter(|membership| {
                membership.state == TeamMembershipStatus::Active
                    && membership.agent_member_id == "membership-agent"
            })
            .collect::<Vec<_>>();
        assert_eq!(active.len(), 1);
        assert_eq!(
            store
                .fabric_message_subscriptions("space-test")
                .unwrap()
                .into_iter()
                .filter(|subscription| {
                    subscription.subscriber_kind == MessageSubjectKind::AgentMember
                        && subscription.subscriber_ref == "membership-agent"
                        && subscription.status == MessageSubscriptionStatus::Active
                })
                .count(),
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn node_daemon_authors_and_claims_identity_first_message() {
        let (store, root) = fabric_store();
        for id in ["sender", "recipient"] {
            store
                .migrate_legacy_agent_identity_same_id(
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
            subscriber_kind: MessageSubjectKind::AgentMember,
            subscriber_ref: "recipient".into(),
            execution_space_id: "space-test".into(),
            target_team_id: None,
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
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
                    &serde_json::json!({"recipient_agent_member_id": "recipient"}),
                    vec![serde_json::to_value(&subscription).unwrap()],
                    Vec::new(),
                )
                .unwrap();
        }
        let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id: "recipient".into(),
        }];
        let body_digest = format!("sha256:{:x}", Sha256::digest(b"hello"));
        let fingerprint = canonical_json_fingerprint(&serde_json::json!({
            "sender_actor_ref": {"kind": "agent_member", "id": "sender"},
            "sender_agent_member_id": "sender",
            "sender_session_id": "session-sender",
            "address_kind": "direct_agent",
            "target_ref": {"kind": "agent_member", "id": "recipient"},
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
                    sender_agent_member_id: Some("sender".into()),
                    sender_session_id: Some("session-sender".into()),
                    address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
                    target_ref: firm_core::agentfirm_api::MessageRecipientRef {
                        kind: MessageRecipientKind::AgentMember,
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
        assert_eq!(dispatch.projection.recipient_agent_member_id, "recipient");
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
    fn source_node_authors_cross_node_message_only_with_frozen_delegation_authority() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
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
        let source_membership = store
            .team_host_membership("space-test", "source-team", true)
            .unwrap();
        let source_work =
            insert_runtime_work(&store, "source-work", "source-team", "source-team-run");
        store
            .bind_work_execution(
                &context("fixture-host", "work.bind", "source-work-binding", 0),
                WorkExecutionBinding {
                    id: "source-work-binding".into(),
                    work_id: source_work.id.clone(),
                    work_revision: source_work.version,
                    team_id: "source-team".into(),
                    team_membership_id: source_membership.id,
                    agent_member_id: "remote-sender".into(),
                    agent_session_id: "session-remote-sender".into(),
                    agent_session_generation: 1,
                    delivery_id: "source-work-delivery".into(),
                    binding_generation: 1,
                    status: WorkExecutionBindingStatus::Active,
                    version: 1,
                    created_by: actor("fixture-host"),
                    bound_at: "t-binding".into(),
                    ended_at: None,
                },
            )
            .unwrap();
        let source_work_ref = firm_core::collaboration::RemoteWorkRef {
            schema_version: "agentfirm.remote-work-ref.v1".into(),
            execution_space_id: "space-test".into(),
            node_id: "11111111-1111-4111-8111-111111111111".into(),
            team_id: "source-team".into(),
            team_revision: 1,
            placement_generation: 1,
            work_id: source_work.id.clone(),
            work_revision: source_work.version,
            work_event_id: "source-work-event".into(),
            digest: canonical_json_fingerprint(&serde_json::to_value(&source_work).unwrap()),
        };
        let target_work_ref = firm_core::collaboration::RemoteWorkRef {
            schema_version: "agentfirm.remote-work-ref.v1".into(),
            execution_space_id: "space-target".into(),
            node_id: "22222222-2222-4222-8222-222222222222".into(),
            team_id: "target-team".into(),
            team_revision: 1,
            placement_generation: 1,
            work_id: "target-work".into(),
            work_revision: 1,
            work_event_id: "target-work-event".into(),
            digest: format!("sha256:{:064x}", 2),
        };
        let policy = firm_core::collaboration::DelegationInboundPolicySnapshot {
            policy_id: "policy-source-target".into(),
            policy_revision: 1,
            policy_digest: format!("sha256:{:064x}", 3),
            mode: firm_core::collaboration::DelegationInboundMode::HostApprovalRequired,
            allowed_outcome_classes: vec!["implementation".into()],
            max_active_delegations: 1,
        };
        let mut authority = CollaborationMessageAuthority {
            company_id: "company-test".into(),
            delegation_id: "delegation-a-b".into(),
            delegation_revision: 3,
            source_work_ref: source_work_ref.clone(),
            target_work_ref: target_work_ref.clone(),
            target_placement: firm_core::collaboration::TargetPlacementRef {
                team_id: "target-team".into(),
                team_revision: 1,
                node_id: "22222222-2222-4222-8222-222222222222".into(),
                placement_generation: 1,
            },
            source_owner_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "remote-sender".into(),
            },
            source_host_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "remote-sender".into(),
            },
            target_host_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "target-host-on-another-node".into(),
            },
            inbound_policy_snapshot: policy,
            authority_digest: String::new(),
        };
        authority.authority_digest = canonical_json_fingerprint(&serde_json::json!({
            "company_id": authority.company_id,
            "delegation_id": authority.delegation_id,
            "delegation_revision": authority.delegation_revision,
            "source_work_ref": authority.source_work_ref,
            "target_work_ref": authority.target_work_ref,
            "target_placement": authority.target_placement,
            "source_owner_ref": authority.source_owner_ref,
            "source_host_ref": authority.source_host_ref,
            "target_host_ref": authority.target_host_ref,
            "inbound_policy_snapshot": authority.inbound_policy_snapshot,
        }));
        let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
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
            sender_agent_member_id: Some("remote-sender".into()),
            sender_session_id: Some("session-remote-sender".into()),
            address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
            target_ref: recipients[0].clone(),
            recipients,
            team_id: Some("source-team".into()),
            team_run_id: Some("source-team-run".into()),
            work_id: Some(source_work.id.clone()),
            collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
                source_team_id: "source-team".into(),
                target_team_id: "target-team".into(),
                delegation_id: Some("delegation-a-b".into()),
                expected_delegation_revision: Some(3),
                source_work_ref: Some(source_work_ref),
                target_work_ref: Some(target_work_ref),
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

        let before = store.canonical_operations().unwrap();
        let messages_before = store.fabric_messages("space-test").unwrap();
        let deliveries_before = store.fabric_message_deliveries("space-test").unwrap();
        store
            .author_message(
                &service_context("message.author", "cross-node-message", 0),
                message.clone(),
            )
            .expect_err("caller-built collaboration scope is not Message authority");
        assert_eq!(store.canonical_operations().unwrap(), before);
        assert_eq!(
            store.fabric_messages("space-test").unwrap(),
            messages_before
        );
        assert_eq!(
            store.fabric_message_deliveries("space-test").unwrap(),
            deliveries_before
        );
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "identity-wrong-source", 0),
                identity("wrong-source"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "wrong-source-session", 0),
                session("session-wrong-source", "wrong-source"),
            )
            .unwrap();
        join_runtime_membership(
            &store,
            "wrong-source-membership",
            "source-team",
            "wrong-source",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        let mut wrong_source = message.clone();
        wrong_source.id = "cross-node-message-wrong-source".into();
        wrong_source.sender_actor_ref = ActorRef {
            kind: ActorKind::AgentMember,
            id: "wrong-source".into(),
        };
        wrong_source.sender_agent_member_id = Some("wrong-source".into());
        wrong_source.sender_session_id = Some("session-wrong-source".into());
        wrong_source.idempotency_key = wrong_source.id.clone();
        wrong_source.content_fingerprint = message_content_fingerprint(&wrong_source);
        let before_wrong_source = store.canonical_operations().unwrap();
        store
            .author_message_with_collaboration_authority(
                &service_context("message.author", &wrong_source.id, 0),
                wrong_source,
                Some(&authority),
            )
            .expect_err("ordinary source Team Member cannot impersonate Delegation authority");
        assert_eq!(
            store.canonical_operations().unwrap(),
            before_wrong_source,
            "hostile source actor has zero Message/Delivery side effects"
        );
        assert_eq!(
            store.fabric_messages("space-test").unwrap(),
            messages_before
        );
        assert_eq!(
            store.fabric_message_deliveries("space-test").unwrap(),
            deliveries_before
        );
        let authored = store
            .author_message_with_collaboration_authority(
                &service_context("message.author", "cross-node-message", 0),
                message.clone(),
                Some(&authority),
            )
            .expect("source Node validates frozen Delegation authority under the Store lock");
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
            .migrate_legacy_agent_identity_same_id(
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
        let target_work =
            insert_runtime_work(&store, "target-work", "target-team", "target-team-run");
        join_runtime_membership(
            &store,
            "target-membership",
            "target-team",
            "remote-recipient",
            firm_core::agentfirm_api::TeamMembershipRole::Member,
        );
        let subscription = MessageSubscription {
            id: "remote-direct-recipient".into(),
            subscriber_kind: MessageSubjectKind::AgentMember,
            subscriber_ref: "remote-recipient".into(),
            execution_space_id: "space-test".into(),
            target_team_id: None,
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
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
                    &serde_json::json!({"recipient_agent_member_id": "remote-recipient"}),
                    vec![serde_json::to_value(&subscription).unwrap()],
                    Vec::new(),
                )
                .unwrap();
        }

        let make_message = |body: &str| {
            let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
                kind: MessageRecipientKind::AgentMember,
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
                sender_agent_member_id: Some("remote-sender".into()),
                sender_session_id: Some("remote-sender-session".into()),
                address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
                target_ref: recipients[0].clone(),
                recipients,
                team_id: Some("source-team".into()),
                team_run_id: None,
                work_id: Some("source-work".into()),
                collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
                    source_team_id: "source-team".into(),
                    target_team_id: "target-team".into(),
                    delegation_id: Some("delegation-source-target".into()),
                    expected_delegation_revision: Some(3),
                    source_work_ref: Some(firm_core::collaboration::RemoteWorkRef {
                        schema_version: "agentfirm.remote-work-ref.v1".into(),
                        execution_space_id: "space-source".into(),
                        node_id: "22222222-2222-4222-8222-222222222222".into(),
                        team_id: "source-team".into(),
                        team_revision: 1,
                        placement_generation: 1,
                        work_id: "source-work".into(),
                        work_revision: 1,
                        work_event_id: "source-work-event".into(),
                        digest: format!("sha256:{:064x}", 1),
                    }),
                    target_work_ref: Some(firm_core::collaboration::RemoteWorkRef {
                        schema_version: "agentfirm.remote-work-ref.v1".into(),
                        execution_space_id: "space-test".into(),
                        node_id: "11111111-1111-4111-8111-111111111111".into(),
                        team_id: "target-team".into(),
                        team_revision: 1,
                        placement_generation: 1,
                        work_id: target_work.id.clone(),
                        work_revision: target_work.version,
                        work_event_id: "target-work-event".into(),
                        digest: canonical_json_fingerprint(
                            &serde_json::to_value(&target_work).unwrap(),
                        ),
                    }),
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
            let scope = message.collaboration_scope.as_ref().unwrap();
            let policy = firm_core::collaboration::DelegationInboundPolicySnapshot {
                policy_id: "policy-source-target".into(),
                policy_revision: 1,
                policy_digest: format!("sha256:{:064x}", 4),
                mode: firm_core::collaboration::DelegationInboundMode::HostApprovalRequired,
                allowed_outcome_classes: vec!["implementation".into()],
                max_active_delegations: 1,
            };
            let mut authority = CollaborationMessageAuthority {
                company_id: "company-test".into(),
                delegation_id: scope.delegation_id.clone().unwrap(),
                delegation_revision: scope.expected_delegation_revision.unwrap(),
                source_work_ref: scope.source_work_ref.clone().unwrap(),
                target_work_ref: scope.target_work_ref.clone().unwrap(),
                target_placement: firm_core::collaboration::TargetPlacementRef {
                    team_id: "target-team".into(),
                    team_revision: 1,
                    node_id: "11111111-1111-4111-8111-111111111111".into(),
                    placement_generation: 1,
                },
                source_owner_ref: message.sender_actor_ref.clone(),
                source_host_ref: message.sender_actor_ref.clone(),
                target_host_ref: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: "target-host".into(),
                },
                inbound_policy_snapshot: policy,
                authority_digest: String::new(),
            };
            authority.authority_digest = canonical_json_fingerprint(&serde_json::json!({
                "company_id": authority.company_id,
                "delegation_id": authority.delegation_id,
                "delegation_revision": authority.delegation_revision,
                "source_work_ref": authority.source_work_ref,
                "target_work_ref": authority.target_work_ref,
                "target_placement": authority.target_placement,
                "source_owner_ref": authority.source_owner_ref,
                "source_host_ref": authority.source_host_ref,
                "target_host_ref": authority.target_host_ref,
                "inbound_policy_snapshot": authority.inbound_policy_snapshot,
            }));
            let message_reference = firm_fabric::MessageReference {
                message_id: message.id.clone(),
                body_digest: message.body_digest.clone(),
                canonical_message_envelope: Some(serde_json::to_value(message).unwrap()),
                message_object_ref: None,
            };
            let payload = serde_json::json!({
                "message_reference": message_reference,
                "delegation_authority": authority,
            });
            let body = serde_json::to_value(firm_fabric::CollaborationBusinessReference {
                business_kind: "team_message_deliver".into(),
                required_capability: "collaboration.team_message_deliver".into(),
                business_actor_kind: "agent_member".into(),
                business_actor_id: "remote-sender".into(),
                target_team_id: "target-team".into(),
                target_team_revision: 1,
                placement_generation: 1,
                expected_revision: 3,
                payload_digest: canonical_json_fingerprint(&payload),
                payload,
            })
            .unwrap();
            firm_fabric::RoutedOperation {
                id: "remote-route-1".into(),
                company_id: "company-test".into(),
                kind: firm_fabric::COLLABORATION_BUSINESS_OPERATION_KIND.into(),
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
                authorization_context: BTreeMap::from([
                    ("target_team_id".into(), "target-team".into()),
                    ("target_team_revision".into(), "1".into()),
                    ("placement_generation".into(), "1".into()),
                    (
                        "required_capability".into(),
                        "collaboration.team_message_deliver".into(),
                    ),
                    ("business_actor_kind".into(), "agent_member".into()),
                    ("business_actor_id".into(), "remote-sender".into()),
                ]),
                idempotency_key: "remote-route-1".into(),
                ordering_key: "message:remote-recipient".into(),
                correlation_id: message.correlation_id.clone(),
                causation_id: None,
                expected_target_revision: Some(3),
                body_schema: firm_fabric::COLLABORATION_BUSINESS_OPERATION_SCHEMA.into(),
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
        let mut no_delegation_authority = operation.clone();
        no_delegation_authority
            .body
            .get_mut("payload")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap()
            .remove("delegation_authority");
        no_delegation_authority.body_digest =
            firm_fabric::json_digest(&no_delegation_authority.body).unwrap();
        let mut rejected_context =
            service_context("remote_message_persist", &no_delegation_authority.id, 0);
        rejected_context.request_fingerprint =
            Some(firm_fabric::json_digest(&no_delegation_authority).unwrap());
        let operations_before_reject = store.canonical_operations().unwrap();
        let messages_before_reject = store.fabric_messages("space-test").unwrap();
        let deliveries_before_reject = store.fabric_message_deliveries("space-test").unwrap();
        store
            .persist_remote_message(
                &rejected_context,
                &no_delegation_authority,
                message.clone(),
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
            )
            .expect_err("target application requires the frozen Delegation authority");
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_reject
        );
        assert_eq!(
            store.fabric_messages("space-test").unwrap(),
            messages_before_reject
        );
        assert_eq!(
            store.fabric_message_deliveries("space-test").unwrap(),
            deliveries_before_reject
        );
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
        assert_eq!(
            deliveries[0].recipient_agent_member_id.as_deref(),
            Some("remote-recipient")
        );

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
            .migrate_legacy_agent_identity_same_id(
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
            binding: test_runtime_binding("session-runtime"),
            precondition: Default::default(),
            postcondition: Default::default(),
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
    fn agent_session_reattach_preserves_native_identity_and_fences_daemon_driver() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "reattach-agent", 0),
                identity("reattach-agent"),
            )
            .unwrap();
        let mut target = session("session-reattach", "reattach-agent");
        target.control_state.runtime_residency = RuntimeResidency::Attached;
        target.control_state.activity = RuntimeActivity::Idle;
        target.native_session_ref = Some(NativeSessionRef {
            provider: "codex".into(),
            execution_mode: "codex_app_server".into(),
            native_session_id: "thread-native-1".into(),
            native_locator_kind: "codex_thread".into(),
            provider_version: Some("0.148.0-alpha.9".into()),
            adapter_contract_version: "codex-app-server-v1".into(),
            availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
            supports_resume: true,
            last_verified_at: Some("t1".into()),
            parent_native_session_id: None,
        });
        store
            .create_agent_session(
                &service_context("session.create", "session-reattach", 0),
                target.clone(),
            )
            .unwrap();
        let now = current_unix_ms();
        store
            .drain_node_daemon_lease(&target.node_id, "daemon-1", 1, "instance-1", now, 60_000)
            .unwrap();
        store
            .release_node_daemon_lease(&target.node_id, "daemon-1", 1, "instance-1", now + 1)
            .unwrap();
        let successor = store
            .acquire_node_daemon_lease(&target.node_id, "daemon-2", "instance-2", now + 2, 60_000)
            .unwrap();
        let reattach_context = MutationContext {
            execution_space_id: "space-test".into(),
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: successor.daemon_id.clone(),
            },
            authority_actor: None,
            command_name: "node_daemon.session.reattach".into(),
            idempotency_key: "reattach-session-1".into(),
            expected_version: target.version,
            request_fingerprint: None,
        };
        let moved = store
            .reattach_agent_session_to_node_daemon(
                &reattach_context,
                &target.id,
                target.runtime_generation,
                1,
                &successor.daemon_id,
                successor.generation,
                "t2",
            )
            .unwrap();
        assert_eq!(
            moved.projection.runtime_generation,
            target.runtime_generation
        );
        assert_eq!(
            moved.projection.native_session_ref,
            target.native_session_ref
        );
        assert_eq!(
            moved.projection.node_daemon_generation,
            successor.generation
        );
        assert_eq!(moved.projection.control_state.driver_generation, 2);
        assert_eq!(
            moved.projection.control_state.runtime_residency,
            RuntimeResidency::Detached
        );
        assert_eq!(
            moved.projection.control_state.driver_ref,
            RuntimeDriverRef::NodeDaemon {
                node_daemon_id: successor.daemon_id,
                node_daemon_generation: successor.generation,
            }
        );
        let replay = store
            .reattach_agent_session_to_node_daemon(
                &reattach_context,
                &target.id,
                target.runtime_generation,
                1,
                "daemon-2",
                2,
                "t2",
            )
            .unwrap();
        assert!(replay.replayed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_session_reattach_rejects_expiry_without_provider_drain_receipt() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "expired-agent", 0),
                identity("expired-agent"),
            )
            .unwrap();
        let mut target = session("session-expired-reattach", "expired-agent");
        target.control_state.runtime_residency = RuntimeResidency::Attached;
        target.control_state.activity = RuntimeActivity::Idle;
        target.native_session_ref = Some(NativeSessionRef {
            provider: "codex".into(),
            execution_mode: "codex_app_server".into(),
            native_session_id: "thread-native-expired".into(),
            native_locator_kind: "codex_thread".into(),
            provider_version: Some("0.148.0-alpha.9".into()),
            adapter_contract_version: "codex-app-server-v1".into(),
            availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
            supports_resume: true,
            last_verified_at: Some("t1".into()),
            parent_native_session_id: None,
        });
        store
            .create_agent_session(
                &service_context("session.create", "session-expired-reattach", 0),
                target.clone(),
            )
            .unwrap();
        let successor = store
            .acquire_node_daemon_lease(
                &target.node_id,
                "daemon-2",
                "instance-2",
                current_unix_ms() + 61_000,
                60_000,
            )
            .unwrap();
        let before = store.canonical_operations().unwrap();
        let error = store
            .reattach_agent_session_to_node_daemon(
                &MutationContext {
                    execution_space_id: "space-test".into(),
                    authenticated_actor: ActorRef {
                        kind: ActorKind::Service,
                        id: successor.daemon_id.clone(),
                    },
                    authority_actor: None,
                    command_name: "node_daemon.session.reattach".into(),
                    idempotency_key: "reattach-expired-session".into(),
                    expected_version: target.version,
                    request_fingerprint: None,
                },
                &target.id,
                target.runtime_generation,
                1,
                &successor.daemon_id,
                successor.generation,
                "t2",
            )
            .expect_err("lease expiry is not a provider drain receipt");
        assert!(error
            .to_string()
            .contains("explicit predecessor NodeDaemon release"));
        assert_eq!(store.canonical_operations().unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupt_is_the_only_successor_admitted_while_start_cycle_is_in_flight() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "host",
                    "identity.create",
                    "identity-compensating-control",
                    0,
                ),
                identity("compensating-control"),
            )
            .unwrap();
        let session = session("session-compensating-control", "compensating-control");
        store
            .create_agent_session(
                &service_context("session.create", "session-compensating-control", 0),
                session.clone(),
            )
            .unwrap();
        store
            .transition_agent_session(
                &service_context("session.activate", "session-compensating-active", 1),
                &session.id,
                AgentSessionStatus::Active,
                "t-active",
            )
            .unwrap();

        let (start, start_context) = runtime_command_fixture(
            "runtime-compensating-start",
            RuntimeCommandKind::StartCycle,
            &session,
            "start_cycle",
        );
        store
            .prepare_runtime_command(&start_context, &start, current_unix_ms(), "t-start")
            .expect("StartCycle admission");

        let (interrupt, interrupt_context) = runtime_command_fixture(
            "runtime-compensating-interrupt",
            RuntimeCommandKind::InterruptCurrentCycle,
            &session,
            "interrupt_current_cycle",
        );
        let admitted = store
            .prepare_runtime_command(
                &interrupt_context,
                &interrupt,
                current_unix_ms(),
                "t-interrupt",
            )
            .expect("an exact interrupt must be able to compensate an in-flight StartCycle");
        assert_eq!(admitted.projection.status, RuntimeCommandStatus::Accepted);

        let (quiesce, quiesce_context) = runtime_command_fixture(
            "runtime-compensating-quiesce",
            RuntimeCommandKind::QuiesceExecutionLane,
            &session,
            "quiesce_execution_lane",
        );
        let error = store
            .prepare_runtime_command(&quiesce_context, &quiesce, current_unix_ms(), "t-quiesce")
            .expect_err("quiesce still requires the cycle and interrupt to become terminal");
        assert!(error.to_string().contains("reconciliation is required"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_command_replay_precedes_successor_fence_but_stale_settlement_is_zero_effect() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
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
            binding: test_runtime_binding("session-runtime-fence"),
            precondition: Default::default(),
            postcondition: Default::default(),
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
            .migrate_legacy_agent_identity_same_id(
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
            binding: test_runtime_binding("session-runtime-control"),
            precondition: Default::default(),
            postcondition: Default::default(),
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
            agent_member_id: "runtime-control".into(),
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
            binding: test_runtime_binding("session-runtime-control"),
            precondition: Default::default(),
            postcondition: Default::default(),
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
            .migrate_legacy_agent_identity_same_id(
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
            binding: test_runtime_binding("session-runtime-stop"),
            precondition: Default::default(),
            postcondition: Default::default(),
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
                .migrate_legacy_agent_identity_same_id(
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
                .migrate_legacy_agent_identity_same_id(
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
            .migrate_legacy_agent_identity_same_id(
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
                .migrate_legacy_agent_identity_same_id(
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
            binding: test_runtime_binding("session-runtime-widened"),
            precondition: Default::default(),
            postcondition: Default::default(),
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
            .migrate_legacy_agent_identity_same_id(
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
            binding: test_runtime_binding("session-standalone"),
            precondition: Default::default(),
            postcondition: Default::default(),
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
                    && membership.agent_member_id == "standalone-agent"
                    && membership.state == TeamMembershipStatus::Active
            }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn team_host_cannot_stop_shared_session_and_active_bindings_require_explicit_release() {
        let (store, root) = fabric_store();
        for identity_id in ["shared-agent", "host-a", "host-b"] {
            store
                .migrate_legacy_agent_identity_same_id(
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
        assert_eq!(
            store
                .team_host_membership("space-test", "team-a", true)
                .unwrap()
                .agent_member_id,
            "host-a"
        );
        assert_eq!(
            store
                .team_host_membership("space-test", "team-b", true)
                .unwrap()
                .agent_member_id,
            "host-b"
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
                        agent_member_id: "shared-agent".into(),
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
    fn runtime_command_exact_binding_rejects_stale_fields_before_acceptance() {
        for field in ["driver", "composition", "capability"] {
            let (store, root) = fabric_store();
            let identity_id = format!("binding-{field}");
            let session_id = format!("session-binding-{field}");
            store
                .migrate_legacy_agent_identity_same_id(
                    &context("host", "identity.create", &identity_id, 0),
                    identity(&identity_id),
                )
                .unwrap();
            let target = session(&session_id, &identity_id);
            store
                .create_agent_session(
                    &service_context("session.create", &session_id, 0),
                    target.clone(),
                )
                .unwrap();
            let (mut command, mut admission) = runtime_command_fixture(
                &format!("binding-command-{field}"),
                RuntimeCommandKind::OpenRuntime,
                &target,
                "open_runtime",
            );
            match field {
                "driver" => command.binding.target_driver_generation = Some(2),
                "composition" => {
                    command.binding.composition_fingerprint = Some("composition:stale".into())
                }
                "capability" => {
                    command.binding.capability_fingerprint = Some("capability:stale".into())
                }
                _ => unreachable!(),
            }
            admission.request_fingerprint =
                Some(runtime_command_envelope_fingerprint(&command).unwrap());
            let before = store.canonical_operations().unwrap();
            let error = store
                .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-rejected")
                .expect_err("a stale exact-binding field must be fenced before Accepted");
            assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
            assert_eq!(store.canonical_operations().unwrap(), before, "{field}");
            assert!(store.runtime_commands("space-test").unwrap().is_empty());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn runtime_command_team_supervisor_generation_is_live_fenced_at_prepare_and_settle() {
        let (store, root) = fabric_store();
        append_runtime_team(&store, "team-supervisor", "run-supervisor");
        let lease = store
            .acquire_team_supervisor_under_node_lease(
                "run-supervisor",
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
                "space-test",
                "project-1",
                "supervisor-1",
                std::process::id(),
                "loopback://supervisor-1",
                current_unix_ms(),
                60_000,
            )
            .unwrap();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "supervised-agent", 0),
                identity("supervised-agent"),
            )
            .unwrap();
        let mut target = session("session-supervised", "supervised-agent");
        target.control_state.driver_ref = RuntimeDriverRef::TeamSupervisor {
            team_run_id: "run-supervisor".into(),
            team_supervisor_id: lease.supervisor_id.clone(),
            team_supervisor_generation: lease.generation,
        };
        store
            .create_agent_session(
                &service_context("session.create", "session-supervised", 0),
                target.clone(),
            )
            .unwrap();

        let (command, admission) = runtime_command_fixture(
            "supervisor-live-command",
            RuntimeCommandKind::OpenRuntime,
            &target,
            "open_runtime",
        );
        let accepted = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
            .unwrap();
        assert_eq!(
            accepted.projection.request_fingerprint,
            runtime_command_envelope_fingerprint(&command).unwrap(),
            "RuntimeCommandRecord must snapshot the full command, not only payload"
        );
        store
            .release_team_supervisor_lease(
                "run-supervisor",
                &lease.supervisor_id,
                lease.generation,
                current_unix_ms(),
            )
            .unwrap();
        let before_settle = store.canonical_operations().unwrap();
        let error = store
            .settle_runtime_command(
                &service_context(
                    "node_daemon.runtime.settle",
                    "supervisor-live-command:settle",
                    accepted.projection.version,
                ),
                &command.id,
                RuntimeCommandStatus::Applied,
                RuntimeEffectCertainty::Applied,
                Some(serde_json::json!({"provider_receipt": "must-not-land"})),
                None,
                "t-settle",
            )
            .expect_err("a released Supervisor cannot settle Applied");
        assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), before_settle);

        let successor = store
            .acquire_team_supervisor_under_node_lease(
                "run-supervisor",
                "11111111-1111-4111-8111-111111111111",
                "daemon-1",
                1,
                "space-test",
                "project-1",
                "supervisor-2",
                std::process::id(),
                "loopback://supervisor-2",
                current_unix_ms(),
                60_000,
            )
            .unwrap();

        let mut stale = session("session-stale-supervisor", "supervised-agent");
        stale.id = "session-stale-supervisor".into();
        stale.agent_member_id = "another-supervised-agent".into();
        stale.control_state.driver_ref = RuntimeDriverRef::TeamSupervisor {
            team_run_id: "run-supervisor".into(),
            team_supervisor_id: successor.supervisor_id,
            team_supervisor_generation: successor.generation.saturating_add(1),
        };
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "another-supervised-agent", 0),
                identity("another-supervised-agent"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "session-stale-supervisor", 0),
                stale.clone(),
            )
            .unwrap();
        let (stale_command, stale_admission) = runtime_command_fixture(
            "supervisor-stale-command",
            RuntimeCommandKind::OpenRuntime,
            &stale,
            "open_runtime",
        );
        let before_prepare = store.canonical_operations().unwrap();
        let error = store
            .prepare_runtime_command(
                &stale_admission,
                &stale_command,
                current_unix_ms(),
                "t-stale",
            )
            .expect_err("a stale Supervisor generation must not reach Accepted");
        assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), before_prepare);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_session_json_is_readable_but_cannot_admit_an_unbound_new_effect() {
        let (store, root) = fabric_store();
        let mut legacy_json =
            serde_json::to_value(session("legacy-session", "legacy-agent")).unwrap();
        let legacy_object = legacy_json.as_object_mut().unwrap();
        legacy_object.remove("control_state");
        let legacy_identity = legacy_object
            .remove("agent_member_id")
            .expect("canonical AgentMember field");
        legacy_object.insert("agent_identity_id".into(), legacy_identity);
        let legacy: AgentSession = serde_json::from_value(legacy_json).unwrap();
        assert_eq!(legacy.control_state.driver_generation, 0);
        assert_eq!(legacy.control_state.driver_ref, RuntimeDriverRef::Unknown);
        let rewritten = serde_json::to_value(&legacy).unwrap();
        assert_eq!(rewritten["agent_member_id"], "legacy-agent");
        assert!(rewritten.get("agent_identity_id").is_none());
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "legacy-agent", 0),
                identity("legacy-agent"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "legacy-session", 0),
                legacy.clone(),
            )
            .unwrap();
        let (mut command, mut admission) = runtime_command_fixture(
            "legacy-unbound-command",
            RuntimeCommandKind::OpenRuntime,
            &legacy,
            "open_runtime",
        );
        command.binding = Default::default();
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let before = store.canonical_operations().unwrap();
        let error = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-rejected")
            .expect_err("legacy readability must not become new effect authority");
        assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), before);
        assert!(store.runtime_commands("space-test").unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn provider_continuation_driver_requires_exact_active_armed_generation() {
        for case in ["exact", "disarmed", "revision"] {
            let (store, root) = fabric_store();
            let identity_id = format!("continuation-{case}");
            let session_id = format!("session-continuation-{case}");
            store
                .migrate_legacy_agent_identity_same_id(
                    &context("host", "identity.create", &identity_id, 0),
                    identity(&identity_id),
                )
                .unwrap();
            let mut target = session(&session_id, &identity_id);
            target.control_state.execution_driver = MemberExecutionDriver::ProviderDriven;
            target.control_state.driver_generation = 7;
            target.control_state.driver_ref = RuntimeDriverRef::ProviderContinuation {
                provider: "codex".into(),
                continuation_id: "native-goal-1".into(),
                continuation_revision: Some(3),
                runtime_generation: 1,
            };
            target
                .control_state
                .continuation
                .definition
                .continuation_ref = Some("native-goal-1".into());
            target.control_state.continuation.definition.revision =
                Some(if case == "revision" { 4 } else { 3 });
            target.control_state.continuation.definition.phase = NativeContinuationPhase::Active;
            target.control_state.continuation.activation = if case == "disarmed" {
                NativeContinuationActivation::Disarmed
            } else {
                NativeContinuationActivation::Armed {
                    runtime_generation: 1,
                    driver_generation: 7,
                }
            };
            store
                .create_agent_session(
                    &service_context("session.create", &session_id, 0),
                    target.clone(),
                )
                .unwrap();
            if case == "exact" {
                let (start_cycle, start_admission) = runtime_command_fixture(
                    "continuation-must-not-host-start",
                    RuntimeCommandKind::StartCycle,
                    &target,
                    "start_cycle",
                );
                let before_start = store.canonical_operations().unwrap();
                let error = store
                    .prepare_runtime_command(
                        &start_admission,
                        &start_cycle,
                        current_unix_ms(),
                        "t-start-rejected",
                    )
                    .expect_err(
                        "an armed provider continuation must remain the only next-cycle driver",
                    );
                assert!(error.to_string().contains(
                    "cannot start a provider cycle while the AgentSession is provider-driven"
                ));
                assert_eq!(store.canonical_operations().unwrap(), before_start);
                assert!(store.runtime_commands("space-test").unwrap().is_empty());
            }
            let (command, admission) = runtime_command_fixture(
                &format!("continuation-command-{case}"),
                RuntimeCommandKind::InspectContinuation,
                &target,
                "inspect_continuation",
            );
            let before = store.canonical_operations().unwrap();
            let result =
                store.prepare_runtime_command(&admission, &command, current_unix_ms(), "t");
            if case == "exact" {
                assert_eq!(
                    result.unwrap().projection.status,
                    RuntimeCommandStatus::Accepted
                );
            } else {
                let error = result.expect_err("continuation fence must reject mismatch");
                assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
                assert_eq!(store.canonical_operations().unwrap(), before);
            }
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn runtime_command_semantic_preconditions_are_lock_checked_with_zero_side_effects() {
        for case in [
            "session_version",
            "residency",
            "activity",
            "execution_driver",
            "cycle_ref",
            "continuation_ref",
            "continuation_phase",
            "runtime_idle",
            "execution_lane_quiesced",
        ] {
            let (store, root) = fabric_store();
            let identity_id = format!("precondition-{case}");
            let session_id = format!("session-precondition-{case}");
            store
                .migrate_legacy_agent_identity_same_id(
                    &context("host", "identity.create", &identity_id, 0),
                    identity(&identity_id),
                )
                .unwrap();
            let target = session(&session_id, &identity_id);
            store
                .create_agent_session(
                    &service_context("session.create", &session_id, 0),
                    target.clone(),
                )
                .unwrap();
            let (mut command, mut admission) = runtime_command_fixture(
                &format!("precondition-{case}"),
                RuntimeCommandKind::OpenRuntime,
                &target,
                "open_runtime",
            );
            match case {
                "session_version" => command.precondition.expected_session_version = Some(2),
                "residency" => {
                    command.precondition.expected_residency = Some(RuntimeResidency::Attached)
                }
                "activity" => {
                    command.precondition.expected_activity = Some(RuntimeActivity::Running)
                }
                "execution_driver" => {
                    command.precondition.expected_execution_driver =
                        Some(MemberExecutionDriver::ProviderDriven)
                }
                "cycle_ref" => {
                    command.precondition.expected_cycle_ref =
                        Some(firm_core::agentfirm_api::RuntimeNativeObjectRef {
                            id: "missing-cycle".into(),
                            revision: None,
                            fingerprint: None,
                        })
                }
                "continuation_ref" => {
                    command.precondition.expected_continuation_ref =
                        Some(firm_core::agentfirm_api::RuntimeNativeObjectRef {
                            id: "missing-continuation".into(),
                            revision: None,
                            fingerprint: None,
                        })
                }
                "continuation_phase" => {
                    command.precondition.expected_continuation_phase =
                        Some(NativeContinuationPhase::Active)
                }
                "runtime_idle" => {
                    command.precondition.safe_point = RuntimeSafePointRequirement::RuntimeIdle
                }
                "execution_lane_quiesced" => {
                    command.precondition.safe_point =
                        RuntimeSafePointRequirement::ExecutionLaneQuiesced
                }
                _ => unreachable!(),
            }
            admission.request_fingerprint =
                Some(runtime_command_envelope_fingerprint(&command).unwrap());
            let before = store.canonical_operations().unwrap();
            let error = store
                .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-rejected")
                .expect_err("an unproven semantic precondition must reject before admission");
            assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
            assert_eq!(store.canonical_operations().unwrap(), before, "{case}");
            assert!(store.runtime_commands("space-test").unwrap().is_empty());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn runtime_command_settlement_rechecks_the_prepared_semantic_snapshot() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "settle-precondition", 0),
                identity("settle-precondition"),
            )
            .unwrap();
        let target = session("session-settle-precondition", "settle-precondition");
        store
            .create_agent_session(
                &service_context("session.create", "session-settle-precondition", 0),
                target.clone(),
            )
            .unwrap();
        let (mut command, mut admission) = runtime_command_fixture(
            "settle-precondition-command",
            RuntimeCommandKind::OpenRuntime,
            &target,
            "open_runtime",
        );
        command.precondition.expected_session_version = Some(target.version);
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let accepted = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
            .unwrap();
        store
            .transition_agent_session(
                &service_context("session.activate", "settle-precondition:activate", 1),
                &target.id,
                AgentSessionStatus::Active,
                "t-active",
            )
            .unwrap();
        let before_settle = store.canonical_operations().unwrap();
        let error = store
            .settle_runtime_command(
                &service_context(
                    "node_daemon.runtime.settle",
                    "settle-precondition:settle",
                    accepted.projection.version,
                ),
                &command.id,
                RuntimeCommandStatus::Applied,
                RuntimeEffectCertainty::Applied,
                Some(serde_json::json!({"provider_receipt": "stale"})),
                None,
                "t-settle",
            )
            .expect_err("settlement must not bless a command whose semantic snapshot drifted");
        assert!(error.to_string().contains("expected_session_version"));
        assert_eq!(store.canonical_operations().unwrap(), before_settle);
        assert_eq!(
            store.runtime_commands("space-test").unwrap()[0].status,
            RuntimeCommandStatus::Accepted
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_cannot_confirm_applied_after_semantic_precondition_drift() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "recovery-precondition", 0),
                identity("recovery-precondition"),
            )
            .unwrap();
        let target = session("session-recovery-precondition", "recovery-precondition");
        store
            .create_agent_session(
                &service_context("session.create", "session-recovery-precondition", 0),
                target.clone(),
            )
            .unwrap();
        let (mut command, mut admission) = runtime_command_fixture(
            "recovery-precondition-command",
            RuntimeCommandKind::StopSession,
            &target,
            "stop_session",
        );
        command.precondition.expected_session_version = Some(target.version);
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
            .unwrap();
        store
            .settle_runtime_command(
                &service_context(
                    "node_daemon.runtime.settle",
                    "recovery-precondition:settle",
                    1,
                ),
                &command.id,
                RuntimeCommandStatus::RecoveryRequired,
                RuntimeEffectCertainty::Unknown,
                None,
                Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
                "t-recovery",
            )
            .unwrap();
        store
            .transition_agent_session(
                &service_context("session.activate", "recovery-precondition:activate", 1),
                &target.id,
                AgentSessionStatus::Active,
                "t-active",
            )
            .unwrap();

        let mut confirm_applied = service_context(
            "operator.runtime.resolve",
            "recovery-precondition:confirm-applied",
            2,
        );
        confirm_applied.authority_actor = Some(ActorRef {
            kind: ActorKind::Service,
            id: target.node_id.clone(),
        });
        let before = store.canonical_operations().unwrap();
        let error = store
            .resolve_runtime_command_recovery(
                &confirm_applied,
                &command.id,
                &target.node_id,
                &target.node_daemon_id,
                target.node_daemon_generation,
                RuntimeRecoveryResolution::ConfirmApplied,
                "evidence:provider-claimed-applied",
                "t-confirm-applied",
            )
            .expect_err("stale semantics cannot be promoted to Applied during recovery");
        assert!(error.to_string().contains("expected_session_version"));
        assert_eq!(store.canonical_operations().unwrap(), before);

        let mut confirm_not_applied = service_context(
            "operator.runtime.resolve",
            "recovery-precondition:confirm-not-applied",
            2,
        );
        confirm_not_applied.authority_actor = Some(ActorRef {
            kind: ActorKind::Service,
            id: target.node_id.clone(),
        });
        let resolved = store
            .resolve_runtime_command_recovery(
                &confirm_not_applied,
                &command.id,
                &target.node_id,
                &target.node_daemon_id,
                target.node_daemon_generation,
                RuntimeRecoveryResolution::ConfirmNotApplied,
                "evidence:provider-absent",
                "t-confirm-not-applied",
            )
            .expect("stale work must remain safely resolvable as NotApplied");
        assert_eq!(resolved.projection.status, RuntimeCommandStatus::Failed);
        assert_eq!(
            resolved.projection.effect_certainty,
            RuntimeEffectCertainty::NotApplied
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn control_state_binding_is_quiescent_generation_fenced_and_exactly_replayable() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "control-bind-agent", 0),
                identity("control-bind-agent"),
            )
            .unwrap();
        let mut target = session("session-control-bind", "control-bind-agent");
        target.control_state.runtime_residency = RuntimeResidency::Detached;
        target.control_state.activity = RuntimeActivity::Idle;
        store
            .create_agent_session(
                &service_context("session.create", "session-control-bind", 0),
                target.clone(),
            )
            .unwrap();
        let mut next = target.control_state.clone();
        next.driver_generation = 2;
        next.composition_fingerprint = Some("composition:v2".into());
        let mutation = service_context("session.control.bind", "control-bind", 1);
        let first = store
            .bind_agent_session_control_state(
                &mutation,
                &target.id,
                target.runtime_generation,
                next.clone(),
                "t2",
            )
            .unwrap();
        assert_eq!(first.projection.control_state.driver_generation, 2);
        let before_replay = store.canonical_operations().unwrap();
        let replay = store
            .bind_agent_session_control_state(
                &mutation,
                &target.id,
                target.runtime_generation,
                next,
                "t2",
            )
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(store.canonical_operations().unwrap(), before_replay);

        let error = store
            .bind_agent_session_control_state(
                &service_context("session.control.bind", "control-bind-stale", 2),
                &target.id,
                target.runtime_generation.saturating_add(1),
                first.projection.control_state,
                "t3",
            )
            .expect_err("stale runtime generation must not mutate control state");
        assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), before_replay);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_session_rejects_every_provider_runtime_effect_with_zero_delta() {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
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
