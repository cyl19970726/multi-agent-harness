//! Canonical Member Execution Trust Kernel wire contracts.
//!
//! These closed types intentionally live behind one module while the Wave 4A
//! clean cutover removes the former runtime-heavy identity and embedded
//! delivery/gate records from the root module.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    AgentMember,
    External,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorRef {
    pub kind: ActorKind,
    pub id: String,
}

/// Stable organizational identity. Provider-native state never lives here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentIdentity {
    pub id: String,
    pub display_name: String,
    pub organization_status: AgentMemberOrganizationStatus,
    pub permission_ceiling: PermissionCeiling,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionCeiling {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMemberOrganizationStatus {
    Active,
    Paused,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMember {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub skill_refs: Vec<String>,
    #[serde(default)]
    pub provider_profile_ref: Option<String>,
    #[serde(default)]
    pub model_preference: Option<String>,
    pub workspace_policy: String,
    pub permission_ceiling: PermissionCeiling,
    pub organization_status: AgentMemberOrganizationStatus,
    pub version: u64,
    pub created_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSessionAvailability {
    Available,
    Stale,
    Missing,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionStatus {
    Starting,
    Idle,
    Running,
    Waiting,
    Disconnected,
    Stopped,
    Failed,
}

/// One machine-local provider session owned by an exact NodeDaemon generation.
/// Team membership is deliberately absent: a session can outlive or move
/// between collaboration overlays without changing its identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSession {
    pub id: String,
    pub agent_identity_id: String,
    pub node_id: String,
    pub execution_space_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub provider: String,
    pub provider_profile_ref: String,
    pub effective_permission_ceiling: PermissionCeiling,
    pub status: AgentSessionStatus,
    pub generation: u64,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMembershipStatus {
    Active,
    Left,
    Revoked,
}

/// Collaboration membership. It binds a stable identity to one Team, not a
/// provider process or native session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMembership {
    pub id: String,
    pub team_id: String,
    pub team_run_id: String,
    pub agent_identity_id: String,
    pub node_id: String,
    pub role_snapshot: String,
    pub status: TeamMembershipStatus,
    pub version: u64,
    pub joined_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkExecutionBindingStatus {
    Active,
    Released,
    Completed,
    Cancelled,
}

/// Exact accountable binding from Work to identity + membership + current
/// machine-local session generation. A successor session never inherits Work
/// authority implicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkExecutionBinding {
    pub id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub team_membership_id: String,
    pub agent_identity_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub status: WorkExecutionBindingStatus,
    pub version: u64,
    pub bound_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionRef {
    pub provider: String,
    pub execution_mode: String,
    pub native_session_id: String,
    pub native_locator_kind: String,
    #[serde(default)]
    pub provider_version: Option<String>,
    pub adapter_contract_version: String,
    pub availability: NativeSessionAvailability,
    pub supports_resume: bool,
    #[serde(default)]
    pub last_verified_at: Option<String>,
    #[serde(default)]
    pub parent_native_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberCoordinationStatus {
    Active,
    Closed,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRuntimeStatus {
    Starting,
    Idle,
    Queued,
    Running,
    Waiting,
    Disconnected,
    Reviewing,
    Blocked,
    Completed,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberRun {
    pub id: String,
    pub agent_member_id: String,
    pub team_run_id: String,
    pub role_snapshot: String,
    #[serde(default)]
    pub provider_profile_snapshot: Option<String>,
    #[serde(default)]
    pub requested_controls: serde_json::Value,
    #[serde(default)]
    pub effective_controls: serde_json::Value,
    pub coordination_status: MemberCoordinationStatus,
    pub runtime_status: MemberRuntimeStatus,
    pub runtime_generation: u64,
    #[serde(default)]
    pub workspace_binding_id: Option<String>,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    pub version: u64,
    pub started_at: String,
    #[serde(default)]
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageKind {
    Message,
    Control,
    ProviderInteractionRequest,
    ProviderInteractionResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseIntent {
    Informational,
    ResponseRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMessage {
    pub id: String,
    pub team_run_id: String,
    #[serde(default)]
    pub work_id: Option<String>,
    pub sender: ActorRef,
    pub recipients: Vec<ActorRef>,
    pub kind: TeamMessageKind,
    pub body: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    pub response_intent: ResponseIntent,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub created_at: String,
}

/// Canonical message kind. Runtime control and Work delivery are intentionally
/// excluded so neither plane can smuggle executable authority through chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    Reply,
    RequestDecision,
    ProviderInteractionRequest,
    ProviderInteractionResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRecipientKind {
    AgentIdentity,
    Team,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRecipientRef {
    pub kind: MessageRecipientKind,
    pub id: String,
}

/// Immutable source-authored message. `author_node_*` is frozen by the source
/// NodeDaemon and is never rewritten by the Company control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub execution_space_id: String,
    pub author_node_id: String,
    pub author_node_daemon_id: String,
    pub author_node_daemon_generation: u64,
    pub sender_identity_id: String,
    pub recipients: Vec<MessageRecipientRef>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub team_run_id: Option<String>,
    #[serde(default)]
    pub work_id: Option<String>,
    pub kind: MessageKind,
    pub body: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    pub response_intent: ResponseIntent,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub content_fingerprint: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSubscriptionKind {
    Direct,
    Team,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSubscriptionStatus {
    Active,
    Revoked,
}

/// Durable routing policy. Consumption progress is held separately in
/// [`SubscriptionCursor`] so changing a policy cannot rewrite inbox history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageSubscription {
    pub id: String,
    pub recipient_identity_id: String,
    pub execution_space_id: String,
    pub kind: MessageSubscriptionKind,
    #[serde(default)]
    pub team_membership_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
    pub status: MessageSubscriptionStatus,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionCursor {
    pub id: String,
    pub subscription_id: String,
    pub recipient_identity_id: String,
    pub last_message_sequence: u64,
    pub version: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalMessageDeliveryStatus {
    Queued,
    Routed,
    Claimed,
    ProviderReceived,
    Acknowledged,
    Failed,
    Expired,
    Invalidated,
}

/// Per-recipient inbox/delivery truth, owned by the target NodeDaemon. The
/// recipient session remains absent while no unique current session exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalMessageDelivery {
    pub id: String,
    pub message_id: String,
    pub subscription_id: String,
    pub recipient_identity_id: String,
    pub target_node_id: String,
    #[serde(default)]
    pub recipient_session_id: Option<String>,
    #[serde(default)]
    pub recipient_session_generation: Option<u64>,
    pub status: CanonicalMessageDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_node_daemon_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub failure_detail: Option<String>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteJournalStatus {
    Pending,
    Routed,
    Received,
    Failed,
}

/// Cross-node route metadata only. It contains no provider/session ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRouteJournal {
    pub id: String,
    pub message_id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub target_execution_space_id: String,
    pub attempt: u32,
    pub status: RouteJournalStatus,
    #[serde(default)]
    pub receipt_id: Option<String>,
    pub version: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommandKind {
    StartSession,
    StopSession,
    ResumeSession,
    DispatchProvider,
    CancelProviderTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDispatchMode {
    QueueOnly,
    StartIfIdle,
    InjectIfSafe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlCommandEnvelope {
    pub id: String,
    pub execution_space_id: String,
    pub target_node_id: String,
    pub target_node_daemon_id: String,
    pub target_node_daemon_generation: u64,
    pub authenticated_actor: ActorRef,
    pub command: RuntimeCommandKind,
    pub required_capability: String,
    pub idempotency_key: String,
    pub expected_version: u64,
    pub expires_unix_ms: u64,
    pub payload: serde_json::Value,
    pub payload_fingerprint: String,
    pub issued_at: String,
}

/// The only provider-facing dispatch shape after Wave 4C. It is created by the
/// target NodeDaemon from a claimed canonical delivery; public callers cannot
/// author or claim it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDispatchEnvelope {
    pub id: String,
    pub source_plane: String,
    pub source_record_id: String,
    pub recipient_identity_id: String,
    pub recipient_session_id: String,
    pub recipient_session_generation: u64,
    pub node_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub provider: String,
    pub dispatch_mode: RuntimeDispatchMode,
    pub permission_ceiling: PermissionCeiling,
    pub content: String,
    pub content_fingerprint: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDeliveryStatus {
    Queued,
    Claimed,
    ProviderReceived,
    Acknowledged,
    Failed,
    Expired,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageDelivery {
    pub id: String,
    pub message_id: String,
    pub recipient_member_run_id: String,
    pub status: MessageDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_supervisor_generation: Option<u64>,
    #[serde(default)]
    pub claimed_member_generation: Option<u64>,
    #[serde(default)]
    pub claim_expires_at: Option<String>,
    #[serde(default)]
    pub freeze_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub failure_detail: Option<String>,
    pub version: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDeliveryStatus {
    Queued,
    Claimed,
    ProviderReceived,
    Failed,
    Expired,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDelivery {
    pub id: String,
    pub work_event_id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub recipient_member_run_id: String,
    pub status: WorkDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_supervisor_generation: Option<u64>,
    #[serde(default)]
    pub claimed_member_generation: Option<u64>,
    #[serde(default)]
    pub claim_expires_at: Option<String>,
    #[serde(default)]
    pub freeze_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub failure_detail: Option<String>,
    pub version: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryClaim {
    pub claim_id: String,
    pub supervisor_generation: u64,
    pub member_generation: u64,
    pub claim_expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReceipt {
    pub claim_id: String,
    pub supervisor_generation: u64,
    pub member_generation: u64,
    pub provider_receipt_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryReconcileOutcome {
    Acknowledged,
    RetrySafeFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    Worktree,
    IsolatedSnapshot,
    SharedLive,
    Inherit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceOwnership {
    Managed,
    AttachedExternal,
    SharedProject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLifecycle {
    Requested,
    Preparing,
    Ready,
    Attached,
    Dirty,
    Conflicted,
    Missing,
    Archived,
    CleanupBlocked,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberWorkspaceBinding {
    pub id: String,
    pub project_binding_id: String,
    pub team_run_id: String,
    pub member_run_id: String,
    #[serde(default)]
    pub work_id: Option<String>,
    pub mode: WorkspaceMode,
    pub ownership: WorkspaceOwnership,
    pub canonical_root: String,
    #[serde(default)]
    pub git_common_dir: Option<String>,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub git_head: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub dirty_fingerprint: Option<String>,
    #[serde(default)]
    pub instruction_roots: Vec<String>,
    #[serde(default)]
    pub skill_roots: Vec<String>,
    pub lifecycle: WorkspaceLifecycle,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub attached_member_generation: Option<u64>,
    pub version: u64,
    pub created_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSafetyProof {
    pub canonical_root: String,
    pub project_binding_id: String,
    #[serde(default)]
    pub git_common_dir: Option<String>,
    pub link_escape_free: bool,
    pub repository_matches: bool,
    pub is_dirty: bool,
    pub is_conflicted: bool,
    pub observed_member_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    GitCommit,
    ArtifactDigest,
    ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRef {
    pub kind: CandidateKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkReportKind {
    Progress,
    Result,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkReport {
    pub id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub report_revision: u64,
    pub kind: WorkReportKind,
    pub authored_by: ActorRef,
    pub summary: String,
    #[serde(default)]
    pub base_revision: Option<String>,
    #[serde(default)]
    pub candidate: Option<CandidateRef>,
    #[serde(default)]
    pub candidate_fingerprint: Option<String>,
    #[serde(default)]
    pub finding_refs: Vec<String>,
    #[serde(default)]
    pub failure_analysis_ref: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub check_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub known_risks: Vec<String>,
    #[serde(default)]
    pub confidence: Option<Confidence>,
    #[serde(default)]
    pub recommended_next_action: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkFindingKind {
    Discovery,
    Difficulty,
    Decision,
    Risk,
    ReusablePattern,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkFinding {
    pub id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub kind: WorkFindingKind,
    pub summary: String,
    pub detail_markdown: String,
    #[serde(default)]
    pub affected_work_refs: Vec<String>,
    #[serde(default)]
    pub reusable_asset_refs: Vec<String>,
    #[serde(default)]
    pub invalidated_assumptions: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub confidence: Confidence,
    pub reported_by: ActorRef,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimaryCauseStatus {
    Unknown,
    Suspected,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrySafety {
    Safe,
    Unsafe,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureAnalysis {
    pub id: String,
    pub work_id: String,
    pub work_revision: u64,
    #[serde(default)]
    pub member_run_id: Option<String>,
    #[serde(default)]
    pub candidate: Option<CandidateRef>,
    pub observed_failure: String,
    pub impact: String,
    pub primary_cause_status: PrimaryCauseStatus,
    #[serde(default)]
    pub primary_cause: Option<String>,
    #[serde(default)]
    pub contributing_causes: Vec<String>,
    #[serde(default)]
    pub attempts_already_made: Vec<String>,
    #[serde(default)]
    pub last_safe_checkpoint: Option<String>,
    pub retry_safety: RetrySafety,
    #[serde(default)]
    pub side_effect_summary: Option<String>,
    #[serde(default)]
    pub recovery_options: Vec<String>,
    pub recommended_host_decision: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub confidence: Confidence,
    pub reported_by: ActorRef,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkModuleDefinition {
    pub module_id: String,
    pub module_version: u64,
    pub schema_version: u64,
    pub display_name: String,
    pub config_schema: serde_json::Value,
    pub allowed_actions: Vec<String>,
    pub relation_types: Vec<String>,
    pub default_gate_templates: Vec<serde_json::Value>,
    pub implementation_ref: String,
}

pub fn integration_plan_module_v1() -> WorkModuleDefinition {
    WorkModuleDefinition {
        module_id: "integration-plan".into(),
        module_version: 1,
        schema_version: 1,
        display_name: "Integration Plan".into(),
        config_schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "base_revision", "target_revision", "work_boundaries",
                "candidate_boundaries", "interfaces", "convergence_points",
                "merge_order", "conflict_owner", "per_merge_checks",
                "combined_verification", "rollback_plan"
            ]
        }),
        allowed_actions: vec!["attach".into(), "detach".into(), "resolve_gates".into()],
        relation_types: vec!["prerequisite".into(), "converges_into".into()],
        default_gate_templates: vec![serde_json::json!({
            "gate_type": "integration-plan-completeness",
            "gate_contract_version": "1",
            "required": true
        })],
        implementation_ref: "builtin:integration-plan@1".into(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkModuleBinding {
    pub id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub module_id: String,
    pub module_version: u64,
    pub resolved_config: serde_json::Value,
    pub config_fingerprint: String,
    pub attached_by: ActorRef,
    pub attached_at: String,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateRequirementSource {
    Direct,
    Module,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateRequirement {
    pub id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub work_report_id: String,
    pub candidate_fingerprint: String,
    pub source: GateRequirementSource,
    #[serde(default)]
    pub source_binding_id: Option<String>,
    pub gate_type: String,
    pub gate_contract_version: String,
    /// Exact authenticated evaluator identity frozen with the requirement.
    pub evaluator_ref: ActorRef,
    pub evaluator_version: String,
    /// Fingerprint of `(evaluator_ref, evaluator_version)` so an adapter or
    /// service upgrade cannot silently change who produced the verdict.
    pub evaluator_fingerprint: String,
    pub resolved_config: serde_json::Value,
    pub config_fingerprint: String,
    pub required: bool,
    #[serde(default)]
    pub dependency_requirement_ids: Vec<String>,
    pub requirement_set_fingerprint: String,
    pub created_at: String,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    Passed,
    Failed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateEvaluation {
    pub id: String,
    pub requirement_id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub work_report_id: String,
    pub candidate_fingerprint: String,
    pub config_fingerprint: String,
    pub evaluator_version: String,
    pub evaluator_fingerprint: String,
    pub dependency_fingerprint: String,
    pub verdict: GateVerdict,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub performed_by: ActorRef,
    pub evaluated_at: String,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateWaiverState {
    Active,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateWaiver {
    pub id: String,
    pub requirement_id: String,
    pub work_id: String,
    pub work_revision: u64,
    pub candidate_fingerprint: String,
    pub authority_actor: ActorRef,
    pub performed_by_actor: ActorRef,
    pub reason: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub state: GateWaiverState,
    pub version: u64,
    pub created_at: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalMutationEvent {
    pub id: String,
    pub aggregate_kind: String,
    pub aggregate_id: String,
    pub sequence: u64,
    pub store_sequence: u64,
    pub transition: String,
    pub expected_version: u64,
    pub resulting_version: u64,
    pub performed_by_actor: ActorRef,
    #[serde(default)]
    pub authority_actor: Option<ActorRef>,
    #[serde(default)]
    pub causation_ref: Option<String>,
    pub idempotency_key: String,
    pub canonical_request_fingerprint: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalOperation {
    pub event: CanonicalMutationEvent,
    pub resulting_projection: serde_json::Value,
    #[serde(default)]
    pub immutable_side_records: Vec<serde_json::Value>,
    #[serde(default)]
    pub initial_outbox_records: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationContext {
    pub execution_space_id: String,
    pub authenticated_actor: ActorRef,
    #[serde(default)]
    pub authority_actor: Option<ActorRef>,
    pub command_name: String,
    pub idempotency_key: String,
    pub expected_version: u64,
    /// Stable transport-bound fingerprint for an authenticated semantic
    /// action. Direct CLI/MCP commands leave this unset and use the canonical
    /// command payload fingerprint instead.
    #[serde(default)]
    pub request_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrustErrorCode {
    VersionConflict,
    IdempotencyKeyReused,
    UnauthorizedActor,
    InvalidStateTransition,
    AgentMemberPaused,
    AgentMemberRetired,
    MemberRunClosed,
    MemberRunRetired,
    MemberRunGenerationFenced,
    SupervisorGenerationFenced,
    NativeSessionMissing,
    NativeSessionIncompatible,
    DeliveryClaimConflict,
    DeliveryReceiptMissing,
    DeliveryRecoveryUncertain,
    WorkRevisionStale,
    WorkspacePathUnsafe,
    WorkspaceRepositoryMismatch,
    WorkspaceLinkEscape,
    WorkspaceGenerationFenced,
    WorkspaceDirty,
    WorkspaceConflicted,
    WorkspaceCleanupBlocked,
    ModuleConfigInvalid,
    ModuleLifecycleViolation,
    GateDependencyCycle,
    GateRequirementStale,
    GateEvaluationRequired,
    GateWaiverUnauthorized,
    ReportEvidenceMissing,
    FailureAnalysisMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustError {
    pub code: TrustErrorCode,
    pub message: String,
    pub retryable: bool,
    pub resource_kind: String,
    pub resource_id: String,
    #[serde(default)]
    pub current_version: Option<u64>,
}
