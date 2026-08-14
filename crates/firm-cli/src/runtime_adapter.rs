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
    require_provider_session_authority, settle_provider_effect, team_message_kind_label,
    transition_provider_session_for_member, wait_for_idle_member_wake, work_contract_prompt,
    ClaimedWork, CliError, CliResult, ControlReceiver, IdleMemberWake,
    LiveMemberControlRegistration, LiveProviderTurnGuard, MemberActionStatus, MemberControlCommand,
    MemberCoordinationStatus, MemberOutcome, MemberRoundResult, MemberRunStatus,
    MemberRuntimeContext, ProviderRuntimeProjection, TeamMessageProjection, TeamRunEventSourceKind,
    TeamRunLedger,
};

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
#[derive(Debug, Default, Clone)]
pub(crate) struct CycleControl {
    pub close: bool,
    pub interrupt: bool,
    /// Explicit Steer command bodies to compile into current-cycle injection
    /// at the binding's next control boundary.
    pub injects: Vec<String>,
}

/// One ExecutionCycle's settled outcome. `interrupted` /
/// `close_requested_by_harness` describe the cycle only — neither is a Work
/// or Message acceptance.
#[derive(Debug, Clone)]
pub(crate) struct ExecutionCycleOutcome {
    pub final_text: String,
    pub interrupted: bool,
    pub close_requested_by_harness: bool,
    pub tool_call_count: u32,
}

// ---------------------------------------------------------------------------
// The adapter contract
// ---------------------------------------------------------------------------

/// The provider-neutral semantic-intent surface for a persistent Team member
/// runtime. Intent names follow DOC-89 §8.1; bindings compile them into
/// provider primitives (Pi: coding-agent RPC; Codex: app-server; a future
/// DeepSeek native bridge needs no new core object to plug in here).
pub(crate) trait TeamRuntimeAdapter {
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

    /// ExecutionCycle: drive one accepted input to its settled boundary.
    /// `on_event` sees raw provider frames for live projection;
    /// `poll_control` drains pending Harness control intents.
    fn run_cycle(
        &mut self,
        input: &str,
        idle_timeout: Duration,
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
// Permission ceiling → Pi tool allowlist compilation
// ---------------------------------------------------------------------------

/// Compile a permission ceiling into a Pi `--tools` allowlist. This is the
/// adapter-level enforcement locus for Pi: the string is actually passed to
/// the spawned process, not just generated and dropped.
///
/// - `ReadOnly` → read-only tools only.
/// - `WorkspaceWrite` → read + workspace-write tools, **no `bash`**: a shell
///   escapes the workspace boundary and cannot be verified by the adapter.
/// - `FullAccess` → `None`: the Pi default toolset (including `bash`) runs
///   unrestricted. This is only honest when the profile records
///   `security_enforcement_locus = none_verified` — the adapter enforces
///   nothing and says so.
pub(crate) fn pi_tools_allowlist_for_ceiling(ceiling: PermissionCeiling) -> Option<&'static str> {
    match ceiling {
        PermissionCeiling::ReadOnly => Some("read,grep,find,ls"),
        PermissionCeiling::WorkspaceWrite => Some("read,grep,find,ls,write,edit"),
        PermissionCeiling::FullAccess => None,
    }
}

/// The enforcement-locus claim matching `pi_tools_allowlist_for_ceiling`.
pub(crate) fn pi_security_enforcement_locus(
    ceiling: PermissionCeiling,
) -> harness_core::SecurityEnforcementLocus {
    use harness_core::{SecurityEnforcementLocus, SecurityEnforcementLocusKind};
    match ceiling {
        PermissionCeiling::ReadOnly | PermissionCeiling::WorkspaceWrite => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::AdapterToolAllowlist,
            note: Some(
                "compiled to `pi --tools <allowlist>` at spawn; bash is withheld below full access"
                    .to_string(),
            ),
        },
        PermissionCeiling::FullAccess => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::NoneVerified,
            note: Some(
                "full access runs the Pi default toolset; no adapter-level enforcement verified"
                    .to_string(),
            ),
        },
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

/// The two previously per-loop, twice-per-loop wake match blocks collapsed
/// into one shared projection.
fn idle_wake_into_cycle<A: TeamRuntimeAdapter>(
    wake: IdleMemberWake,
    ledger: &TeamRunLedger,
    objective: &str,
    context: &MemberRuntimeContext,
    member_row: &ProviderRuntimeProjection,
    adapter: &A,
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
            let mut prompt = String::from(
                "TEAM MESSAGES arrived. They are conversation, not Work ownership. \
                 Address the question or coordination request, and use the Works \
                 board for any durable responsibility.\n\n",
            );
            for message in &messages {
                prompt.push_str(&format!(
                    "--- {} ({}, correlation_id={}) ---\n{}\n\n",
                    message.sender_runtime_id,
                    team_message_kind_label(&message.kind),
                    message.correlation_id,
                    message.body
                ));
            }
            Ok(Ok(CycleInput {
                prompt,
                active_work: None,
                accepted_messages: messages,
                consumed_work_version: None,
            }))
        }
        IdleMemberWake::Closed => Ok(Err(MemberOutcome::new(
            member_row,
            MemberRunStatus::Stopped,
            format!("{} member runtime closed by Host", adapter.display_name()),
        ))),
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
        let effect = prepare_provider_effect(ledger, member_row, &source_record_id, &prompt)?;
        let mut pending_control_effect: Option<Box<PendingProviderControl>> = None;
        let mut control_prepare_error: Option<String> = None;

        let turn_result = {
            let _turn_lease = context.turn_leases.acquire();
            let _live_turn_guard = LiveProviderTurnGuard::new(
                context.live_sink.clone(),
                ledger.run_id.clone(),
                member_row.id.clone(),
            );
            let live_sink = context.live_sink.clone();
            adapter.run_cycle(
                &prompt,
                context.idle_timeout,
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
                            MemberControlCommand::Close { reason, .. } => {
                                let mut close = false;
                                let mut interrupt = false;
                                let dispatch = {
                                    let mut proxy = A::native_control(&mut close, &mut interrupt);
                                    provider_adapter::execute_team_control(
                                        ledger,
                                        member_row,
                                        &format!("{provider}-close:{}:{round}", member_row.id),
                                        &reason,
                                        true,
                                        &mut proxy,
                                    )
                                };
                                control.close = close;
                                control.interrupt = interrupt;
                                match dispatch {
                                    Ok(ProviderControlDispatch::Pending(pending)) => {
                                        pending_control_effect = Some(pending)
                                    }
                                    Ok(ProviderControlDispatch::Replayed) => {}
                                    Err(error) => {
                                        control_prepare_error = Some(error.to_string())
                                    }
                                }
                                return control;
                            }
                            MemberControlCommand::Interrupt { reason, .. } => {
                                let mut close = false;
                                let mut interrupt = false;
                                let dispatch = {
                                    let mut proxy = A::native_control(&mut close, &mut interrupt);
                                    provider_adapter::execute_team_control(
                                        ledger,
                                        member_row,
                                        &format!("{provider}-interrupt:{}:{round}", member_row.id),
                                        &reason,
                                        false,
                                        &mut proxy,
                                    )
                                };
                                control.close = close;
                                control.interrupt = interrupt;
                                match dispatch {
                                    Ok(ProviderControlDispatch::Pending(pending)) => {
                                        pending_control_effect = Some(pending)
                                    }
                                    Ok(ProviderControlDispatch::Replayed) => {}
                                    Err(error) => {
                                        control_prepare_error = Some(error.to_string())
                                    }
                                }
                                return control;
                            }
                            MemberControlCommand::Steer {
                                content, reply, ..
                            } => {
                                // Only an explicit Steer command may compile
                                // into current-cycle injection. Ordinary
                                // Messages never reach this channel.
                                if supports_inject {
                                    control.injects.push(content);
                                    let _ = reply.send(Ok(serde_json::json!({
                                        "member_run_id": member_row.id,
                                        "status": "steer_accepted",
                                        "provider_ack": "compiles_at_next_control_boundary",
                                    })));
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
        let turn = match turn_result {
            Ok(turn) => {
                settle_provider_effect(
                    ledger,
                    &effect,
                    true,
                    Some(serde_json::json!({
                        "provider": provider,
                        "round": round,
                        "session_file": adapter.native_session_locator(),
                    })),
                    None,
                )?;
                turn
            }
            Err(error) => {
                provider_adapter::settle_optional_team_control_without_terminal_ack(
                    ledger,
                    &mut pending_control_effect,
                )?;
                settle_provider_effect(ledger, &effect, false, None, Some(error.to_string()))?;
                return Err(error);
            }
        };
        if let Some(error) = control_prepare_error {
            provider_adapter::settle_optional_team_control_without_terminal_ack(
                ledger,
                &mut pending_control_effect,
            )?;
            return Err(CliError::Usage(error));
        }
        if let Some(pending) = pending_control_effect.take() {
            provider_adapter::settle_team_control(
                ledger,
                &pending,
                turn.interrupted.then_some("interrupted"),
            )?;
        }

        require_provider_session_authority(ledger, &member_row.agent_member_id, true)?;
        // Refresh the native session locator after the cycle.
        let expected = member_row.clone();
        member_row.native_session = Some(native_session_ref(
            member_row,
            adapter.native_session_locator(),
            adapter.native_locator_kind(),
        ));
        ledger.save_member_run(&expected, member_row)?;

        let receipt = format!(
            "{provider}:{}:round-{round}",
            adapter.native_session_locator()
        );
        if let Some(claimed) = cycle.active_work.as_ref() {
            ledger.complete_work_delivery(claimed, &receipt)?;
        }
        drop(cycle.active_work.take());
        for message in &cycle.accepted_messages {
            mark_message_delivered(ledger, message, &member_row.id, &member_row.name, &receipt)?;
        }
        cycle.accepted_messages.clear();

        if turn.interrupted {
            let expected = member_row.clone();
            let interruption_summary = if turn.close_requested_by_harness {
                format!("The Host explicitly closed the {display} member runtime.")
            } else {
                format!("The operator or Lead interrupted the active {display} turn.")
            };
            if turn.close_requested_by_harness {
                transition_provider_session_for_member(
                    ledger,
                    member_row,
                    AgentSessionStatus::Idle,
                )?;
                member_row.coordination_status = MemberCoordinationStatus::Closed;
            }
            member_row.status = if turn.close_requested_by_harness {
                MemberRunStatus::Stopped
            } else {
                MemberRunStatus::Idle
            };
            member_row.finished_at = turn.close_requested_by_harness.then(now_string);
            member_row.last_event_at = Some(now_string());
            ledger.save_member_run(&expected, member_row)?;
            ledger.append_action(
                &member_row.id,
                if turn.close_requested_by_harness {
                    "closed"
                } else {
                    "interrupted"
                },
                MemberActionStatus::Cancelled,
                if turn.close_requested_by_harness {
                    "member runtime closed"
                } else {
                    "provider turn interrupted"
                },
                &interruption_summary,
            )?;
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
            let is_zero_output = final_text.trim().is_empty() && turn.tool_call_count == 0;
            if is_zero_output {
                zero_output_streak += 1;
            } else {
                zero_output_streak = 0;
            }
            let action_status = if is_zero_output {
                MemberActionStatus::Failed
            } else {
                let result = parse_round_result(&final_text);
                if result == MemberRoundResult::Done {
                    MemberActionStatus::Succeeded
                } else {
                    MemberActionStatus::Failed
                }
            };
            let round_summary =
                provider_turn_coordination_summary(display, round, !final_text.trim().is_empty());
            let action = ledger.append_action(
                &member_row.id,
                "turn_completed",
                action_status,
                &format!("{display} provider round {round} completed"),
                &round_summary,
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

/// Executable capability report for a provider's Team runtime binding, when
/// one exists. `None` means the provider still runs a branded loop without
/// an executable binding report — itself an honest signal.
pub(crate) fn capability_bindings_for(provider: &str) -> Option<Vec<CapabilityBinding>> {
    match provider {
        "pi" => Some(crate::pi_rpc::PiTeamRuntime::capability_bindings()),
        _ => None,
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
    fn pi_tools_allowlist_compiles_every_ceiling() {
        assert_eq!(
            pi_tools_allowlist_for_ceiling(PermissionCeiling::ReadOnly),
            Some("read,grep,find,ls")
        );
        assert_eq!(
            pi_tools_allowlist_for_ceiling(PermissionCeiling::WorkspaceWrite),
            Some("read,grep,find,ls,write,edit")
        );
        assert_eq!(
            pi_tools_allowlist_for_ceiling(PermissionCeiling::FullAccess),
            None
        );
    }

    #[test]
    fn workspace_write_allowlist_never_includes_bash() {
        // A shell escapes the workspace boundary; the adapter cannot verify
        // it, so bash stays out of every restricted allowlist.
        for ceiling in [
            PermissionCeiling::ReadOnly,
            PermissionCeiling::WorkspaceWrite,
        ] {
            let allowlist = pi_tools_allowlist_for_ceiling(ceiling).unwrap();
            assert!(
                !allowlist.split(',').any(|tool| tool == "bash"),
                "{ceiling:?} allowlist must not contain bash: {allowlist}"
            );
        }
    }

    #[test]
    fn enforcement_locus_matches_compilation() {
        let restricted = pi_security_enforcement_locus(PermissionCeiling::WorkspaceWrite);
        assert_eq!(
            restricted.kind,
            SecurityEnforcementLocusKind::AdapterToolAllowlist
        );
        let full = pi_security_enforcement_locus(PermissionCeiling::FullAccess);
        assert_eq!(full.kind, SecurityEnforcementLocusKind::NoneVerified);
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
        // Permission enforcement claims a real locus.
        let permission = bindings
            .iter()
            .find(|binding| binding.capability == "permission_enforcement")
            .unwrap();
        assert!(permission.status.is_supported());
        assert!(permission.security_enforcement_locus.is_some());
    }

    #[test]
    fn providers_without_a_binding_report_none() {
        // DeepSeek needs no new core object to plug in later — until its
        // bridge exists, the honest report is "no executable binding yet".
        assert!(capability_bindings_for("deepseek").is_none());
        assert!(capability_bindings_for("codex").is_none());
    }
}
