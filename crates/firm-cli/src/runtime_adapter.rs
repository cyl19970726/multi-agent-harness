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
    require_provider_session_authority, settle_provider_effect, settle_provider_effect_not_applied,
    team_message_kind_label, transition_provider_session_for_member, wait_for_idle_member_wake,
    work_contract_prompt, ClaimedWork, CliError, CliResult, ControlReceiver, IdleMemberWake,
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

    fn runtime_released(&self) -> bool {
        !self.transport_alive && !self.process_alive && self.is_streaming == Some(false)
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
    reply: SyncSender<CliResult<Value>>,
}

fn fail_pending_control_replies(replies: &mut Vec<PendingControlReply>, detail: impl Into<String>) {
    let detail = detail.into();
    for pending in replies.drain(..) {
        let _ = pending.reply.send(Err(CliError::Usage(detail.clone())));
    }
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
                    crate::transition_provider_session_runtime_control(
                        ledger,
                        member_row,
                        harness_core::agentfirm_api::RuntimeResidency::Attached,
                        harness_core::agentfirm_api::RuntimeActivity::Running,
                    )?;
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
                        SteerProviderResult::Acknowledged(receipt) => settle_provider_effect(
                            ledger,
                            &pending.admission,
                            true,
                            Some(serde_json::json!({
                                "phase": "provider_input_accepted",
                                "provider_receipt": receipt,
                            })),
                            None,
                        ),
                        SteerProviderResult::Unknown(detail) => settle_provider_effect(
                            ledger,
                            &pending.admission,
                            false,
                            None,
                            Some(detail.clone()),
                        ),
                        SteerProviderResult::NotApplied(detail) => {
                            settle_provider_effect_not_applied(
                                ledger,
                                &pending.admission,
                                detail.clone(),
                            )
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
                            MemberControlCommand::Close { reason, reply, .. } => {
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
                                        pending_control_effects.push(pending);
                                        pending_control_replies.push(PendingControlReply {
                                            action: provider_adapter::ProviderControlAction::CloseSession,
                                            reply,
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
                            MemberControlCommand::Interrupt { reason, reply, .. } => {
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
                                        pending_control_effects.push(pending);
                                        pending_control_replies.push(PendingControlReply {
                                            action: provider_adapter::ProviderControlAction::CancelProviderTurn,
                                            reply,
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
                                                "provider_ack": "pi_rpc_response_success",
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
                    settle_provider_effect(ledger, &effect, false, None, Some(error.to_string()))?;
                }
                return Err(error);
            }
        };
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

        // Close is an ordered composition of three independently authorized
        // effects.  The interrupt command must first become terminal before
        // the Store can admit quiesce; quiesce must then become terminal
        // before release.  Keep the API replies pending until the complete
        // lifecycle postcondition is durable below.
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

        let mut terminal_observation = turn.terminal_observation.clone();
        let mut close_quiesce: Option<crate::runtime_adapter_contract::QuiesceReceipt> = None;
        let mut close_release: Option<crate::runtime_adapter_contract::ReleaseReceipt> = None;
        let mut terminal_failure = None;
        if turn.close_requested_by_harness {
            let profile = member_row.provider_profile.as_ref().ok_or_else(|| {
                CliError::Usage(format!(
                    "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
                    member_row.id
                ))
            })?;

            // Close is a composed control, not one overloaded interrupt. The
            // interrupt above has its own RuntimeCommand; quiesce and release
            // each receive a separate exact durable authorization before the
            // adapter may cross the provider boundary.
            let quiesce_source = format!("{source_record_id}:close:quiesce");
            match crate::prepare_provider_effect_kind(
                ledger,
                member_row,
                &quiesce_source,
                "quiesce execution lane after terminal cycle acknowledgement",
                harness_core::agentfirm_api::RuntimeCommandKind::QuiesceExecutionLane,
                "execution_lane.quiesce",
            ) {
                Ok(admission) => {
                    let bind_result = adapter
                        .bind_authority_session(admission.target_session.clone(), profile)
                        .and_then(|_| {
                            let binding = canonical_runtime_binding(&admission.target_session);
                            crate::runtime_adapter_contract::preflight_effect(
                                crate::runtime_adapter_contract::RuntimeAdapter::describe(adapter),
                                &admission.target_session,
                                crate::runtime_adapter_contract::RuntimeFence {
                                    binding: &binding,
                                    target_node_daemon_id: &admission.target_session.node_daemon_id,
                                    target_node_daemon_generation: admission
                                        .target_session
                                        .node_daemon_generation,
                                },
                                crate::runtime_adapter_contract::SemanticCapability::Quiesce,
                                &[],
                            )
                            .map(|_| ())
                            .map_err(|error| CliError::Usage(error.to_string()))
                        });
                    if let Err(error) = bind_result {
                        settle_provider_effect_not_applied(ledger, &admission, error.to_string())?;
                        terminal_failure = Some(format!(
                            "RUNTIME_COMMAND_NOT_APPLIED: {provider} quiesce preflight failed: {error}"
                        ));
                    } else {
                        let binding = canonical_runtime_binding(&admission.target_session);
                        match crate::runtime_adapter_contract::RuntimeAdapter::quiesce(
                            adapter,
                            crate::runtime_adapter_contract::RuntimeFence {
                                binding: &binding,
                                target_node_daemon_id: &admission.target_session.node_daemon_id,
                                target_node_daemon_generation: admission
                                    .target_session
                                    .node_daemon_generation,
                            },
                        ) {
                            Ok(receipt) => {
                                settle_provider_effect(
                                    ledger,
                                    &admission,
                                    true,
                                    Some(serde_json::json!({
                                        "phase": "execution_lane_quiesced",
                                        "receipt": &receipt,
                                    })),
                                    None,
                                )?;
                                close_quiesce = Some(receipt);
                            }
                            Err(error) => {
                                settle_provider_effect(
                                    ledger,
                                    &admission,
                                    false,
                                    None,
                                    Some(error.to_string()),
                                )?;
                                terminal_failure = Some(format!(
                                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: {provider} quiesce failed after provider-boundary admission: {error}"
                                ));
                            }
                        }
                    }
                }
                Err(error) => {
                    terminal_failure = Some(format!(
                        "RUNTIME_COMMAND_NOT_APPLIED: {provider} quiesce admission failed: {error}"
                    ));
                }
            }

            if terminal_failure.is_none() {
                let release_source = format!("{source_record_id}:close:release");
                match crate::prepare_provider_effect_kind(
                    ledger,
                    member_row,
                    &release_source,
                    "release owned runtime after verified quiesce",
                    harness_core::agentfirm_api::RuntimeCommandKind::ReleaseRuntime,
                    "runtime.release",
                ) {
                    Ok(admission) => {
                        let bind_result = adapter
                            .bind_authority_session(admission.target_session.clone(), profile)
                            .and_then(|_| {
                                let binding = canonical_runtime_binding(&admission.target_session);
                                crate::runtime_adapter_contract::preflight_effect(
                                    crate::runtime_adapter_contract::RuntimeAdapter::describe(
                                        adapter,
                                    ),
                                    &admission.target_session,
                                    crate::runtime_adapter_contract::RuntimeFence {
                                        binding: &binding,
                                        target_node_daemon_id: &admission
                                            .target_session
                                            .node_daemon_id,
                                        target_node_daemon_generation: admission
                                            .target_session
                                            .node_daemon_generation,
                                    },
                                    crate::runtime_adapter_contract::SemanticCapability::Release,
                                    &[],
                                )
                                .map(|_| ())
                                .map_err(|error| CliError::Usage(error.to_string()))
                            });
                        if let Err(error) = bind_result {
                            settle_provider_effect_not_applied(
                                ledger,
                                &admission,
                                error.to_string(),
                            )?;
                            terminal_failure = Some(format!(
                                "RUNTIME_COMMAND_NOT_APPLIED: {provider} release preflight failed: {error}"
                            ));
                        } else {
                            let binding = canonical_runtime_binding(&admission.target_session);
                            match crate::runtime_adapter_contract::RuntimeAdapter::release(
                                adapter,
                                crate::runtime_adapter_contract::RuntimeFence {
                                    binding: &binding,
                                    target_node_daemon_id: &admission.target_session.node_daemon_id,
                                    target_node_daemon_generation: admission
                                        .target_session
                                        .node_daemon_generation,
                                },
                            ) {
                                Ok(receipt) => {
                                    settle_provider_effect(
                                        ledger,
                                        &admission,
                                        true,
                                        Some(serde_json::json!({
                                            "phase": "runtime_released",
                                            "receipt": &receipt,
                                        })),
                                        None,
                                    )?;
                                    terminal_observation = RuntimeObservation {
                                        transport_alive: false,
                                        process_alive: false,
                                        is_streaming: Some(false),
                                        pending_message_count: Some(0),
                                        steering_mode: turn
                                            .terminal_observation
                                            .steering_mode
                                            .clone(),
                                        follow_up_mode: turn
                                            .terminal_observation
                                            .follow_up_mode
                                            .clone(),
                                        settled_boundary_observed: true,
                                    };
                                    close_release = Some(receipt);
                                }
                                Err(error) => {
                                    settle_provider_effect(
                                        ledger,
                                        &admission,
                                        false,
                                        None,
                                        Some(error.to_string()),
                                    )?;
                                    terminal_failure = Some(format!(
                                        "RUNTIME_COMMAND_RECOVERY_REQUIRED: {provider} release failed after provider-boundary admission: {error}"
                                    ));
                                }
                            }
                        }
                    }
                    Err(error) => {
                        terminal_failure = Some(format!(
                            "RUNTIME_COMMAND_NOT_APPLIED: {provider} release admission failed: {error}"
                        ));
                    }
                }
            }
        }

        let close_terminal_observed = !turn.close_requested_by_harness
            || (close_quiesce.is_some()
                && close_release.is_some()
                && terminal_observation.runtime_released());
        let terminal_ack = (turn.interrupted
            && abort_receipt_observed
            && cycle_terminal_observed
            && close_terminal_observed
            && terminal_failure.is_none())
        .then(|| {
            serde_json::json!({
                "provider_terminal_event": "agent_settled",
                "control_transport_receipts": &turn.control_receipts,
                "post_abort_observation": &turn.terminal_observation,
                "quiesce": &close_quiesce,
                "release": &close_release,
                "post_release_observation": &terminal_observation,
            })
            .to_string()
        });
        if let Some(detail) = terminal_failure {
            fail_pending_control_replies(&mut pending_control_replies, detail.clone());
            return Err(CliError::Usage(detail));
        }
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
        // Refresh the native session locator after the cycle.
        let mut close_authority_anchor = None;
        if turn.close_requested_by_harness {
            // HTTP Close durably latches coordination=Closed before the live
            // provider loop receives its control. Rebase only that expected
            // same-runtime drift, then fold native-session refresh and the
            // terminal Stopped state into the single CAS below. Saving here
            // against the stale Active row would deterministically conflict
            // with the already-committed Close latch.
            let latest = ledger.latest_member_run(&member_row.id)?.ok_or_else(|| {
                CliError::Usage(format!(
                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: {} disappeared while Close was settling",
                    member_row.id
                ))
            })?;
            if !latest.coordination_is_closed()
                || !crate::member_runtime_anchor_matches(member_row, &latest)
                || latest.native_session != member_row.native_session
            {
                return Err(CliError::Usage(format!(
                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: {} changed outside the exact runtime generation admitted by Close",
                    member_row.id
                )));
            }
            close_authority_anchor = Some(latest.clone());
            *member_row = latest;
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
            let refreshed_native_session = member_row.native_session.clone();
            let terminal_timestamp = now_string();
            let mut attempt = 0usize;
            loop {
                let expected = member_row.clone();
                member_row.native_session = refreshed_native_session.clone();
                member_row.status = if turn.close_requested_by_harness {
                    MemberRunStatus::Stopped
                } else {
                    MemberRunStatus::Idle
                };
                member_row.finished_at = turn
                    .close_requested_by_harness
                    .then(|| terminal_timestamp.clone());
                member_row.last_event_at = Some(terminal_timestamp.clone());
                match ledger.save_member_run(&expected, member_row) {
                    Ok(()) => break,
                    Err(CliError::Store(harness_store::StoreError::Conflict(_)))
                        if turn.close_requested_by_harness
                            && attempt + 1 < crate::PROVIDER_MEMBER_CAS_RETRIES =>
                    {
                        attempt += 1;
                        let latest = ledger.latest_member_run(&member_row.id)?.ok_or_else(|| {
                            CliError::Usage(format!(
                                "RUNTIME_COMMAND_RECOVERY_REQUIRED: {} disappeared while Close was finalizing",
                                member_row.id
                            ))
                        })?;
                        let anchor = close_authority_anchor.as_ref().expect("Close anchor");
                        if !latest.coordination_is_closed()
                            || !crate::member_runtime_progress_matches(
                                anchor, anchor, &latest, false,
                            )
                        {
                            return Err(CliError::Usage(format!(
                                "RUNTIME_COMMAND_RECOVERY_REQUIRED: {} changed outside the exact runtime generation admitted by Close",
                                member_row.id
                            )));
                        }
                        *member_row = latest;
                    }
                    Err(error) => return Err(error),
                }
            }
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
                let _ = pending.reply.send(Ok(serde_json::json!({
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
    fn providers_without_a_binding_report_none() {
        // DeepSeek needs no new core object to plug in later — until its
        // bridge exists, the honest report is "no executable binding yet".
        assert!(capability_bindings_for("deepseek").is_none());
        assert!(capability_bindings_for("codex").is_none());
    }
}
