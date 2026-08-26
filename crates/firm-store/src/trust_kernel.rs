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
    CanonicalOperation, CanonicalWorkDelivery, ControlCommandEnvelope, DeliveryReconcileOutcome,
    FailureAnalysis, GateEvaluation, GateRequirement, GateRequirementSource, GateVerdict,
    GateWaiver, GateWaiverState, MemberCoordinationStatus, MemberExecutionDriver, MemberRun,
    MemberRuntimeStatus, MemberWorkspaceBinding, Message, MessageRecipientKind, MessageSubjectKind,
    MessageSubscription, MessageSubscriptionKind, MessageSubscriptionStatus, MutationContext,
    NativeContinuationActivation, NativeContinuationPhase, NativeSessionRef, PermissionCeiling,
    ProviderCycleCorrelation, ProviderInvocation, RuntimeActivity, RuntimeCommandKind,
    RuntimeCommandPhase, RuntimeCommandPrecondition, RuntimeCommandRecord, RuntimeCommandStatus,
    RuntimeDriverRef, RuntimeEffectCertainty, RuntimePostconditionStatus,
    RuntimeRecoveryResolution, RuntimeResidency, RuntimeSafePointRequirement, SubscriptionCursor,
    TeamMembership, TeamMembershipRole, TeamMembershipStatus, TeamMessageDeliveryClaim, TrustError,
    TrustErrorCode, WorkDeliveryStatus, WorkExecutionBinding, WorkExecutionBindingStatus,
    WorkFinding, WorkModuleBinding, WorkModuleId, WorkReport, WorkReportKind, WorkspaceLifecycle,
    WorkspaceMode, WorkspaceOwnership, WorkspaceSafetyProof,
};
use firm_core::collaboration::{
    CollaborationMessageAuthority, MessageAdmissionAuthority, PeerTeamMessageAdmissionAuthority,
};
use firm_core::{
    AgentTeam, AgentTeamStatus, ExecutionNodeStatus, HostAttention, HostAttentionKind,
    HostAttentionStatus, MemberCoordinationStatus as LegacyMemberCoordinationStatus,
    MemberRunStatus, ProviderRuntimeProjection, TeamActorKind, TeamActorRef, Validate, Work,
    WorkCommandContext, WorkDelegationRevision,
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

/// Fail-closed parity validation between the legacy ProviderRuntimeProjection
/// and the canonical MemberRun. Rows written before the DOC-108 cutover
/// legitimately materialized with a canonical `None` where the legacy
/// projection kept a `Some`; for ANY comparable optional lifecycle field that
/// exact shape (canonical `None`, legacy `Some`) is a known migration fact,
/// not corruption, so validation adopts the legacy value for it (the field is
/// skipped) through the field-generic [`optional_field_mismatch`] rule. Every
/// other divergence — including both-`Some` disagreement — still fails closed
/// for every field, and scalar lifecycle fields always compare strictly.
///
/// Sync decisions keep using the strict
/// [`current_member_lifecycle_mismatch_fields`]: any real difference still
/// heals the canonical projection from the legacy row on the next mutation.
pub(crate) fn current_member_lifecycle_validation_mismatch_fields(
    canonical: &MemberRun,
    runtime: &ProviderRuntimeProjection,
) -> StoreResult<Vec<&'static str>> {
    lifecycle_mismatch_fields(canonical, runtime, PreCutoverTolerance::AdoptLegacy)
}

pub(crate) fn current_member_lifecycle_mismatch_fields(
    canonical: &MemberRun,
    runtime: &ProviderRuntimeProjection,
) -> StoreResult<Vec<&'static str>> {
    lifecycle_mismatch_fields(canonical, runtime, PreCutoverTolerance::FailClosed)
}

/// How a canonical-`None` + legacy-`Some` divergence on an optional lifecycle
/// field is treated. That shape is the known DOC-108 pre-cutover migration
/// artifact and can only occur on optional fields, never on scalar ones.
#[derive(Clone, Copy)]
enum PreCutoverTolerance {
    /// The row predates the cutover: adopt the legacy value (skip the field).
    AdoptLegacy,
    /// Any divergence fails closed.
    FailClosed,
}

/// Shared parity comparison between the legacy ProviderRuntimeProjection and
/// the canonical MemberRun. Every comparable optional lifecycle field runs
/// through [`optional_field_mismatch`], which applies the field-generic
/// pre-cutover tolerance; scalar fields always compare strictly.
fn lifecycle_mismatch_fields(
    canonical: &MemberRun,
    runtime: &ProviderRuntimeProjection,
    pre_cutover: PreCutoverTolerance,
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
    if let Some(field) = optional_field_mismatch(
        "native_session",
        &canonical.native_session,
        &canonical_native,
        pre_cutover,
    ) {
        mismatches.push(field);
    }
    if canonical.started_at != runtime.started_at {
        mismatches.push("started_at");
    }
    if let Some(field) = optional_field_mismatch(
        "last_event_at",
        &canonical.last_event_at,
        &runtime.last_event_at,
        pre_cutover,
    ) {
        mismatches.push(field);
    }
    if let Some(field) = optional_field_mismatch(
        "finished_at",
        &canonical.finished_at,
        &runtime.finished_at,
        pre_cutover,
    ) {
        mismatches.push(field);
    }
    Ok(mismatches)
}

/// Field-generic mismatch check for one optional lifecycle field. With
/// [`PreCutoverTolerance::AdoptLegacy`], a canonical `None` paired with a
/// legacy `Some` is the known pre-cutover migration shape for ANY field and is
/// skipped (the legacy value is adopted). Every other divergence — including
/// both-`Some` disagreement — reports the field.
fn optional_field_mismatch<T: PartialEq>(
    field: &'static str,
    canonical: &Option<T>,
    legacy: &Option<T>,
    pre_cutover: PreCutoverTolerance,
) -> Option<&'static str> {
    if matches!(pre_cutover, PreCutoverTolerance::AdoptLegacy)
        && canonical.is_none()
        && legacy.is_some()
    {
        return None;
    }
    (canonical != legacy).then_some(field)
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
    firm_core::agentfirm_api::message_content_fingerprint(message)
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

mod fabric_foundation;
mod fabric_identity_sessions;
mod fabric_message_authoring;
mod fabric_message_delivery;
mod fabric_runtime_commands;
mod fabric_teams;
mod fabric_work_execution;
mod trust_deliveries;
mod trust_foundation;
mod trust_members;
mod trust_work_evidence;
mod trust_workspace;

#[cfg(test)]
#[path = "trust_kernel_tests/mod.rs"]
mod tests;
