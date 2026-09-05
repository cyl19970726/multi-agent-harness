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

use harness_application::{
    circuit_breaker_reason, decide_team_round, verified_terminal_control_ack, RoundActionStatus,
};
use harness_core::agentfirm_api::{AgentSessionStatus, PermissionCeiling};

use crate::provider_adapter::{self, PendingProviderControl, ProviderControlDispatch};
use crate::settlements::{APPLIED_SATISFIED, UNPROVEN};
use crate::supervisor_wake::{WakeBackoff, WakePolicy};
use crate::{
    active_work_continuation_prompt, emit_native_session_wake, mark_message_delivered,
    member_work_collaboration_envelope, native_session_ref, now_string, parse_round_result,
    prepare_provider_effect, record_provider_cycle_correlation,
    refresh_member_after_provider_callbacks, requeue_managed_host_attentions,
    require_provider_session_authority, settle_managed_host_attentions, settle_provider_effect,
    settle_provider_effect_not_applied, stop_member_for_latched_close, team_messages_prompt,
    transition_provider_session_for_member, wait_for_idle_member_wake, work_contract_prompt,
    ClaimedWork, CliError, CliResult, ControlReceiver, HostAttention, IdleMemberWake,
    LiveMemberControlRegistration, MemberActionStatus, MemberControlCommand, MemberOutcome,
    MemberRoundResult, MemberRunStatus, MemberRuntimeContext, NativeSessionWakeGuard,
    ProviderRuntimeProjection, TeamMessageProjection, TeamRunEventSourceKind, TeamRunLedger,
};

#[path = "runtime_adapter/native_session_binding.rs"]
mod native_session_binding;

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
    CapabilityBinding, CapabilityStatus, CycleControl, SteerProviderResult, SteerRequest,
    TeamRuntimeAdapter,
};

struct PendingSteerSettlement {
    success_reply: Value,
    reply: Option<std::sync::mpsc::SyncSender<CliResult<Value>>>,
    admission: crate::ProviderEffectAdmission,
}

impl Drop for PendingSteerSettlement {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Err(CliError::RuntimeRecoveryRequired(
                "steer ended without provider settlement".to_string(),
            )));
        }
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
    host_attentions: Vec<HostAttention>,
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
            let _ = reply.send(Err(CliError::RuntimeRecoveryRequired(
                "provider control ended before its durable postcondition receipt was published"
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
fn idle_wake_into_cycle<A: TeamRuntimeAdapter<Error = CliError>>(
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
                host_attentions: Vec::new(),
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
                host_attentions: Vec::new(),
                consumed_work_version: Some(consumed),
            }))
        }
        IdleMemberWake::Messages {
            messages,
            host_attentions,
        } => {
            let mut prompt = team_messages_prompt(
                "TEAM MESSAGES arrived. They are conversation, not Work ownership. \
                 Address the question or coordination request, and use the Works \
                 board for any durable responsibility.",
                &messages,
            );
            if !host_attentions.is_empty() {
                prompt.push_str(
                    "\n\nBATCHED TEAM STATUS (coordination facts, not Work ownership):\n",
                );
                for attention in &host_attentions {
                    prompt.push_str(&format!(
                        "- {:?}: work={} version={} source={}\n",
                        attention.kind,
                        attention.work_id,
                        attention.work_version,
                        attention.source_event_ref
                    ));
                }
            }
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: messages,
                host_attentions,
                consumed_work_version: None,
            }))
        }
        IdleMemberWake::HostAttentions(attentions) => {
            let mut prompt = String::from(
                "TEAM STATUS ATTENTION arrived for the Host. These are durable coordination facts, not new Work ownership. Review, respond, or route only when a decision is required.\n\n",
            );
            for attention in &attentions {
                prompt.push_str(&format!(
                    "- {:?}: work={} version={} source={} member_run={}\n",
                    attention.kind,
                    attention.work_id,
                    attention.work_version,
                    attention.source_event_ref,
                    attention.member_run_id.as_deref().unwrap_or("none")
                ));
            }
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: Vec::new(),
                host_attentions: attentions,
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
fn close_idle_runtime<A: TeamRuntimeAdapter<Error = CliError>>(
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
fn execute_member_runtime_close<A: TeamRuntimeAdapter<Error = CliError>>(
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
        None,
    )?;
    let close_admission = adapter
        .bind_authority_session(effect.target_session.clone(), &profile)
        .and_then(|_| {
            crate::runtime_adapter_contract::preflight_effect(
                adapter.describe(),
                &effect.target_session,
                effect.fence.clone(),
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
    let close_receipt = match adapter.close_runtime(effect.fence.clone()) {
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
            settle_provider_effect(ledger, &effect, UNPROVEN, None, Some(error.to_string()))?;
            return Err(CliError::RuntimeRecoveryRequired(format!(
                "{} {boundary} Close is unproven: {error}",
                adapter.provider()
            )));
        }
    };
    if let Err(error) = close_receipt.verify() {
        settle_provider_effect(ledger, &effect, UNPROVEN, None, Some(error.to_string()))?;
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "{} {boundary} Close receipt is incomplete: {error}",
            adapter.provider()
        )));
    }
    settle_provider_effect(
        ledger,
        &effect,
        APPLIED_SATISFIED,
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
fn await_next_cycle<A: TeamRuntimeAdapter<Error = CliError>>(
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

struct RuntimeSupervisorState<'a, A> {
    ledger: &'a TeamRunLedger,
    objective: &'a str,
    member_row: &'a mut ProviderRuntimeProjection,
    context: &'a MemberRuntimeContext,
    adapter: &'a mut A,
    live_control: &'a ControlReceiver<MemberControlCommand>,
    live_control_registration: Option<LiveMemberControlRegistration>,
    supports_inject: bool,
    provider: &'static str,
    display: &'static str,
    wake_policy: WakePolicy,
    wake_backoff: WakeBackoff,
    zero_output_streak: u32,
    last_consumed_work_version: Option<u64>,
}

struct DrivenRuntimeCycle {
    cycle: CycleInput,
    effect: crate::ProviderEffectAdmission,
    accepted_provider_receipt: Option<String>,
    pending_control_effects: Vec<PendingProviderControl>,
    pending_control_replies: Vec<PendingControlReply>,
    control_prepare_error: Option<String>,
    round_start: ProviderRuntimeProjection,
    turn_result: CliResult<harness_runtime_contract::ExecutionCycleOutcome>,
}

/// The provider-neutral persistent member loop: wake → claim → drive one
/// ExecutionCycle → settle receipts → repeat. One implementation for every
/// binding; provider differences live behind `TeamRuntimeAdapter`, not in a
/// branded loop per provider.
///
/// The binding's prepare step owns provider-specific spawn/policy work and
/// hands over a live adapter plus the registered control channel.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_team_member_with_adapter<A: TeamRuntimeAdapter<Error = CliError>>(
    ledger: &TeamRunLedger,
    objective: &str,
    member_row: &mut ProviderRuntimeProjection,
    context: &MemberRuntimeContext,
    adapter: &mut A,
    live_control: &ControlReceiver<MemberControlCommand>,
    live_control_registration: Option<LiveMemberControlRegistration>,
    provider_attempt: u64,
) -> CliResult<MemberOutcome> {
    let supports_inject = adapter.supports_inject_current_cycle();
    let provider = adapter.provider();
    let display = adapter.display_name();

    let zero_output_streak = member_row.zero_output_streak;
    let last_consumed_work_version = member_row.last_consumed_work_version;
    let state = RuntimeSupervisorState {
        ledger,
        objective,
        member_row,
        context,
        adapter,
        live_control,
        live_control_registration,
        supports_inject,
        provider,
        display,
        wake_policy: crate::supervisor_wake::effective_wake_policy(),
        wake_backoff: WakeBackoff::new(),
        zero_output_streak,
        last_consumed_work_version,
    };

    use harness_runtime_supervisor::{
        run_team_supervisor, SupervisorDirective, SupervisorPortFn, SupervisorWake,
    };
    let mut supervisor_port = SupervisorPortFn::new(
        state,
        |state: &mut RuntimeSupervisorState<'_, A>, _round| match await_next_cycle(
            state.ledger,
            state.objective,
            state.context,
            state.member_row,
            state.adapter,
            state.live_control,
            state.zero_output_streak,
            state.last_consumed_work_version,
            &state.wake_policy,
            &mut state.wake_backoff,
        )? {
            Ok(cycle) => {
                if let Some(version) = cycle.consumed_work_version {
                    state.last_consumed_work_version = Some(version);
                }
                if cycle.active_work.is_some() {
                    state.wake_backoff.reset();
                }
                Ok(SupervisorWake::Cycle(cycle))
            }
            Err(outcome) => Ok(SupervisorWake::Complete(outcome)),
        },
        |state: &mut RuntimeSupervisorState<'_, A>, round, cycle: CycleInput| {
            let RuntimeSupervisorState {
                ledger,
                member_row,
                context,
                adapter,
                live_control,
                supports_inject,
                provider,
                ..
            } = state;
            let ledger = *ledger;
            let member_row = &mut **member_row;
            let context = *context;
            let adapter = &mut **adapter;
            let live_control = *live_control;
            let supports_inject = *supports_inject;
            let provider = *provider;
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
                .or_else(|| {
                    cycle
                        .host_attentions
                        .last()
                        .map(|attention| attention.id.as_str())
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
            let effect = match prepare_provider_effect(
                ledger,
                member_row,
                &source_record_id,
                &prompt,
                provider_attempt,
            ) {
                Ok(effect) => effect,
                Err(error) => {
                    requeue_managed_host_attentions(
                        ledger,
                        &cycle.host_attentions,
                        &error.to_string(),
                    )?;
                    return Err(error);
                }
            };
            let profile = member_row.provider_profile.as_ref().ok_or_else(|| {
                CliError::Usage(format!(
                    "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
                    member_row.id
                ))
            })?;
            let adapter_admission = adapter
                .bind_authority_session(effect.target_session.clone(), profile)
                .and_then(|()| {
                    preflight_start_cycle(adapter, &effect.target_session, &effect.fence)
                });
            if let Err(error) = adapter_admission {
                settle_provider_effect_not_applied(ledger, &effect, error.to_string())?;
                requeue_managed_host_attentions(
                    ledger,
                    &cycle.host_attentions,
                    &error.to_string(),
                )?;
                return Err(error);
            }
            let mut accepted_provider_receipt: Option<String> = None;
            let mut pending_control_effects: Vec<PendingProviderControl> = Vec::new();
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
            let early_native_binding_error = RefCell::new(None::<String>);
            let native_locator_kind = adapter.native_locator_kind().to_string();

            let mut round_start = member_row.clone();
            let turn_result = {
                let _turn_lease = context.turn_leases.acquire();
                let _native_session_wake_guard = NativeSessionWakeGuard::new(
                    context.live_sink.clone(),
                    ledger.run_id.clone(),
                    member_row.agent_member_id.clone(),
                    member_row.id.clone(),
                    member_row.runtime_generation,
                );
                let live_sink = context.live_sink.clone();
                adapter.run_cycle(
                &prompt,
                context.timeouts,
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
                        APPLIED_SATISFIED,
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
                    settle_managed_host_attentions(
                        ledger,
                        &cycle.host_attentions,
                        provider_receipt,
                    )?;
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
                            CliError::RuntimeRecoveryRequired(format!(
                                "unknown steer token {}",
                                request.token
                            ))
                        })?;
                    match result {
                        SteerProviderResult::Acknowledged(receipt) => {
                            settle_provider_effect(
                                ledger,
                                &pending.admission,
                                APPLIED_SATISFIED,
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
                                UNPROVEN,
                                None,
                                Some(detail.clone()),
                            )?;
                            if let Some(reply) = pending.reply.take() {
                                let _ = reply.send(Err(CliError::RuntimeRecoveryRequired(
                                    detail.clone(),
                                )));
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
                    if provider == "claude"
                        && event.get("event").and_then(serde_json::Value::as_str)
                            == Some("session_bound")
                    {
                        let binding_result =
                            native_session_binding::persist_verified_claude_session_binding(
                                ledger,
                                &round_start,
                                event,
                                &native_locator_kind,
                            );
                        match binding_result {
                            Ok(bound) => round_start = bound,
                            Err(error) => {
                                *early_native_binding_error.borrow_mut() = Some(error.to_string());
                            }
                        }
                    }
                    if let Some(sink) = &live_sink {
                        emit_native_session_wake(sink, ledger, &round_start);
                    }
                },
                &mut || {
                    let mut control = CycleControl::default();
                    if let Some(error) = early_native_binding_error.borrow().clone() {
                        control.fatal_error = Some(error);
                        return control;
                    }
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
                                        let detail = "live Close no longer matches the exact durable Close latch".to_string();
                                        let _ = reply.send(Err(CliError::RuntimeRecoveryRequired(detail.clone())));
                                        control_prepare_error = Some(detail.clone());
                                        control.fatal_error = Some(detail);
                                        return control;
                                    }
                                    Ok(None) => {
                                        let detail = "live Close has no durable Close latch".to_string();
                                        let _ = reply.send(Err(CliError::RuntimeRecoveryRequired(detail.clone())));
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
                                        pending_control_effects.push(*pending);
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
                                        pending_control_effects.push(*pending);
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
                                        None,
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
            Ok(DrivenRuntimeCycle {
                cycle,
                effect,
                accepted_provider_receipt,
                pending_control_effects,
                pending_control_replies,
                control_prepare_error,
                round_start,
                turn_result,
            })
        },
        |state: &mut RuntimeSupervisorState<'_, A>, round, driven: DrivenRuntimeCycle| {
            let RuntimeSupervisorState {
                ledger,
                member_row,
                adapter,
                live_control_registration,
                provider,
                display,
                zero_output_streak,
                last_consumed_work_version,
                ..
            } = state;
            let ledger = *ledger;
            let member_row = &mut **member_row;
            let adapter = &mut **adapter;
            let provider = *provider;
            let display = *display;
            let DrivenRuntimeCycle {
                mut cycle,
                effect,
                accepted_provider_receipt,
                mut pending_control_effects,
                mut pending_control_replies,
                control_prepare_error,
                round_start,
                turn_result,
            } = driven;
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
                    settle_provider_effect(
                        ledger,
                        &effect,
                        UNPROVEN,
                        None,
                        Some(error.to_string()),
                    )?;
                    requeue_managed_host_attentions(
                        ledger,
                        &cycle.host_attentions,
                        &error.to_string(),
                    )?;
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
                        requeue_managed_host_attentions(
                            ledger,
                            &cycle.host_attentions,
                            &error.to_string(),
                        )?;
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
                        return Err(match error {
                            CliError::RuntimeRecoveryRequired(detail) => {
                                CliError::RuntimeRecoveryRequired(detail)
                            }
                            error => CliError::RuntimeRecoveryRequired(format!(
                                "{display} failed after accepting cycle input: {error}"
                            )),
                        });
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
            // A fresh provider runtime learns its native session only after
            // the first input has crossed the provider boundary. Persist that
            // one same-generation attachment before correlating the terminal
            // frame so Store can prove it against the current AgentSession.
            // An existing non-empty locator may never be replaced here.
            let terminal_native_session_id = adapter.native_session_locator().trim();
            if terminal_native_session_id.is_empty() {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "{provider} terminal cycle has no exact native session binding"
                )));
            }
            match member_row.native_session.as_ref() {
                Some(existing) if existing.native_session_id != terminal_native_session_id => {
                    return Err(CliError::RuntimeRecoveryRequired(format!(
                        "{provider} terminal cycle attempted to replace the bound native session"
                    )));
                }
                Some(_) => {}
                None => {
                    let expected = member_row.clone();
                    member_row.native_session = Some(native_session_ref(
                        member_row,
                        terminal_native_session_id,
                        adapter.native_locator_kind(),
                    ));
                    ledger.save_member_run(&expected, member_row)?;
                }
            }
            if turn
                .native_correlation
                .input_acceptance_receipt
                .response_id
                .as_deref()
                != accepted_provider_receipt.as_deref()
            {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "{provider} terminal cycle receipt no longer matches the accepted input receipt"
                )));
            }
            let cycle_terminal_observed = turn.terminal_observation.terminal_cycle_observed();
            let (cycle_correlation, cycle_outcome) = harness_application::correlate_provider_cycle(
                harness_application::ProviderCycleAuthority {
                    invocation_id: effect.command_id.clone(),
                    source_delivery_id: cycle
                        .active_work
                        .as_ref()
                        .map(|claimed| claimed.delivery.id.clone()),
                    native_session_id: terminal_native_session_id.to_string(),
                    agent_session_generation: effect.target_session.runtime_generation,
                    provider_attempt,
                },
                turn.native_correlation.clone(),
                cycle_terminal_observed,
                turn.interrupt.clone(),
            )
            .map_err(CliError::RuntimeRecoveryRequired)?;
            if matches!(
                cycle_outcome,
                harness_application::CycleOutcome::StillRunning
            ) {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "{provider} returned before the exact provider cycle terminal boundary"
                )));
            }
            record_provider_cycle_correlation(ledger, &effect, &cycle_correlation)?;
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
            let cycle_control_ack = verified_terminal_control_ack(
                turn.interrupt.is_some(),
                abort_receipt_observed,
                cycle_terminal_observed,
                false,
                false,
            )
            .then(|| {
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
                provider_adapter::settle_team_control(
                    ledger,
                    &pending,
                    cycle_control_ack.as_deref(),
                )?;
            }
            if had_pending_control && cycle_control_ack.is_none() {
                fail_pending_control_replies(
                &mut pending_control_replies,
                format!(
                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: {provider} control lacked verified terminal acknowledgement"
                ),
            );
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "{provider} control lacked verified terminal acknowledgement"
                )));
            }

            let mut close_receipt = None;
            let mut close_request = None;
            if turn.close_requested_by_harness {
                let request = crate::pending_member_close(&ledger.store, &member_row.id)?
                    .ok_or_else(|| {
                        CliError::RuntimeRecoveryRequired(format!(
                            "{} requested Close without a durable pending close latch",
                            member_row.id
                        ))
                    })?;
                match execute_member_runtime_close(ledger, member_row, adapter, &request, "active")
                {
                    Ok(receipt) => {
                        close_receipt = Some(receipt);
                        close_request = Some(request);
                    }
                    Err(error) => {
                        fail_pending_control_replies(
                            &mut pending_control_replies,
                            error.to_string(),
                        );
                        return Err(error);
                    }
                }
            }

            let terminal_ack = verified_terminal_control_ack(
                turn.interrupt.is_some(),
                abort_receipt_observed,
                cycle_terminal_observed,
                turn.close_requested_by_harness,
                close_receipt.is_some(),
            )
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

            if turn.interrupt.is_some() {
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
                        provider_adapter::ProviderControlAction::CancelProviderTurn => {
                            "interrupted"
                        }
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
                    return Ok(SupervisorDirective::Complete(MemberOutcome::new(
                        member_row,
                        MemberRunStatus::Stopped,
                        format!("{display} member runtime closed by Host"),
                    )));
                }
            } else {
                let final_text = turn.final_text;
                let provider_terminal_failure = turn.provider_terminal_failure;
                let semantic_done = parse_round_result(&final_text) == MemberRoundResult::Done;
                let decision = decide_team_round(
                    display,
                    round,
                    &final_text,
                    turn.tool_call_count,
                    provider_terminal_failure.as_ref(),
                    semantic_done,
                    *zero_output_streak,
                );
                *zero_output_streak = decision.zero_output_streak;
                let action_status = match decision.action_status {
                    RoundActionStatus::Succeeded => MemberActionStatus::Succeeded,
                    RoundActionStatus::Failed => MemberActionStatus::Failed,
                };
                let action = ledger.append_action_with_provider_status(
                    &member_row.id,
                    decision.action_type,
                    action_status,
                    &decision.action_title,
                    &decision.summary,
                    decision.provider_status,
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
                member_row.zero_output_streak = *zero_output_streak;
                member_row.last_consumed_work_version = *last_consumed_work_version;
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

                if decision.circuit_breaker_open {
                    let mut reason = circuit_breaker_reason(display);
                    if let Err(error) =
                        ledger.fail_unreceived_work_claims_for(&member_row.id, &reason)
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
                    return Ok(SupervisorDirective::Complete(MemberOutcome::new(
                        member_row,
                        MemberRunStatus::Failed,
                        reason,
                    )));
                }
            }

            Ok(SupervisorDirective::Continue)
        },
    );
    run_team_supervisor(&mut supervisor_port)
}
