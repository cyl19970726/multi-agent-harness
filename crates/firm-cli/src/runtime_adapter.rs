//! Provider-neutral coding-agent runtime adapter (main Spec + architecture
//! review DOC-89).
//!
//! Six-layer model; none of these layers is a new durable CompanyOS object:
//!
//! - `AgentSession` / `NativeSessionRef`: already canonical and durable.
//! - `RuntimeHandle`: the process-local live adapter instance itself. It may
//!   be lost at any time and is never an identity.
//! - `ExecutionCycle`: one accepted input driven to the binding's settled
//!   boundary (Pi: `prompt` → `agent_settled`). A cycle outcome is never a
//!   Work acceptance.
//! - `NativeContinuation` / `ExecutionDriver`: projections reported honestly
//!   through capability bindings, not invented per provider.
//!
//! Hard rules for every binding:
//!
//! - compile semantic intents into the provider's real primitives; an intent
//!   the binding cannot execute fails closed with
//!   `PROVIDER_CAPABILITY_UNSUPPORTED`, never a silent no-op;
//! - ordinary Messages stay in the durable Harness queue until the cycle
//!   settles — only an explicit Steer control command may compile into
//!   current-cycle injection (DOC-89 §13.1);
//! - capability claims carry evidence; a bare `true` that the code cannot
//!   back is an overclaim and a defect;
//! - one execution driver per native session + writable workspace: the
//!   supervisor lease + generation fencing stays the single-driver seam.

use std::cell::Cell;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use harness_core::agentfirm_api::{AgentSessionStatus, PermissionCeiling};

use crate::provider_adapter::{
    self, PendingProviderControl, ProviderControlDispatch, ProviderNativeControl,
};
use crate::provider_event_api::LiveProviderActivityKind;
use crate::supervisor_wake::{WakeBackoff, WakePolicy};
use crate::{
    active_work_continuation_prompt, emit_live_provider_activity, mark_message_delivered,
    member_work_collaboration_envelope, native_session_ref, now_string, parse_round_result,
    prepare_provider_effect, provider_turn_coordination_summary,
    refresh_member_after_provider_callbacks, require_provider_session_authority,
    settle_provider_effect, settle_provider_effect_not_applied, stop_member_for_latched_close,
    team_messages_prompt, transition_provider_session_for_member, wait_for_idle_member_wake,
    work_contract_prompt, ClaimedWork, CliError, CliResult, ControlReceiver, IdleMemberWake,
    LiveMemberControlRegistration, LiveProviderTurnGuard, MemberActionStatus, MemberControlCommand,
    MemberOutcome, MemberRoundResult, MemberRunStatus, MemberRuntimeContext,
    ProviderRuntimeProjection, TeamMessageProjection, TeamRunEventSourceKind, TeamRunLedger,
};

const PROVIDER_UNPRODUCTIVE_ROUND_LIMIT: u32 = 3;

/// Deterministic integration hook: pause after the provider terminal boundary
/// is observed but before the current Supervisor/session authority is
/// revalidated. This proves that a successor lease fences every semantic
/// write from the stale process-local handle. The hook is provider-neutral;
/// provider names only select isolated test files.
fn supervisor_test_terminal_receive_barrier(provider: &str) -> CliResult<()> {
    let provider = provider.to_ascii_uppercase();
    let ready_key = format!("FIRM_TEST_{provider}_TERMINAL_RECEIVED_READY");
    let legacy_ready_key = format!("HARNESS_TEST_{provider}_TERMINAL_RECEIVED_READY");
    let release_key = format!("FIRM_TEST_{provider}_TERMINAL_RECEIVED_RELEASE");
    let legacy_release_key = format!("HARNESS_TEST_{provider}_TERMINAL_RECEIVED_RELEASE");
    let Some(ready) = std::env::var_os(&ready_key).or_else(|| std::env::var_os(&legacy_ready_key))
    else {
        return Ok(());
    };
    let release = std::env::var_os(&release_key)
        .or_else(|| std::env::var_os(&legacy_release_key))
        .ok_or_else(|| {
            CliError::Usage(format!(
                "{ready_key} requires the bounded test release selector {release_key}"
            ))
        })?;
    std::fs::write(
        std::path::PathBuf::from(ready),
        b"terminal provider frame received",
    )?;
    let release = std::path::PathBuf::from(release);
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !release.exists() {
        if std::time::Instant::now() >= deadline {
            return Err(CliError::Usage(format!(
                "timed out waiting for {release_key}"
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Capability honesty
// ---------------------------------------------------------------------------

/// Per-capability execution status (DOC-89 §8.1). Order matters only for
/// reporting; every entry must carry `evidence` naming the proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityStatus {
    Supported,
    Unsupported,
    Degraded,
    // No binding reports Experimental today; the status exists so a future
    // binding (e.g. a DeepSeek native bridge canary) can ship honestly
    // between Unsupported and Supported.
    #[allow(dead_code)]
    Experimental,
}

impl CapabilityStatus {
    #[cfg(test)]
    pub(crate) fn is_supported(self) -> bool {
        matches!(self, CapabilityStatus::Supported)
    }
}

/// One semantic intent → execution status + evidence. `evidence` names the
/// proof (RPC contract, test, or transport fact); a binding without proof
/// reports `Unsupported`/`Experimental`, never `Supported`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CapabilityBinding {
    pub capability: &'static str,
    pub status: CapabilityStatus,
    pub evidence: String,
    /// Security-relevant capabilities name the real enforcement mechanism;
    /// absence means none was verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_enforcement_locus: Option<String>,
}

// ---------------------------------------------------------------------------
// Cycle control and outcome
// ---------------------------------------------------------------------------

/// Harness control intents delivered mid-cycle. Ordinary Messages never
/// appear here — they remain in the durable queue until the cycle settles.
#[derive(Debug)]
pub(crate) struct PendingSteer {
    pub content: String,
    pub success_reply: Value,
    pub reply: SyncSender<CliResult<Value>>,
    pub admission: crate::ProviderEffectAdmission,
}

#[derive(Debug)]
pub(crate) enum SteerProviderResult {
    Acknowledged(ControlTransportReceipt),
    Unknown(String),
    NotApplied(String),
}

#[derive(Debug, Default)]
pub(crate) struct CycleControl {
    pub close: bool,
    pub interrupt: bool,
    /// Explicit Steer command bodies to compile into current-cycle injection
    /// at the binding's next control boundary.
    pub injects: Vec<PendingSteer>,
    /// A control failed before reaching the provider boundary. The binding
    /// must stop the cycle instead of silently continuing until a later
    /// natural settlement.
    pub fatal_error: Option<String>,
}

/// Non-invasive point-in-time provider observation. This is deliberately a
/// small projection, never a copy of the native transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimeObservation {
    pub transport_alive: bool,
    pub process_alive: bool,
    pub is_streaming: Option<bool>,
    pub pending_message_count: Option<u64>,
    pub steering_mode: Option<String>,
    pub follow_up_mode: Option<String>,
    pub settled_boundary_observed: bool,
}

impl RuntimeObservation {
    fn terminal_cycle_observed(&self) -> bool {
        self.transport_alive
            && self.process_alive
            && self.is_streaming == Some(false)
            && self.settled_boundary_observed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ControlTransportReceipt {
    pub command: String,
    pub response_id: Option<String>,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct QuiesceOutcome {
    pub drained: bool,
    pub observation: RuntimeObservation,
    pub evidence: String,
}

/// One ExecutionCycle's settled outcome. `interrupted` /
/// `close_requested_by_harness` describe the cycle only — neither is a Work
/// or Message acceptance.
#[derive(Debug, Clone)]
pub(crate) struct ExecutionCycleOutcome {
    pub final_text: String,
    /// A provider-structured terminal semantic failure. This is distinct from
    /// transport uncertainty: the provider accepted the input and emitted a
    /// terminal boundary, so the StartCycle effect remains Applied.
    pub provider_terminal_failure: Option<crate::ProviderTerminalFailure>,
    pub interrupted: bool,
    pub close_requested_by_harness: bool,
    pub tool_call_count: u32,
    /// Provider response proving only that the cycle input crossed the
    /// transport boundary. It is intentionally separate from terminal cycle
    /// observation and never proves semantic completion.
    pub input_acceptance_receipt: ControlTransportReceipt,
    pub control_receipts: Vec<ControlTransportReceipt>,
    pub terminal_observation: RuntimeObservation,
}

// ---------------------------------------------------------------------------
// The adapter contract
// ---------------------------------------------------------------------------

/// The provider-neutral semantic-intent surface for a persistent Team member
/// runtime. Intent names follow DOC-89 §8.1; bindings compile them into
/// provider primitives (Pi: coding-agent RPC; Codex: app-server; a future
/// DeepSeek native bridge needs no new core object to plug in here).
pub(crate) trait TeamRuntimeAdapter:
    crate::runtime_adapter_contract::RuntimeAdapter
{
    /// Closed provider-set member: "pi", "codex", ...
    fn provider(&self) -> &'static str;
    /// Display name for member-facing summaries ("Pi").
    fn display_name(&self) -> &'static str;
    /// Runtime truth behind any static capability matrix. Associated
    /// function (not `&self`) so capability surfaces can report a binding's
    /// honest status without spawning a live runtime.
    fn capability_bindings() -> Vec<CapabilityBinding>
    where
        Self: Sized;

    /// RuntimeHandle liveness; resume/reconnect the transport when needed.
    fn ensure_alive(&mut self) -> CliResult<()>;
    /// Opaque native-session locator (NativeSessionRef content, never a
    /// transcript).
    fn native_session_locator(&self) -> &str;
    /// Locator kind tag for NativeSessionRef ("pi_session").
    fn native_locator_kind(&self) -> &'static str;

    /// Bind the exact persisted profile and durable AgentSession authority
    /// before the canonical provider-neutral contract admits any effect.
    fn bind_authority_session(
        &mut self,
        session: harness_core::agentfirm_api::AgentSession,
        profile: &harness_core::ProviderIntegrationProfile,
    ) -> CliResult<()>;

    /// ExecutionCycle: drive one accepted input to its settled boundary.
    /// `on_event` sees raw provider frames for live projection;
    /// `poll_control` drains pending Harness control intents.
    fn run_cycle(
        &mut self,
        input: &str,
        idle_timeout: Duration,
        on_input_accepted: &mut dyn FnMut(&ControlTransportReceipt) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(&PendingSteer, &SteerProviderResult) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> CycleControl,
    ) -> CliResult<ExecutionCycleOutcome>;

    /// Project one raw provider frame into a typed volatile live activity.
    /// Associated function (not `&self`) so the live-projection closure can
    /// run while the adapter is mutably borrowed by `run_cycle`.
    fn project_live(event: &Value) -> Option<(LiveProviderActivityKind, String)>
    where
        Self: Sized;

    /// Build the executable native-control proxy (interrupt/close) consumed
    /// by `provider_adapter::execute_team_control`. Associated function
    /// (not `&self`) so the generic loop can poll controls while the adapter
    /// is mutably borrowed by `run_cycle`; bindings that need transport
    /// access inside dispatch give their proxy a cloneable channel.
    fn native_control<'a>(
        close: &'a mut bool,
        interrupt: &'a mut bool,
    ) -> Box<dyn ProviderNativeControl + 'a>
    where
        Self: Sized;

    /// inject_current_cycle (steer) support. Bindings that cannot inject
    /// report false and the generic loop answers Steer commands with an
    /// explicit unsupported error instead of dropping them.
    fn supports_inject_current_cycle(&self) -> bool {
        false
    }

    /// queue_at_native_boundary (follow_up) support report. The generic
    /// loop keeps ordinary mail on the Harness queue regardless; this report
    /// is consumed by capability surfaces and future dispatch policies.
    #[allow(dead_code)]
    fn supports_native_boundary_queue(&self) -> bool {
        false
    }
}

fn canonical_runtime_binding(
    session: &harness_core::agentfirm_api::AgentSession,
) -> harness_core::agentfirm_api::RuntimeCommandBinding {
    harness_core::agentfirm_api::RuntimeCommandBinding {
        target_session_id: Some(session.id.clone()),
        target_runtime_generation: Some(session.runtime_generation),
        target_driver_generation: Some(session.control_state.driver_generation),
        target_driver: session.control_state.driver_ref.clone(),
        native_session_ref: session.native_session_ref.clone(),
        composition_fingerprint: session.control_state.composition_fingerprint.clone(),
        capability_fingerprint: session.control_state.capability_fingerprint.clone(),
        capability_profile_version: session
            .control_state
            .capability_fingerprint
            .as_ref()
            .map(|_| "agentfirm-runtime-adapter-v1".to_string()),
        permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
    }
}

fn preflight_start_cycle<A: TeamRuntimeAdapter>(
    adapter: &A,
    session: &harness_core::agentfirm_api::AgentSession,
) -> CliResult<()> {
    let binding = canonical_runtime_binding(session);
    crate::runtime_adapter_contract::preflight_effect(
        crate::runtime_adapter_contract::RuntimeAdapter::describe(adapter),
        session,
        crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &session.node_daemon_id,
            target_node_daemon_generation: session.node_daemon_generation,
        },
        crate::runtime_adapter_contract::SemanticCapability::StartCycle,
        &[],
    )
    .map(|_| ())
    .map_err(|error| CliError::Usage(format!("RUNTIME_ADAPTER_PREFLIGHT_FAILED: {error}")))
}

/// Provider-profile preflight used before a live RuntimeHandle exists. This
/// keeps process spawn/resume behind the same exact capability closure and
/// session composition fence as operations on an attached adapter.
pub(crate) fn preflight_profile_effect(
    profile: &harness_core::ProviderIntegrationProfile,
    session: &harness_core::agentfirm_api::AgentSession,
    capability: crate::runtime_adapter_contract::SemanticCapability,
) -> CliResult<()> {
    harness_core::Validate::validate(profile).map_err(|error| {
        CliError::Usage(format!(
            "RUNTIME_ADAPTER_PROFILE_INVALID: provider profile failed canonical validation: {error}"
        ))
    })?;
    let composition_fingerprint = profile.composition_fingerprint.clone().ok_or_else(|| {
        CliError::Usage(
            "RUNTIME_ADAPTER_FENCE_INCOMPLETE: profile has no composition fingerprint".to_string(),
        )
    })?;
    let capability_fingerprint = profile.capability_fingerprint.clone().ok_or_else(|| {
        CliError::Usage(
            "RUNTIME_ADAPTER_FENCE_INCOMPLETE: profile has no capability fingerprint".to_string(),
        )
    })?;
    let description = crate::runtime_adapter_contract::RuntimeDescription {
        binding_id: format!("{}:{}", profile.provider, profile.execution_mode),
        native_protocol: profile.execution_mode.clone(),
        composition_fingerprint,
        capability_fingerprint,
        capability_bindings: profile.capability_bindings.clone(),
    };
    let binding = canonical_runtime_binding(session);
    crate::runtime_adapter_contract::preflight_effect(
        &description,
        session,
        crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &session.node_daemon_id,
            target_node_daemon_generation: session.node_daemon_generation,
        },
        capability,
        &[],
    )
    .map(|_| ())
    .map_err(|error| CliError::Usage(format!("RUNTIME_ADAPTER_PREFLIGHT_FAILED: {error}")))
}

// ---------------------------------------------------------------------------
// Permission ceiling → Pi tool allowlist compilation
// ---------------------------------------------------------------------------

/// Compile an *admissible* permission ceiling into Pi launch arguments.
///
/// - `ReadOnly` → read-only tools only.
/// - `WorkspaceWrite` → refused: Pi's tool-kind allowlist does not contain
///   write/edit paths and therefore cannot enforce the workspace boundary.
/// - `FullAccess` → `None`: the Pi default toolset (including `bash`) runs
///   unrestricted. This is only honest when the profile records
///   `security_enforcement_locus = none_verified` — the adapter enforces
///   nothing and says so.
pub(crate) fn pi_tools_allowlist_for_ceiling(
    ceiling: PermissionCeiling,
) -> CliResult<Option<&'static str>> {
    match ceiling {
        PermissionCeiling::ReadOnly => Ok(Some("read,grep,find,ls")),
        PermissionCeiling::WorkspaceWrite => Err(CliError::Usage(
            "PI_PERMISSION_ADMISSION_FAILED: workspace_write requires verified filesystem containment; Pi --tools only limits tool kinds"
                .to_string(),
        )),
        PermissionCeiling::FullAccess => Ok(None),
    }
}

/// The enforcement-locus claim matching `pi_tools_allowlist_for_ceiling`.
pub(crate) fn pi_security_enforcement_locus(
    ceiling: PermissionCeiling,
) -> harness_core::SecurityEnforcementLocus {
    use harness_core::{SecurityEnforcementLocus, SecurityEnforcementLocusKind};
    match ceiling {
        PermissionCeiling::ReadOnly => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::AdapterToolAllowlist,
            note: Some(
                "compiled to Pi's read-only `--tools` allowlist at spawn".to_string(),
            ),
        },
        PermissionCeiling::WorkspaceWrite => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::NoneVerified,
            note: Some(
                "Pi --tools limits tool kinds but does not contain write/edit paths; workspace_write admission is refused without an OS sandbox or reviewed bridge"
                    .to_string(),
            ),
        },
        PermissionCeiling::FullAccess => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::NoneVerified,
            note: Some(
                "full access runs the Pi default toolset under explicit trusted policy; no adapter-level enforcement verified"
                    .to_string(),
            ),
        },
    }
}

/// Fail-closed admission for the exact Pi spawn policy. Pi's `--tools`
/// switch is a tool-kind allowlist, not a filesystem containment boundary:
/// `write`/`edit` can target paths outside the workspace. Consequently only
/// read-only and explicitly trusted full-access launches are admissible until
/// an OS sandbox or reviewed native bridge is part of the composition.
pub(crate) fn admit_pi_permission_ceiling(
    ceiling: PermissionCeiling,
    compiled_tools: Option<&str>,
) -> CliResult<harness_core::SecurityEnforcementLocus> {
    let expected = pi_tools_allowlist_for_ceiling(ceiling)?;
    if compiled_tools != expected {
        return Err(CliError::Usage(format!(
            "PI_PERMISSION_ADMISSION_FAILED: {ceiling:?} expected tools {expected:?}, got {compiled_tools:?}"
        )));
    }
    Ok(pi_security_enforcement_locus(ceiling))
}

// ---------------------------------------------------------------------------
// Generic member loop
// ---------------------------------------------------------------------------

/// One cycle's input, projected from an idle wake.
struct CycleInput {
    prompt: String,
    active_work: Option<ClaimedWork>,
    accepted_messages: Vec<TeamMessageProjection>,
    /// New `last_consumed_work_version`; None leaves the tracker unchanged.
    consumed_work_version: Option<u64>,
}

struct PendingControlReply {
    action: provider_adapter::ProviderControlAction,
    reply: Option<SyncSender<CliResult<Value>>>,
}

impl PendingControlReply {
    fn send(mut self, result: CliResult<Value>) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(result);
        }
    }
}

impl Drop for PendingControlReply {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Err(CliError::Usage(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: provider control ended before its durable postcondition receipt was published"
                    .to_string(),
            )));
        }
    }
}

fn fail_pending_control_replies(replies: &mut Vec<PendingControlReply>, detail: impl Into<String>) {
    let detail = detail.into();
    for pending in replies.drain(..) {
        pending.send(Err(CliError::Usage(detail.clone())));
    }
}

/// The two previously per-loop, twice-per-loop wake match blocks collapsed
/// into one shared projection.
fn idle_wake_into_cycle<A: TeamRuntimeAdapter>(
    wake: IdleMemberWake,
    ledger: &TeamRunLedger,
    objective: &str,
    context: &MemberRuntimeContext,
    member_row: &mut ProviderRuntimeProjection,
    adapter: &mut A,
) -> CliResult<Result<CycleInput, MemberOutcome>> {
    match wake {
        IdleMemberWake::Work(claimed) => {
            let envelope = member_work_collaboration_envelope(
                ledger,
                context.execution_space_id.as_deref(),
                context.project_id.as_deref(),
                context.project_selector.as_deref(),
                member_row,
                Some(&claimed.work),
            )?;
            let consumed = claimed.work.version;
            let prompt = work_contract_prompt(objective, member_row, &claimed.work, &envelope);
            Ok(Ok(CycleInput {
                prompt,
                active_work: Some(*claimed),
                accepted_messages: Vec::new(),
                consumed_work_version: Some(consumed),
            }))
        }
        IdleMemberWake::ActiveWorkContinuation(work) => {
            let envelope = member_work_collaboration_envelope(
                ledger,
                context.execution_space_id.as_deref(),
                context.project_id.as_deref(),
                context.project_selector.as_deref(),
                member_row,
                Some(&work),
            )?;
            let consumed = work.version;
            let prompt = active_work_continuation_prompt(objective, member_row, &work, &envelope);
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: Vec::new(),
                consumed_work_version: Some(consumed),
            }))
        }
        IdleMemberWake::Messages(messages) => {
            let prompt = team_messages_prompt(
                "TEAM MESSAGES arrived. They are conversation, not Work ownership. \
                 Address the question or coordination request, and use the Works \
                 board for any durable responsibility.",
                &messages,
            );
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: messages,
                consumed_work_version: None,
            }))
        }
        IdleMemberWake::CloseRequested { close, reply } => {
            let result = close_idle_runtime(ledger, member_row, adapter, &close);
            match result {
                Ok((outcome, close_receipt)) => {
                    if let Some(reply) = reply {
                        let _ = reply.send(Ok(serde_json::json!({
                            "member_run_id": member_row.id,
                            "status": "closed",
                            "provider_ack": "member_runtime_close_applied",
                            "provider_terminal_evidence": {
                                "provider_terminal_event": "idle_before_close",
                                "member_runtime_close": close_receipt,
                            },
                        })));
                    }
                    Ok(Err(outcome))
                }
                Err(error) => {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(CliError::Usage(error.to_string())));
                    }
                    Err(error)
                }
            }
        }
        IdleMemberWake::TestRetired => Ok(Err(MemberOutcome::new(
            member_row,
            MemberRunStatus::Idle,
            format!(
                "{} member test runtime retired while idle",
                adapter.display_name()
            ),
        ))),
        IdleMemberWake::Degraded(reason) => Ok(Err(MemberOutcome::new(
            member_row,
            MemberRunStatus::Blocked,
            format!("{} member degraded: {reason}", adapter.display_name()),
        ))),
    }
}

/// Execute a reversible Team-member Close while idle. This is intentionally
/// not the stronger Quiesce + Release operation: Team Close releases only the
/// owned adapter/process handle and retains the machine-owned AgentSession and
/// provider-native session for Reopen.
fn close_idle_runtime<A: TeamRuntimeAdapter>(
    ledger: &TeamRunLedger,
    member_row: &mut ProviderRuntimeProjection,
    adapter: &mut A,
    close: &harness_core::TeamMemberCloseRequest,
) -> CliResult<(
    MemberOutcome,
    crate::runtime_adapter_contract::MemberRuntimeCloseReceipt,
)> {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    let close_receipt = execute_member_runtime_close(ledger, member_row, adapter, close, "idle")?;
    crate::transition_provider_session_runtime_control(
        ledger,
        member_row,
        RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )?;
    transition_provider_session_for_member(ledger, member_row, AgentSessionStatus::Idle)?;
    stop_member_for_latched_close(ledger, member_row, close)?;
    Ok((
        MemberOutcome::new(
            member_row,
            MemberRunStatus::Stopped,
            format!("{} member runtime closed by Host", adapter.display_name()),
        ),
        close_receipt,
    ))
}

/// Cross the provider boundary for the narrow, reversible Team Close. This
/// is deliberately independent from strong Quiesce/Release: it proves that
/// the owned live handle is gone and the native session locator is retained,
/// but makes no workspace-child or durable-flush claim.
fn execute_member_runtime_close<A: TeamRuntimeAdapter>(
    ledger: &TeamRunLedger,
    member_row: &mut ProviderRuntimeProjection,
    adapter: &mut A,
    close: &harness_core::TeamMemberCloseRequest,
    boundary: &str,
) -> CliResult<crate::runtime_adapter_contract::MemberRuntimeCloseReceipt> {
    use crate::runtime_adapter_contract::SemanticCapability;
    use harness_core::agentfirm_api::RuntimeCommandKind;

    let profile = member_row.provider_profile.clone().ok_or_else(|| {
        CliError::Usage(format!(
            "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
            member_row.id
        ))
    })?;
    let close_source = format!("{}:{boundary}:close-runtime", close.id);
    let effect = crate::prepare_provider_effect_kind(
        ledger,
        member_row,
        &close_source,
        "close the owned Team member runtime and retain its native session",
        RuntimeCommandKind::CloseMember,
        "member.close",
    )?;
    let close_admission = adapter
        .bind_authority_session(effect.target_session.clone(), &profile)
        .and_then(|_| {
            let binding = canonical_runtime_binding(&effect.target_session);
            crate::runtime_adapter_contract::preflight_effect(
                adapter.describe(),
                &effect.target_session,
                crate::runtime_adapter_contract::RuntimeFence {
                    binding: &binding,
                    target_node_daemon_id: &effect.target_session.node_daemon_id,
                    target_node_daemon_generation: effect.target_session.node_daemon_generation,
                },
                SemanticCapability::CloseRuntime,
                &[],
            )
            .map(|_| ())
            .map_err(|error| CliError::Usage(error.to_string()))
        });
    if let Err(error) = close_admission {
        settle_provider_effect_not_applied(ledger, &effect, error.to_string())?;
        return Err(CliError::Usage(format!(
            "RUNTIME_COMMAND_NOT_APPLIED: {} {boundary} Close preflight failed: {error}",
            adapter.provider()
        )));
    }
    let binding = canonical_runtime_binding(&effect.target_session);
    let close_receipt = match adapter.close_runtime(crate::runtime_adapter_contract::RuntimeFence {
        binding: &binding,
        target_node_daemon_id: &effect.target_session.node_daemon_id,
        target_node_daemon_generation: effect.target_session.node_daemon_generation,
    }) {
        Ok(receipt) => receipt,
        Err(
            error @ crate::runtime_adapter_contract::RuntimeContractError::CapabilityAdmissionDenied {
                ..
            },
        ) => {
            // Dynamic adapter admission (for example, a permission mode that
            // cannot prove old workspace writers are gone) is decided before
            // crossing the provider boundary. Preserve that exact NotApplied
            // fact instead of manufacturing RecoveryRequired uncertainty.
            settle_provider_effect_not_applied(ledger, &effect, error.to_string())?;
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_NOT_APPLIED: {} {boundary} Close admission failed: {error}",
                adapter.provider()
            )));
        }
        Err(error) => {
            settle_provider_effect(ledger, &effect, false, None, Some(error.to_string()))?;
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: {} {boundary} Close is unproven: {error}",
                adapter.provider()
            )));
        }
    };
    if let Err(error) = close_receipt.verify() {
        settle_provider_effect(ledger, &effect, false, None, Some(error.to_string()))?;
        return Err(CliError::Usage(format!(
            "RUNTIME_COMMAND_RECOVERY_REQUIRED: {} {boundary} Close receipt is incomplete: {error}",
            adapter.provider()
        )));
    }
    settle_provider_effect(
        ledger,
        &effect,
        true,
        Some(serde_json::json!({
            "phase": "member_runtime_closed",
            "receipt": &close_receipt,
        })),
        None,
    )?;
    Ok(close_receipt)
}

/// Wait for the next wake and project it into a cycle input (or a terminal
/// outcome). Shared by the first wait and the loop-tail wait.
#[allow(clippy::too_many_arguments)]
fn await_next_cycle<A: TeamRuntimeAdapter>(
    ledger: &TeamRunLedger,
    objective: &str,
    context: &MemberRuntimeContext,
    member_row: &mut ProviderRuntimeProjection,
    adapter: &mut A,
    live_control: &ControlReceiver<MemberControlCommand>,
    zero_output_streak: u32,
    last_consumed_work_version: Option<u64>,
    wake_policy: &WakePolicy,
    wake_backoff: &mut WakeBackoff,
) -> CliResult<Result<CycleInput, MemberOutcome>> {
    let wake = {
        let agent_member_id = member_row.agent_member_id.clone();
        wait_for_idle_member_wake(
            ledger,
            member_row,
            live_control,
            || {
                require_provider_session_authority(ledger, &agent_member_id, false)?;
                adapter.ensure_alive()
            },
            zero_output_streak,
            last_consumed_work_version,
            wake_policy,
            wake_backoff,
        )?
    };
    idle_wake_into_cycle(wake, ledger, objective, context, member_row, adapter)
}

/// The provider-neutral persistent member loop: wake → claim → drive one
/// ExecutionCycle → settle receipts → repeat. One implementation for every
/// binding; provider differences live behind `TeamRuntimeAdapter`, not in a
/// branded loop per provider.
///
/// The binding's prepare step owns provider-specific spawn/policy work and
/// hands over a live adapter plus the registered control channel.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_team_member_with_adapter<A: TeamRuntimeAdapter>(
    ledger: &TeamRunLedger,
    objective: &str,
    member_row: &mut ProviderRuntimeProjection,
    context: &MemberRuntimeContext,
    adapter: &mut A,
    live_control: &ControlReceiver<MemberControlCommand>,
    mut live_control_registration: Option<LiveMemberControlRegistration>,
) -> CliResult<MemberOutcome> {
    let supports_inject = adapter.supports_inject_current_cycle();
    let provider = adapter.provider();
    let display = adapter.display_name();

    let wake_policy = WakePolicy::default();
    let mut wake_backoff = WakeBackoff::new();
    let mut zero_output_streak = member_row.zero_output_streak;
    let mut last_consumed_work_version = member_row.last_consumed_work_version;

    let mut cycle = match await_next_cycle(
        ledger,
        objective,
        context,
        member_row,
        adapter,
        live_control,
        zero_output_streak,
        last_consumed_work_version,
        &wake_policy,
        &mut wake_backoff,
    )? {
        Ok(cycle) => cycle,
        Err(outcome) => return Ok(outcome),
    };
    // The first wake is a real delivery/continuation just like every later
    // wake. Persist its consumed Work revision when this first cycle settles;
    // otherwise a restart can rediscover and redrive the already-consumed
    // revision even though later cycles update the tracker correctly.
    if let Some(version) = cycle.consumed_work_version {
        last_consumed_work_version = Some(version);
    }

    let mut round = 0u32;
    loop {
        round += 1;
        let prompt = cycle.prompt.clone();
        let source_record_id = cycle
            .active_work
            .as_ref()
            .map(|claimed| claimed.delivery.id.as_str())
            .or_else(|| {
                cycle
                    .accepted_messages
                    .last()
                    .map(|message| message.id.as_str())
            })
            .map(str::to_string)
            .unwrap_or_else(|| format!("continuation:{}:{round}", member_row.id));
        let source_record_id = format!("{source_record_id}:turn:{round}");
        // A concrete cycle is the only boundary that activates the machine
        // AgentSession. Idle mailbox waiting and an attached process are not
        // an active provider turn.
        transition_provider_session_for_member(
            ledger,
            member_row,
            harness_core::agentfirm_api::AgentSessionStatus::Active,
        )?;
        let effect = prepare_provider_effect(ledger, member_row, &source_record_id, &prompt)?;
        let profile = member_row.provider_profile.as_ref().ok_or_else(|| {
            CliError::Usage(format!(
                "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
                member_row.id
            ))
        })?;
        let adapter_admission = adapter
            .bind_authority_session(effect.target_session.clone(), profile)
            .and_then(|()| preflight_start_cycle(adapter, &effect.target_session));
        if let Err(error) = adapter_admission {
            settle_provider_effect_not_applied(ledger, &effect, error.to_string())?;
            return Err(error);
        }
        let mut accepted_provider_receipt: Option<String> = None;
        let mut pending_control_effects: Vec<Box<PendingProviderControl>> = Vec::new();
        let mut pending_control_replies: Vec<PendingControlReply> = Vec::new();
        let mut control_prepare_error: Option<String> = None;
        // Close/Interrupt may arrive before the provider's first
        // input-acceptance evidence. Both RuntimeCommands are then admitted
        // against the same Idle activity snapshot. Preserve that snapshot
        // until both commands settle, then publish the terminal Idle state.
        let terminal_control_dispatched = Cell::new(false);

        let round_start = member_row.clone();
        let turn_result = {
            let _turn_lease = context.turn_leases.acquire();
            let _live_turn_guard = LiveProviderTurnGuard::new(
                context.live_sink.clone(),
                ledger.run_id.clone(),
                member_row.id.clone(),
                member_row.runtime_generation,
            );
            let live_sink = context.live_sink.clone();
            adapter.run_cycle(
                &prompt,
                context.idle_timeout,
                &mut |acceptance| {
                    // Prompt response success proves input acceptance only.
                    // Settle this dispatch immediately so a later transport
                    // loss cannot negative-ack or blindly redrive it.
                    ledger.require_supervisor_lease()?;
                    require_provider_session_authority(
                        ledger,
                        &member_row.agent_member_id,
                        true,
                    )?;
                    let provider_receipt = acceptance
                        .response_id
                        .as_deref()
                        .filter(|receipt| !receipt.trim().is_empty())
                        .ok_or_else(|| {
                            CliError::Usage(format!(
                                "{provider} accepted cycle input without a provider receipt id"
                            ))
                        })?;
                    settle_provider_effect(
                        ledger,
                        &effect,
                        true,
                        Some(serde_json::json!({
                            "provider": provider,
                            "round": round,
                            "phase": "input_accepted",
                            "provider_receipt": acceptance,
                        })),
                        None,
                    )?;
                    accepted_provider_receipt = Some(provider_receipt.to_string());
                    if let Some(claimed) = cycle.active_work.as_ref() {
                        ledger.complete_work_delivery(claimed, provider_receipt)?;
                    }
                    for message in &cycle.accepted_messages {
                        mark_message_delivered(
                            ledger,
                            message,
                            &member_row.id,
                            &member_row.name,
                            provider_receipt,
                        )?;
                    }
                    if !terminal_control_dispatched.get() {
                        crate::transition_provider_session_runtime_control(
                            ledger,
                            member_row,
                            harness_core::agentfirm_api::RuntimeResidency::Attached,
                            harness_core::agentfirm_api::RuntimeActivity::Running,
                        )?;
                    }
                    Ok(())
                },
                &mut |pending, result| {
                    ledger.require_supervisor_lease()?;
                    require_provider_session_authority(
                        ledger,
                        &member_row.agent_member_id,
                        true,
                    )?;
                    match result {
                        SteerProviderResult::Acknowledged(receipt) => {
                            settle_provider_effect(
                                ledger,
                                &pending.admission,
                                true,
                                Some(serde_json::json!({
                                    "phase": "provider_input_accepted",
                                    "provider_receipt": receipt,
                                })),
                                None,
                            )?;
                            let _ = pending.reply.send(Ok(pending.success_reply.clone()));
                            Ok(())
                        }
                        SteerProviderResult::Unknown(detail) => {
                            settle_provider_effect(
                                ledger,
                                &pending.admission,
                                false,
                                None,
                                Some(detail.clone()),
                            )?;
                            let _ = pending.reply.send(Err(CliError::Usage(format!(
                                "RUNTIME_COMMAND_RECOVERY_REQUIRED: {detail}"
                            ))));
                            Ok(())
                        }
                        SteerProviderResult::NotApplied(detail) => {
                            settle_provider_effect_not_applied(
                                ledger,
                                &pending.admission,
                                detail.clone(),
                            )?;
                            let _ = pending
                                .reply
                                .send(Err(CliError::Usage(detail.clone())));
                            Ok(())
                        }
                    }
                },
                &mut |event| {
                    if let Some((kind, preview)) = A::project_live(event) {
                        if let Some(sink) = &live_sink {
                            emit_live_provider_activity(sink, ledger, member_row, kind, preview);
                        }
                    }
                },
                &mut || {
                    let mut control = CycleControl::default();
                    while let Ok(command) = live_control.try_recv() {
                        match command {
                            MemberControlCommand::Close {
                                reason,
                                requested_by,
                                reply,
                                ..
                            } => {
                                let close_request = match crate::pending_member_close(
                                    &ledger.store,
                                    &member_row.id,
                                ) {
                                    Ok(Some(close))
                                        if close.reason == reason
                                            && close.requested_by == requested_by =>
                                    {
                                        close
                                    }
                                    Ok(Some(_)) => {
                                        let detail = "RUNTIME_COMMAND_RECOVERY_REQUIRED: live Close no longer matches the exact durable Close latch".to_string();
                                        let _ = reply.send(Err(CliError::Usage(detail.clone())));
                                        control_prepare_error = Some(detail.clone());
                                        control.fatal_error = Some(detail);
                                        return control;
                                    }
                                    Ok(None) => {
                                        let detail = "RUNTIME_COMMAND_RECOVERY_REQUIRED: live Close has no durable Close latch".to_string();
                                        let _ = reply.send(Err(CliError::Usage(detail.clone())));
                                        control_prepare_error = Some(detail.clone());
                                        control.fatal_error = Some(detail);
                                        return control;
                                    }
                                    Err(error) => {
                                        let detail = error.to_string();
                                        let _ = reply.send(Err(CliError::Usage(detail.clone())));
                                        control_prepare_error = Some(detail.clone());
                                        control.fatal_error = Some(detail);
                                        return control;
                                    }
                                };
                                let mut close = false;
                                let mut interrupt = false;
                                let dispatch = {
                                    let mut proxy = A::native_control(&mut close, &mut interrupt);
                                    provider_adapter::execute_team_control(
                                        ledger,
                                        member_row,
                                        &format!("{}:active:interrupt", close_request.id),
                                        &reason,
                                        true,
                                        &mut proxy,
                                    )
                                };
                                control.close = close;
                                control.interrupt = interrupt;
                                match dispatch {
                                    Ok(ProviderControlDispatch::Pending(pending)) => {
                                        terminal_control_dispatched.set(true);
                                        pending_control_effects.push(pending);
                                        pending_control_replies.push(PendingControlReply {
                                            action: provider_adapter::ProviderControlAction::CloseSession,
                                            reply: Some(reply),
                                        });
                                    }
                                    Ok(ProviderControlDispatch::Replayed) => {
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "member_run_id": member_row.id,
                                            "status": "replayed",
                                            "provider_effect_repeated": false,
                                        })));
                                    }
                                    Err(error) => {
                                        let detail = error.to_string();
                                        let _ = reply.send(Err(CliError::Usage(detail.clone())));
                                        control_prepare_error = Some(detail.clone());
                                        control.fatal_error = Some(detail);
                                    }
                                }
                                return control;
                            }
                            MemberControlCommand::Interrupt {
                                reason,
                                requested_by,
                                reply,
                                ..
                            } => {
                                let mut close = false;
                                let mut interrupt = false;
                                let dispatch = {
                                    let mut proxy = A::native_control(&mut close, &mut interrupt);
                                    provider_adapter::execute_team_control(
                                        ledger,
                                        member_row,
                                        &format!(
                                            "{provider}-interrupt:{}:{round}:{requested_by}",
                                            member_row.id
                                        ),
                                        &reason,
                                        false,
                                        &mut proxy,
                                    )
                                };
                                control.close = close;
                                control.interrupt = interrupt;
                                match dispatch {
                                    Ok(ProviderControlDispatch::Pending(pending)) => {
                                        terminal_control_dispatched.set(true);
                                        pending_control_effects.push(pending);
                                        pending_control_replies.push(PendingControlReply {
                                            action: provider_adapter::ProviderControlAction::CancelProviderTurn,
                                            reply: Some(reply),
                                        });
                                    }
                                    Ok(ProviderControlDispatch::Replayed) => {
                                        let _ = reply.send(Ok(serde_json::json!({
                                            "member_run_id": member_row.id,
                                            "status": "replayed",
                                            "provider_effect_repeated": false,
                                        })));
                                    }
                                    Err(error) => {
                                        let detail = error.to_string();
                                        let _ = reply.send(Err(CliError::Usage(detail.clone())));
                                        control_prepare_error = Some(detail.clone());
                                        control.fatal_error = Some(detail);
                                    }
                                }
                                return control;
                            }
                            MemberControlCommand::Steer {
                                content,
                                requested_by,
                                reply,
                            } => {
                                // Only an explicit Steer command may compile
                                // into current-cycle injection. Ordinary
                                // Messages never reach this channel.
                                if supports_inject {
                                    let steer_source = format!(
                                        "{source_record_id}:steer:{requested_by}"
                                    );
                                    match crate::prepare_provider_effect_kind(
                                        ledger,
                                        member_row,
                                        &steer_source,
                                        &content,
                                        harness_core::agentfirm_api::RuntimeCommandKind::InjectCurrentCycle,
                                        "cycle.inject_current",
                                    ) {
                                        Ok(admission) => control.injects.push(PendingSteer {
                                            content,
                                            success_reply: serde_json::json!({
                                                "member_run_id": member_row.id,
                                                "status": "steer_accepted",
                                                "delivery": "steered",
                                                "provider_ack": format!(
                                                    "{provider}_native_input_accepted"
                                                ),
                                            }),
                                            reply,
                                            admission,
                                        }),
                                        Err(error) => {
                                            let _ = reply.send(Err(error));
                                        }
                                    }
                                } else {
                                    let _ = reply.send(Err(CliError::Usage(format!(
                                        "PROVIDER_CAPABILITY_UNSUPPORTED: {provider} has no current-cycle injection"
                                    ))));
                                }
                            }
                        }
                    }
                    control
                },
            )
        };
        supervisor_test_terminal_receive_barrier(provider)?;
        // A terminal callback can arrive after supervisor replacement. Fence
        // before every terminal settlement, delivery mutation, action, or
        // member/session transition; stale completions remain explicit
        // recovery work rather than being accepted by a successor.
        ledger.require_supervisor_lease()?;
        require_provider_session_authority(ledger, &member_row.agent_member_id, true)?;
        let turn = match turn_result {
            Ok(turn) if accepted_provider_receipt.is_some() => turn,
            Ok(_) => {
                let error = CliError::Usage(format!(
                    "{provider} cycle returned without a provider input-acceptance receipt"
                ));
                settle_provider_effect(ledger, &effect, false, None, Some(error.to_string()))?;
                return Err(error);
            }
            Err(error) => {
                provider_adapter::settle_team_controls_without_terminal_ack(
                    ledger,
                    pending_control_effects.drain(..),
                )?;
                fail_pending_control_replies(
                    &mut pending_control_replies,
                    format!("RUNTIME_COMMAND_RECOVERY_REQUIRED: {error}"),
                );
                if accepted_provider_receipt.is_none() {
                    settle_provider_effect_not_applied(ledger, &effect, error.to_string())?;
                }
                let action = ledger.append_action(
                    &member_row.id,
                    "provider_error",
                    MemberActionStatus::Failed,
                    &format!("{display} provider round {round} failed"),
                    &crate::provider_turn_failure_summary(display, round),
                )?;
                ledger.fold_event(
                    TeamRunEventSourceKind::Member,
                    Some(member_row.id.clone()),
                    "action",
                    &action.id,
                    "created",
                    &format!("{} provider round {round} failed", member_row.name),
                )?;
                if accepted_provider_receipt.is_some() {
                    let detail = error.to_string();
                    return Err(CliError::Usage(
                        if detail.contains("RUNTIME_COMMAND_RECOVERY_REQUIRED") {
                            detail
                        } else {
                            format!(
                            "RUNTIME_COMMAND_RECOVERY_REQUIRED: {display} failed after accepting cycle input: {detail}"
                        )
                        },
                    ));
                }
                return Err(error);
            }
        };
        // Reverse provider callbacks may legitimately advance only their
        // transient Waiting/Running projection while the native cycle is in
        // flight. Rebase that bounded same-runtime progress before the
        // terminal CAS; never let a stale outer row overwrite it or turn the
        // provider receipt into a false conflict.
        *member_row = refresh_member_after_provider_callbacks(ledger, &round_start)?;
        if turn.input_acceptance_receipt.response_id.as_deref()
            != accepted_provider_receipt.as_deref()
        {
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: {provider} terminal cycle receipt no longer matches the accepted input receipt"
            )));
        }
        if let Some(error) = control_prepare_error {
            provider_adapter::settle_team_controls_without_terminal_ack(
                ledger,
                pending_control_effects.drain(..),
            )?;
            fail_pending_control_replies(&mut pending_control_replies, error.clone());
            return Err(CliError::Usage(error));
        }

        // Busy Close has two independent provider effects. The current-cycle
        // Interrupt must first obtain its terminal acknowledgement. Only then
        // may the reversible CloseMember command dispose this owned runtime.
        // Strong Quiesce/Release are reserved for composition/driver swaps and
        // must not be weakened merely to implement Team Close.
        let abort_receipt_observed = turn
            .control_receipts
            .iter()
            .any(|receipt| receipt.command == "abort" && receipt.success);
        let cycle_terminal_observed = turn.terminal_observation.terminal_cycle_observed();
        let cycle_control_ack =
            (turn.interrupted && abort_receipt_observed && cycle_terminal_observed).then(|| {
                serde_json::json!({
                    "provider_terminal_event": "agent_settled",
                    "control_transport_receipts": &turn.control_receipts,
                    "post_abort_observation": &turn.terminal_observation,
                })
                .to_string()
            });
        let had_pending_control =
            !pending_control_effects.is_empty() || !pending_control_replies.is_empty();
        for pending in pending_control_effects.drain(..) {
            provider_adapter::settle_team_control(ledger, &pending, cycle_control_ack.as_deref())?;
        }
        if had_pending_control && cycle_control_ack.is_none() {
            fail_pending_control_replies(
                &mut pending_control_replies,
                format!(
                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: {provider} control lacked verified terminal acknowledgement"
                ),
            );
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: {provider} control lacked verified terminal acknowledgement"
            )));
        }

        let mut close_receipt = None;
        let mut close_request = None;
        if turn.close_requested_by_harness {
            let request = crate::pending_member_close(&ledger.store, &member_row.id)?.ok_or_else(|| {
                CliError::Usage(format!(
                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: {} requested Close without a durable pending close latch",
                    member_row.id
                ))
            })?;
            match execute_member_runtime_close(ledger, member_row, adapter, &request, "active") {
                Ok(receipt) => {
                    close_receipt = Some(receipt);
                    close_request = Some(request);
                }
                Err(error) => {
                    fail_pending_control_replies(&mut pending_control_replies, error.to_string());
                    return Err(error);
                }
            }
        }

        let terminal_ack = (turn.interrupted
            && abort_receipt_observed
            && cycle_terminal_observed
            && (!turn.close_requested_by_harness || close_receipt.is_some()))
        .then(|| {
            serde_json::json!({
                "provider_terminal_event": "agent_settled",
                "control_transport_receipts": &turn.control_receipts,
                "post_abort_observation": &turn.terminal_observation,
                "member_runtime_close": &close_receipt,
            })
            .to_string()
        });
        debug_assert!(!had_pending_control || terminal_ack.is_some());

        crate::transition_provider_session_runtime_control(
            ledger,
            member_row,
            if turn.close_requested_by_harness {
                harness_core::agentfirm_api::RuntimeResidency::Detached
            } else {
                harness_core::agentfirm_api::RuntimeResidency::Attached
            },
            harness_core::agentfirm_api::RuntimeActivity::Idle,
        )?;
        require_provider_session_authority(ledger, &member_row.agent_member_id, true)?;
        // Refresh the native session locator after the cycle. The locator is
        // stable identity; disposing the process-local handle must not erase
        // it.
        if turn.close_requested_by_harness {
            member_row.native_session = Some(native_session_ref(
                member_row,
                adapter.native_session_locator(),
                adapter.native_locator_kind(),
            ));
        } else {
            let expected = member_row.clone();
            member_row.native_session = Some(native_session_ref(
                member_row,
                adapter.native_session_locator(),
                adapter.native_locator_kind(),
            ));
            ledger.save_member_run(&expected, member_row)?;
        }

        drop(cycle.active_work.take());
        cycle.accepted_messages.clear();

        if turn.interrupted {
            let interruption_summary = if turn.close_requested_by_harness {
                format!("The Host explicitly closed the {display} member runtime.")
            } else if had_pending_control {
                format!("The operator or Lead interrupted the active {display} turn.")
            } else {
                format!(
                    "{display} reported the active turn as interrupted without a Harness control request; inspect the provider-native session for the source."
                )
            };
            if turn.close_requested_by_harness {
                transition_provider_session_for_member(
                    ledger,
                    member_row,
                    AgentSessionStatus::Idle,
                )?;
                let request = close_request.as_ref().expect("verified Close request");
                stop_member_for_latched_close(ledger, member_row, request)?;
            } else {
                let expected = member_row.clone();
                member_row.status = MemberRunStatus::Idle;
                member_row.finished_at = None;
                member_row.last_event_at = Some(now_string());
                ledger.save_member_run(&expected, member_row)?;
            }
            if !turn.close_requested_by_harness {
                ledger.append_action(
                    &member_row.id,
                    "interrupted",
                    MemberActionStatus::Cancelled,
                    "provider turn interrupted",
                    &interruption_summary,
                )?;
            }
            // The HTTP/API acknowledgement is a postcondition receipt, not a
            // transport write receipt. Only expose success after the durable
            // RuntimeCommand, AgentSession control state, MemberRun terminal
            // state, and action journal have all committed.
            for pending in pending_control_replies.drain(..) {
                let evidence = terminal_ack
                    .as_deref()
                    .expect("verified terminal acknowledgement for pending control");
                let status = match pending.action {
                    provider_adapter::ProviderControlAction::CancelProviderTurn => "interrupted",
                    provider_adapter::ProviderControlAction::CloseSession => "closed",
                };
                pending.send(Ok(serde_json::json!({
                    "member_run_id": member_row.id,
                    "status": status,
                    "provider_terminal_evidence": serde_json::from_str::<Value>(evidence)
                        .unwrap_or_else(|_| Value::String(evidence.to_string())),
                })));
            }
            if turn.close_requested_by_harness {
                drop(live_control_registration.take());
                return Ok(MemberOutcome::new(
                    member_row,
                    MemberRunStatus::Stopped,
                    format!("{display} member runtime closed by Host"),
                ));
            }
        } else {
            let final_text = turn.final_text;
            let provider_terminal_failure = turn.provider_terminal_failure;
            let is_zero_output = provider_terminal_failure.is_none()
                && final_text.trim().is_empty()
                && turn.tool_call_count == 0;
            if is_zero_output {
                zero_output_streak += 1;
            } else {
                zero_output_streak = 0;
            }
            let action_status = if is_zero_output || provider_terminal_failure.is_some() {
                MemberActionStatus::Failed
            } else {
                let result = parse_round_result(&final_text);
                if result == MemberRoundResult::Done {
                    MemberActionStatus::Succeeded
                } else {
                    MemberActionStatus::Failed
                }
            };
            let (action_type, action_title, round_summary, provider_status) = if let Some(failure) =
                provider_terminal_failure.as_ref()
            {
                let status = failure
                    .http_status
                    .map(|code| format!(" (HTTP {code})"))
                    .unwrap_or_default();
                (
                        "provider_error",
                        format!("{display} provider round {round} failed"),
                        format!(
                            "{display} provider round {round} failed: {}{status}; transcript remains provider-native",
                            failure.reason
                        ),
                        Some(failure.to_provider_status()),
                    )
            } else if is_zero_output {
                (
                    "empty_provider_round",
                    format!("{display} provider round {round} completed without output"),
                    provider_turn_coordination_summary(display, round, false),
                    None,
                )
            } else {
                (
                    "turn_completed",
                    format!("{display} provider round {round} completed"),
                    provider_turn_coordination_summary(
                        display,
                        round,
                        !final_text.trim().is_empty(),
                    ),
                    None,
                )
            };
            let action = ledger.append_action_with_provider_status(
                &member_row.id,
                action_type,
                action_status,
                &action_title,
                &round_summary,
                provider_status,
                &[],
            )?;
            ledger.fold_event(
                TeamRunEventSourceKind::Member,
                Some(member_row.id.clone()),
                "action",
                &action.id,
                "created",
                &format!("{} completed provider round {round}", member_row.name),
            )?;

            let expected = member_row.clone();
            member_row.zero_output_streak = zero_output_streak;
            member_row.last_consumed_work_version = last_consumed_work_version;
            member_row.status = MemberRunStatus::Idle;
            member_row.finished_at = None;
            member_row.last_event_at = Some(now_string());
            ledger.save_member_run(&expected, member_row)?;
            ledger.fold_event(
                TeamRunEventSourceKind::Member,
                Some(member_row.id.clone()),
                "member_run",
                &member_row.id,
                "updated",
                &format!("member {} idle after round {round}", member_row.name),
            )?;

            if zero_output_streak >= PROVIDER_UNPRODUCTIVE_ROUND_LIMIT {
                let mut reason = format!(
                    "{display} provider circuit breaker opened after {PROVIDER_UNPRODUCTIVE_ROUND_LIMIT} consecutive unproductive rounds (last outcome: empty terminal success). No durable agent output was produced. Provider capacity remains unknown because the runtime adapter has no reviewed quota receipt for this outcome. Inspect the provider-native session, account access, and model-specific controls before explicitly reopening the member."
                );
                if let Err(error) = ledger.fail_unreceived_work_claims_for(&member_row.id, &reason)
                {
                    reason.push_str(&format!(
                        " The unreceived Work claim also needs reconciliation: {error}"
                    ));
                }
                let expected = member_row.clone();
                member_row.status = MemberRunStatus::Failed;
                member_row.finished_at = Some(now_string());
                member_row.last_event_at = Some(now_string());
                ledger.save_member_run(&expected, member_row)?;
                let action = ledger.append_action(
                    &member_row.id,
                    "provider_circuit_breaker",
                    MemberActionStatus::Failed,
                    &format!("{display} provider circuit breaker opened"),
                    &reason,
                )?;
                ledger.fold_event(
                    TeamRunEventSourceKind::Member,
                    Some(member_row.id.clone()),
                    "action",
                    &action.id,
                    "created",
                    &format!(
                        "member {} failed after repeated unproductive provider rounds",
                        member_row.name
                    ),
                )?;
                drop(live_control_registration.take());
                return Ok(MemberOutcome::new(
                    member_row,
                    MemberRunStatus::Failed,
                    reason,
                ));
            }
        }

        cycle = match await_next_cycle(
            ledger,
            objective,
            context,
            member_row,
            adapter,
            live_control,
            zero_output_streak,
            last_consumed_work_version,
            &wake_policy,
            &mut wake_backoff,
        )? {
            Ok(next) => {
                if let Some(version) = next.consumed_work_version {
                    last_consumed_work_version = Some(version);
                }
                if next.active_work.is_some() {
                    wake_backoff.reset();
                }
                next
            }
            Err(outcome) => return Ok(outcome),
        };
    }
}

// ---------------------------------------------------------------------------
// Binding registry
// ---------------------------------------------------------------------------

/// The closed set of persistent Team runtime implementations. This one
/// selector is shared by admission, capability reporting, and runner dispatch
/// so a provider cannot advertise an executable binding without a runnable
/// path (or acquire a runnable path without a capability contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SharedTeamRuntimeKind {
    Pi,
    Kimi,
    Codex,
    Claude,
}

pub(crate) fn shared_team_runtime_kind(
    provider: &str,
    execution_mode: Option<&str>,
) -> Option<SharedTeamRuntimeKind> {
    match (provider, execution_mode) {
        ("pi", Some("pi_rpc") | None) => Some(SharedTeamRuntimeKind::Pi),
        ("kimi", Some("kimi_acp") | None) => Some(SharedTeamRuntimeKind::Kimi),
        ("codex", Some("codex_app_server") | None) => Some(SharedTeamRuntimeKind::Codex),
        ("claude", Some("claude_agent_sdk") | None) => Some(SharedTeamRuntimeKind::Claude),
        _ => None,
    }
}

/// Executable capability report for a provider's Team runtime binding, when
/// one exists. `None` means no provider-neutral Team runtime binding is
/// registered and persistent Team execution must fail closed.
pub(crate) fn capability_bindings_for(provider: &str) -> Option<Vec<CapabilityBinding>> {
    match shared_team_runtime_kind(provider, None)? {
        SharedTeamRuntimeKind::Pi => Some(crate::pi_rpc::PiTeamRuntime::capability_bindings()),
        SharedTeamRuntimeKind::Kimi => {
            Some(crate::kimi_team_runtime::KimiTeamRuntime::capability_bindings())
        }
        SharedTeamRuntimeKind::Codex => Some(crate::codex_team_runtime::capability_bindings()),
        SharedTeamRuntimeKind::Claude => {
            Some(crate::claude_team_runtime::ClaudeTeamRuntime::capability_bindings())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::SecurityEnforcementLocusKind;

    #[test]
    fn binding_registry_is_the_closed_runner_dispatch_registry() {
        for (provider, mode, kind) in [
            ("pi", "pi_rpc", SharedTeamRuntimeKind::Pi),
            ("kimi", "kimi_acp", SharedTeamRuntimeKind::Kimi),
            ("codex", "codex_app_server", SharedTeamRuntimeKind::Codex),
            ("claude", "claude_agent_sdk", SharedTeamRuntimeKind::Claude),
        ] {
            assert_eq!(shared_team_runtime_kind(provider, Some(mode)), Some(kind));
            assert_eq!(shared_team_runtime_kind(provider, None), Some(kind));
            assert!(capability_bindings_for(provider).is_some());
        }
        for (provider, mode) in [
            ("codex", "codex_exec"),
            ("claude", "claude_cli"),
            ("kimi", "pi_rpc"),
            ("unknown", "unknown"),
        ] {
            assert_eq!(shared_team_runtime_kind(provider, Some(mode)), None);
        }
        assert!(capability_bindings_for("unknown").is_none());
    }

    #[test]
    fn pi_launch_policy_only_compiles_admissible_ceilings() {
        assert_eq!(
            pi_tools_allowlist_for_ceiling(PermissionCeiling::ReadOnly).unwrap(),
            Some("read,grep,find,ls")
        );
        assert!(pi_tools_allowlist_for_ceiling(PermissionCeiling::WorkspaceWrite).is_err());
        assert_eq!(
            pi_tools_allowlist_for_ceiling(PermissionCeiling::FullAccess).unwrap(),
            None
        );
    }

    #[test]
    fn readonly_allowlist_never_includes_mutating_tools() {
        let allowlist = pi_tools_allowlist_for_ceiling(PermissionCeiling::ReadOnly)
            .unwrap()
            .unwrap();
        for forbidden in ["bash", "write", "edit"] {
            assert!(!allowlist.split(',').any(|tool| tool == forbidden));
        }
    }

    #[test]
    fn enforcement_locus_matches_compilation() {
        let restricted = pi_security_enforcement_locus(PermissionCeiling::ReadOnly);
        assert_eq!(
            restricted.kind,
            SecurityEnforcementLocusKind::AdapterToolAllowlist
        );
        let workspace_write = pi_security_enforcement_locus(PermissionCeiling::WorkspaceWrite);
        assert_eq!(
            workspace_write.kind,
            SecurityEnforcementLocusKind::NoneVerified
        );
        let full = pi_security_enforcement_locus(PermissionCeiling::FullAccess);
        assert_eq!(full.kind, SecurityEnforcementLocusKind::NoneVerified);
    }

    #[test]
    fn pi_permission_admission_fails_closed_without_filesystem_containment() {
        assert!(admit_pi_permission_ceiling(
            PermissionCeiling::ReadOnly,
            Some("read,grep,find,ls")
        )
        .is_ok());
        assert!(admit_pi_permission_ceiling(PermissionCeiling::FullAccess, None).is_ok());
        let error = admit_pi_permission_ceiling(
            PermissionCeiling::WorkspaceWrite,
            Some("read,grep,find,ls,write,edit"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("filesystem containment"));
        let error = admit_pi_permission_ceiling(PermissionCeiling::ReadOnly, None).unwrap_err();
        assert!(error.to_string().contains("expected tools"));
    }

    #[test]
    fn pi_capability_bindings_are_honest() {
        let bindings = capability_bindings_for("pi").expect("pi binding report");
        // Every Supported claim must name its evidence.
        for binding in &bindings {
            if binding.status.is_supported() {
                assert!(
                    !binding.evidence.trim().is_empty(),
                    "{} is Supported without evidence",
                    binding.capability
                );
            }
        }
        // Continuation intents are honestly Unsupported: Pi has no native Goal.
        for capability in [
            "inspect_continuation",
            "inhibit_continuation",
            "resume_continuation",
        ] {
            let binding = bindings
                .iter()
                .find(|binding| binding.capability == capability)
                .unwrap();
            assert_eq!(
                binding.status,
                CapabilityStatus::Unsupported,
                "{capability}"
            );
        }
        // reconcile_effect was the static-matrix overclaim; the executable
        // report must not claim it.
        let reconcile = bindings
            .iter()
            .find(|binding| binding.capability == "reconcile_effect")
            .unwrap();
        assert!(!reconcile.status.is_supported());
        // Permission enforcement is conditional per admitted session. The
        // static binding must not describe trusted full_access as verified.
        let permission = bindings
            .iter()
            .find(|binding| binding.capability == "permission_enforcement")
            .unwrap();
        assert_eq!(permission.status, CapabilityStatus::Degraded);
        assert!(permission.security_enforcement_locus.is_none());
    }

    #[test]
    fn kimi_capability_bindings_match_the_reviewed_acp_surface() {
        let bindings = capability_bindings_for("kimi").expect("Kimi ACP binding report");
        for capability in [
            "open_or_resume",
            "start_cycle",
            "interrupt_current_cycle",
            "observe",
        ] {
            let binding = bindings
                .iter()
                .find(|binding| binding.capability == capability)
                .unwrap();
            assert_eq!(binding.status, CapabilityStatus::Supported, "{capability}");
            assert!(!binding.evidence.trim().is_empty(), "{capability}");
        }
        for capability in [
            "inject_current_cycle",
            "queue_at_native_boundary",
            "inspect_continuation",
            "reconcile_effect",
        ] {
            let binding = bindings
                .iter()
                .find(|binding| binding.capability == capability)
                .unwrap();
            assert_eq!(
                binding.status,
                CapabilityStatus::Unsupported,
                "{capability}"
            );
        }
        for capability in ["quiesce", "release", "permission_enforcement"] {
            let binding = bindings
                .iter()
                .find(|binding| binding.capability == capability)
                .unwrap();
            assert_eq!(binding.status, CapabilityStatus::Degraded, "{capability}");
        }
    }

    #[test]
    fn unknown_providers_without_a_binding_report_none() {
        // DeepSeek needs no new core object to plug in later — until its
        // bridge exists, the honest report is "no executable binding yet".
        assert!(capability_bindings_for("deepseek").is_none());
        for provider in ["codex", "claude"] {
            let bindings = capability_bindings_for(provider).expect("executable binding report");
            assert!(bindings.iter().any(|binding| {
                binding.capability == "close_runtime"
                    && binding.status == CapabilityStatus::Supported
            }));
        }
    }
}
