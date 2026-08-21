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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use serde_json::Value;

use harness_core::agentfirm_api::{AgentSessionStatus, PermissionCeiling};

use crate::provider_adapter::{
    self, PendingProviderControl, ProviderControlDispatch, ProviderNativeControl,
};
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

#[path = "runtime_adapter_capabilities.rs"]
mod capabilities;
pub(crate) use capabilities::*;

// ---------------------------------------------------------------------------
// Cycle control and outcome
// ---------------------------------------------------------------------------

/// Harness control intents delivered mid-cycle. Ordinary Messages never
/// appear here — they remain in the durable queue until the cycle settles.
pub(crate) use harness_runtime_contract::{
    CapabilityBinding, CapabilityStatus, ControlTransportReceipt, CycleControl,
    CycleRuntimeObservation as RuntimeObservation, ExecutionCycleOutcome, LiveProviderActivityKind,
    QuiesceOutcome, SteerProviderResult, SteerRequest,
};

struct PendingSteerSettlement {
    success_reply: Value,
    reply: Option<std::sync::mpsc::SyncSender<CliResult<Value>>>,
    admission: crate::ProviderEffectAdmission,
}

impl Drop for PendingSteerSettlement {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Err(CliError::Usage(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: steer ended without provider settlement"
                    .to_string(),
            )));
        }
    }
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
        on_steer_result: &mut dyn FnMut(&SteerRequest, &SteerProviderResult) -> CliResult<()>,
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
        let pending_steers: RefCell<HashMap<u64, PendingSteerSettlement>> =
            RefCell::new(HashMap::new());
        let next_steer_token = Cell::new(0u64);

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
                &mut |request, result| {
                    ledger.require_supervisor_lease()?;
                    require_provider_session_authority(
                        ledger,
                        &member_row.agent_member_id,
                        true,
                    )?;
                    let mut pending = pending_steers
                        .borrow_mut()
                        .remove(&request.token)
                        .ok_or_else(|| {
                            CliError::Usage(format!(
                                "RUNTIME_COMMAND_RECOVERY_REQUIRED: unknown steer token {}",
                                request.token
                            ))
                        })?;
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
                            if let Some(response_id) = receipt.response_id.as_ref() {
                                pending.success_reply["provider_response_id"] =
                                    response_id.clone().into();
                            }
                            if let Some(reply) = pending.reply.take() {
                                let _ = reply.send(Ok(pending.success_reply.clone()));
                            }
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
                            if let Some(reply) = pending.reply.take() {
                                let _ = reply.send(Err(CliError::Usage(format!(
                                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: {detail}"
                                ))));
                            }
                            Ok(())
                        }
                        SteerProviderResult::NotApplied(detail) => {
                            settle_provider_effect_not_applied(
                                ledger,
                                &pending.admission,
                                detail.clone(),
                            )?;
                            if let Some(reply) = pending.reply.take() {
                                let _ = reply.send(Err(CliError::Usage(detail.clone())));
                            }
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
                                        Ok(admission) => {
                                            let token = next_steer_token.get();
                                            next_steer_token.set(token.saturating_add(1));
                                            pending_steers.borrow_mut().insert(token, PendingSteerSettlement {
                                                success_reply: serde_json::json!({
                                                "member_run_id": member_row.id,
                                                "status": "steer_accepted",
                                                "delivery": "steered",
                                                "provider_ack": format!(
                                                    "{provider}_native_input_accepted"
                                                ),
                                                }),
                                                reply: Some(reply),
                                                admission,
                                            });
                                            control.injects.push(SteerRequest { token, content });
                                        }
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
