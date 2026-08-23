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
    pub result: Option<serde_json::Value>,
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
