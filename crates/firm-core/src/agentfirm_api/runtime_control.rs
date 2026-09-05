use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommandKind {
    AuthorMessage,
    StartSession,
    StopSession,
    ResumeSession,
    DispatchProvider,
    CancelProviderTurn,
    OpenRuntime,
    ResumeNativeSession,
    ReleaseRuntime,
    CloseMember,
    ReopenMember,
    RetireMember,
    DeleteNativeSession,
    StartCycle,
    InjectCurrentCycle,
    QueueAtNativeBoundary,
    InterruptCurrentCycle,
    CancelPendingInput,
    InspectContinuation,
    ActivateContinuation,
    InhibitContinuation,
    ResumeContinuation,
    ReplaceContinuationCondition,
    ClearContinuation,
    QuiesceExecutionLane,
    DrainRuntime,
    StopBackgroundTask,
    TransferExecutionDriver,
    InspectCommandEffect,
    ReconcileUnknownEffect,
    ReattachLiveRuntime,
    AbortIfNotApplied,
}

/// Exact immutable target identity resolved before a provider effect is
/// driven. An empty/default binding is readable for legacy records but is not
/// sufficient to admit a new provider-facing command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommandBinding {
    #[serde(default)]
    pub target_member_run_id: Option<String>,
    #[serde(default)]
    pub target_member_run_generation: Option<u64>,
    #[serde(default)]
    pub target_session_id: Option<String>,
    #[serde(default)]
    pub target_runtime_generation: Option<u64>,
    #[serde(default)]
    pub target_driver_generation: Option<u64>,
    #[serde(default)]
    pub target_driver: RuntimeDriverRef,
    #[serde(default)]
    pub native_session_ref: Option<NativeSessionRef>,
    #[serde(default)]
    pub composition_fingerprint: Option<String>,
    #[serde(default)]
    pub capability_fingerprint: Option<String>,
    #[serde(default)]
    pub capability_profile_version: Option<String>,
    #[serde(default)]
    pub permission_envelope_ref: Option<String>,
}

/// Stable reference to a provider-native cycle or continuation observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeNativeObjectRef {
    pub id: String,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// The provider boundary at which an effect is allowed to happen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSafePointRequirement {
    Immediate,
    CurrentCycle,
    CycleBoundary,
    RuntimeIdle,
    ExecutionLaneQuiesced,
    #[default]
    Unknown,
}

/// Preconditions are intentionally semantic rather than provider API names.
/// Adapters compile them into versioned native checks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommandPrecondition {
    #[serde(default)]
    pub expected_session_version: Option<u64>,
    #[serde(default)]
    pub expected_residency: Option<RuntimeResidency>,
    #[serde(default)]
    pub expected_activity: Option<RuntimeActivity>,
    #[serde(default)]
    pub expected_execution_driver: Option<MemberExecutionDriver>,
    #[serde(default)]
    pub expected_cycle_ref: Option<RuntimeNativeObjectRef>,
    #[serde(default)]
    pub expected_continuation_ref: Option<RuntimeNativeObjectRef>,
    #[serde(default)]
    pub expected_continuation_phase: Option<NativeContinuationPhase>,
    #[serde(default)]
    pub safe_point: RuntimeSafePointRequirement,
}

/// Strongest acknowledgement the caller asks the Runtime Supervisor to prove.
/// Higher levels must never be inferred from lower-level provider receipts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAcknowledgementLevel {
    CommandAdmitted,
    ProviderReceipt,
    TargetNativeStateObserved,
    CurrentCycleTerminalObserved,
    ContinuationInhibitedObserved,
    ExecutionLaneQuiesced,
    StateReconciled,
    SemanticWorkOutcomeRecorded,
    #[default]
    Unknown,
}

/// Semantic runtime postcondition requested by the command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDesiredPostcondition {
    ProviderAcknowledged,
    CycleStarted,
    CurrentCycleTerminal,
    PendingInputCancelled,
    ContinuationActivated,
    ContinuationInhibited,
    ExecutionLaneQuiesced,
    RuntimeAttached,
    RuntimeReleased,
    DriverTransferred,
    StateReconciled,
    #[default]
    Unknown,
}

/// Postcondition satisfaction is orthogonal to command transport phase and
/// effect certainty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePostconditionStatus {
    Satisfied,
    Unsatisfied,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommandPostcondition {
    #[serde(default)]
    pub desired_ack_level: RuntimeAcknowledgementLevel,
    #[serde(default)]
    pub desired_postcondition: RuntimeDesiredPostcondition,
    #[serde(default)]
    pub status: RuntimePostconditionStatus,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
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
    #[serde(default)]
    pub binding: RuntimeCommandBinding,
    #[serde(default)]
    pub precondition: RuntimeCommandPrecondition,
    #[serde(default)]
    pub postcondition: RuntimeCommandPostcondition,
    pub payload: serde_json::Value,
    pub payload_fingerprint: String,
    pub issued_at: String,
}

/// The only provider-facing dispatch shape after Wave 4C. It is created by the
/// target NodeDaemon from a claimed canonical delivery; public callers cannot
/// author or claim it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvocation {
    pub id: String,
    pub source_plane: String,
    pub source_record_id: String,
    #[serde(alias = "recipient_identity_id")]
    pub recipient_agent_member_id: String,
    pub recipient_session_id: String,
    pub recipient_session_generation: u64,
    pub node_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub provider: String,
    pub dispatch_mode: RuntimeDispatchMode,
    /// Immutable command/session/composition fence captured at derivation.
    /// Provider drivers must reject an invocation whose binding no longer
    /// matches the under-lock AgentSession and supervisor facts.
    #[serde(default)]
    pub binding: RuntimeCommandBinding,
    pub permission_ceiling: PermissionCeiling,
    pub content: String,
    pub content_fingerprint: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommandStatus {
    Requested,
    Accepted,
    Quiesced,
    Applied,
    Failed,
    RecoveryRequired,
}

/// Durable transport/effect phase. The legacy [`RuntimeCommandStatus`] remains
/// a compatibility projection only and must not carry effect certainty or
/// postcondition satisfaction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommandPhase {
    Prepared,
    Dispatched,
    ProviderAcknowledged,
    Observed,
    Settled,
    Rejected,
    RecoveryRequired,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEffectCertainty {
    None,
    NotApplied,
    Applied,
    Unknown,
}

/// Durable, provider-neutral identity for one accepted provider cycle and its
/// exact terminal observation. This is coordination evidence only: it neither
/// mirrors a provider transcript nor proves semantic Work success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCycleCorrelation {
    pub invocation_id: String,
    #[serde(default)]
    pub source_delivery_id: Option<String>,
    pub provider_input_id: String,
    pub input_acceptance_receipt: String,
    #[serde(default)]
    pub terminal_provider_input_id: Option<String>,
    #[serde(default)]
    pub exact_terminal_ref: Option<String>,
    pub native_session_id: String,
    pub agent_session_generation: u64,
    pub provider_attempt: u64,
}

/// Durable machine-local command journal. The NodeDaemon records acceptance
/// before touching a provider and records the observed effect afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommandRecord {
    pub id: String,
    pub execution_space_id: String,
    pub target_node_id: String,
    pub target_node_daemon_id: String,
    pub target_node_daemon_generation: u64,
    pub authenticated_actor: ActorRef,
    pub command: RuntimeCommandKind,
    pub required_capability: String,
    pub idempotency_key: String,
    pub request_fingerprint: String,
    /// Compatibility projection for existing callers. New control logic must
    /// use `phase`, `effect_certainty`, and `postcondition_status` separately.
    pub status: RuntimeCommandStatus,
    #[serde(default)]
    pub phase: RuntimeCommandPhase,
    pub effect_certainty: RuntimeEffectCertainty,
    #[serde(default)]
    pub postcondition_status: RuntimePostconditionStatus,
    #[serde(default)]
    pub binding: RuntimeCommandBinding,
    #[serde(default)]
    pub precondition: RuntimeCommandPrecondition,
    #[serde(default)]
    pub postcondition: RuntimeCommandPostcondition,
    #[serde(default)]
    pub target_session_id: Option<String>,
    #[serde(default)]
    pub target_session_generation: Option<u64>,
    #[serde(default)]
    pub source_record_id: Option<String>,
    #[serde(default)]
    pub provider_attempt: Option<u64>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    /// Present only after the exact StartCycle input and provider terminal have
    /// been correlated under the same immutable RuntimeCommand authority.
    #[serde(default)]
    pub cycle_correlation: Option<ProviderCycleCorrelation>,
    #[serde(default)]
    pub failure_code: Option<String>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
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
}

/// One in-flight `CanonicalWorkDelivery` superseded because the exact runtime
/// generation that received it was terminated by a NodeDaemon drain.
pub const WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN: &str =
    "WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN";
/// The same supersession proved by the Operator's predecessor-recovery
/// process/process-group termination evidence instead of an in-process drain.
pub const WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_PREDECESSOR_RECOVERY: &str =
    "WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_PREDECESSOR_RECOVERY";
/// The same supersession proved by the Host from durable epochs alone: the
/// exact MemberRun/AgentSession generation the delivery was bound to can never
/// pass the runtime fence again (`team-run work recover-lost-execution`).
pub const WORK_DELIVERY_SUPERSEDED_BY_HOST_LOST_EXECUTION_RECOVERY: &str =
    "WORK_DELIVERY_SUPERSEDED_BY_HOST_LOST_EXECUTION_RECOVERY";

/// The exact refusal the Store returns when a Session interrupted by a
/// NodeDaemon drain is asked to re-enter the ordinary lane before its killed
/// runtime is provably gone.
///
/// The fence itself and every reader that must recognize the refusal share
/// this one string so they cannot drift apart. Recognizing it is never
/// permission to bypass it: the refusal says the lane is not resumable *yet*,
/// so the only admitted response is to leave the member startable and let a
/// later pass retry once the lane settles.
pub const AGENT_SESSION_DRAIN_RESUME_NOT_YET_RESUMABLE: &str =
    "AgentSession interrupted by a NodeDaemon drain may resume only from a detached, disarmed lane at a terminal turn boundary with no ambiguous RuntimeCommand; reconcile the runtime first";

/// Every failure code that records an in-flight delivery invalidated because
/// its exact runtime generation is provably gone. None of them asserts a
/// provider turn outcome: they say the attempt can never be settled by that
/// generation, never that the killed turn completed or failed semantically.
/// The delivery fold and the writers that emit them read this one list, so
/// they cannot drift apart, and no code enters it before a writer emits it.
pub const LOST_RUNTIME_GENERATION_DELIVERY_FAILURE_CODES: [&str; 3] = [
    WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN,
    WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_PREDECESSOR_RECOVERY,
    WORK_DELIVERY_SUPERSEDED_BY_HOST_LOST_EXECUTION_RECOVERY,
];

/// True when `code` is one of [`LOST_RUNTIME_GENERATION_DELIVERY_FAILURE_CODES`].
pub fn is_lost_runtime_generation_delivery_failure_code(code: &str) -> bool {
    LOST_RUNTIME_GENERATION_DELIVERY_FAILURE_CODES.contains(&code)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryClaim {
    pub claim_id: String,
    pub supervisor_generation: u64,
    pub member_generation: u64,
    pub claim_expires_at: String,
}

/// Atomic Team-subject inbox claim. The membership generation is the routing
/// fence; no AgentSession is required when the Team delivery is admitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMessageDeliveryClaim {
    pub claim_id: String,
    pub team_membership_id: String,
    pub membership_generation: u64,
    pub node_daemon_generation: u64,
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
pub enum RuntimeRecoveryResolution {
    ConfirmApplied,
    ConfirmNotApplied,
    KeepRecoveryRequired,
}
