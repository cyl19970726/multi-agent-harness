use harness_core::agentfirm_api::{
    AgentSession, RuntimeEffectCertainty, RuntimePostconditionStatus,
};
use harness_core::ProviderBindingAdmission;

use crate::{
    CapabilityBinding, EffectReceipt, ProviderNativeControl, ProviderTerminalFailure,
    RuntimeAdapter,
};

/// The three irreducible cycle time bounds (SPEC-TYPED-CYCLE-OUTCOME-01 §3.1).
///
/// One bare `Duration` previously carried all three meanings and each adapter
/// guessed one (#708). These are transport-layer bounds only: none of them is
/// a plan gate, a work-acceptance gate, or a provider-silence verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleTimeouts {
    /// Delivery boundary: from writing the input to holding the exact
    /// provider acceptance receipt for THIS input. It is valid only BEFORE
    /// the receipt exists — once the input is accepted, wall-clock or
    /// inter-frame silence alone must never turn the cycle into an `Err` or a
    /// provider terminal failure (invariant I1). A cycle whose receipt never
    /// arrives fails here, and that failure maps to "input never accepted"
    /// (replay-safe), because the provider never took the input.
    pub input_acceptance: std::time::Duration,
    /// Liveness-proof boundary: the longest interval allowed WITHOUT a
    /// positive proof that the process and transport are alive. It is NOT a
    /// provider-silence cap (frozen decision D2): an adapter that
    /// continuously proves liveness (e.g. an `ensure_alive` probe or a
    /// reader-thread disconnect branch) uses this only as a probe deadline,
    /// and its expiry alone is not a failure — only a failed or impossible
    /// probe fails, and that failure stays fail-closed as "accepted, outcome
    /// unproven" (invariant I2), never "not applied".
    pub transport_liveness: std::time::Duration,
    /// Control-settle boundary: after Interrupt/Close is sent, the longest
    /// wait for its exact settled confirmation. It bounds control only, never
    /// the cycle itself (A5); an expired settle maps to "unproven" (Unknown),
    /// never to success and never to a cycle failure (decision D3).
    pub control_settle: std::time::Duration,
}

impl CycleTimeouts {
    /// Contract defaults (frozen decision 6): a caller that exposes one
    /// timeout flag sets only `input_acceptance` and takes these for the
    /// other two bounds.
    pub const DEFAULT_INPUT_ACCEPTANCE: std::time::Duration = std::time::Duration::from_secs(300);
    pub const DEFAULT_TRANSPORT_LIVENESS: std::time::Duration = std::time::Duration::from_secs(30);
    pub const DEFAULT_CONTROL_SETTLE: std::time::Duration = std::time::Duration::from_secs(15);

    /// Bounds for a pure control path (Interrupt/Close), where only
    /// `control_settle` is operative and the other two are inert.
    pub fn control_path(control_settle: std::time::Duration) -> Self {
        Self {
            control_settle,
            ..Self::default()
        }
    }

    /// The single-flag shape: an explicit `input_acceptance`, contract
    /// defaults for the rest (frozen decision 6).
    pub fn with_input_acceptance(input_acceptance: std::time::Duration) -> Self {
        Self {
            input_acceptance,
            ..Self::default()
        }
    }
}

impl Default for CycleTimeouts {
    fn default() -> Self {
        Self {
            input_acceptance: Self::DEFAULT_INPUT_ACCEPTANCE,
            transport_liveness: Self::DEFAULT_TRANSPORT_LIVENESS,
            control_settle: Self::DEFAULT_CONTROL_SETTLE,
        }
    }
}

/// Who caused an interrupt (invariant I3: a Host control action and an
/// adapter's own policy must be distinguishable in the durable record).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum InterruptCause {
    /// The Harness/Host issued Interrupt through `CycleControl`. This is the
    /// only cause adapters produce on the ordinary path.
    HostControl,
    /// The adapter interrupted on its own provider-native policy. Kept as a
    /// reviewed escape hatch (frozen decision 2) but NOT produced by default:
    /// after the S2 migration no adapter's normal path may yield it (B4 is
    /// the reverse proof). `reason` must be non-empty (assertion B2).
    AdapterPolicy { reason: String },
    /// The PROVIDER ended the cycle as interrupted on its own — no Harness
    /// control request and no adapter policy (Owner decision after S2 review
    /// 01: the real second interrupt source §3.2's two variants cannot
    /// express). `reason` must be non-empty.
    ProviderInitiated { reason: String },
}

impl InterruptCause {
    /// The only construction path for an adapter-policy cause; rejects an
    /// empty or blank reason so B2 is falsifiable at the type boundary.
    pub fn adapter_policy(reason: impl Into<String>) -> Option<Self> {
        Self::with_reason(reason, |reason| Self::AdapterPolicy { reason })
    }

    /// The only construction path for a provider-initiated cause; rejects an
    /// empty or blank reason for the same falsifiability.
    pub fn provider_initiated(reason: impl Into<String>) -> Option<Self> {
        Self::with_reason(reason, |reason| Self::ProviderInitiated { reason })
    }

    fn with_reason(
        reason: impl Into<String>,
        variant: impl FnOnce(String) -> Self,
    ) -> Option<Self> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            None
        } else {
            Some(variant(reason))
        }
    }
}

/// Non-invasive provider observation. It is deliberately not a transcript or
/// a provider-event mirror.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CycleRuntimeObservation {
    pub transport_alive: bool,
    pub process_alive: bool,
    pub is_streaming: Option<bool>,
    pub pending_message_count: Option<u64>,
    pub steering_mode: Option<String>,
    pub follow_up_mode: Option<String>,
    pub settled_boundary_observed: bool,
}

impl CycleRuntimeObservation {
    pub fn terminal_cycle_observed(&self) -> bool {
        self.transport_alive
            && self.process_alive
            && self.is_streaming == Some(false)
            && self.settled_boundary_observed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ControlTransportReceipt {
    pub command: String,
    pub response_id: Option<String>,
    pub success: bool,
}

/// Provider-owned correlation returned with a terminal cycle. The application
/// combines it with the durable RuntimeCommand/session/attempt identity; the
/// provider adapter must not invent those application-owned fields.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NativeCycleCorrelation {
    pub provider_input_id: String,
    pub input_acceptance_receipt: ControlTransportReceipt,
    pub terminal_provider_input_id: Option<String>,
    pub exact_terminal_ref: Option<String>,
}

/// Whether the provider's terminal cycle state was observed. Source: the
/// existing `CycleRuntimeObservation::terminal_cycle_observed`, nothing new.
/// Placed in cycle.rs per the package-boundary edge list; the Spec text
/// names receipt_and_terminal.rs (Host amendment, recorded in DEV-156 S1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleTerminalStatus {
    Observed,
    NotObserved,
}

/// The interrupt axis of one settled cycle. `Option<InterruptCause>` alone
/// cannot express D5's "interrupt issued but never settled", so the
/// settlement carries that third state explicitly. Placed in cycle.rs per
/// the package-boundary edge list; the Spec text names
/// receipt_and_terminal.rs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleInterruptSettlement {
    /// No interrupt was issued during the cycle.
    None,
    /// An interrupt settled with an exact attributed cause.
    Settled(InterruptCause),
    /// An interrupt was issued but its exact settled confirmation never
    /// arrived (control_settle expired, decision D3).
    Unsettled,
}

/// The typed settlement facts of one finished cycle (#709, decision D5).
///
/// Scope (decision D6): this is control/effect settlement evidence only. It
/// does not claim semantic success, a Work result, or Host acceptance — the
/// strongest value the runtime layer can express is "the postcondition held",
/// and semantics stay in the Harness (invariant I6). Placed in cycle.rs per
/// the package-boundary edge list; the Spec text names
/// receipt_and_terminal.rs.
///
/// Fields are private so the ONLY public way to obtain a StartCycle-shaped
/// [`EffectReceipt`] is [`EffectReceipt::for_cycle`]. Assertion C4 is the
/// negative proof that no struct-literal path exists outside this crate:
///
/// ```compile_fail
/// use firm_runtime_contract::{
///     CycleInterruptSettlement, CycleSettlement, CycleTerminalStatus,
/// };
/// let settlement = CycleSettlement {
///     correlation: todo!(),
///     terminal: CycleTerminalStatus::Observed,
///     provider_terminal_failure: None,
///     interrupt: CycleInterruptSettlement::None,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleSettlement {
    correlation: crate::NativeCycleCorrelation,
    terminal: CycleTerminalStatus,
    provider_terminal_failure: Option<ProviderTerminalFailure>,
    interrupt: CycleInterruptSettlement,
}

impl CycleSettlement {
    pub fn new(
        correlation: crate::NativeCycleCorrelation,
        terminal: CycleTerminalStatus,
        provider_terminal_failure: Option<ProviderTerminalFailure>,
        interrupt: CycleInterruptSettlement,
    ) -> Self {
        Self {
            correlation,
            terminal,
            provider_terminal_failure,
            interrupt,
        }
    }

    pub fn correlation(&self) -> &crate::NativeCycleCorrelation {
        &self.correlation
    }

    pub fn terminal(&self) -> CycleTerminalStatus {
        self.terminal
    }

    pub fn provider_terminal_failure(&self) -> Option<&ProviderTerminalFailure> {
        self.provider_terminal_failure.as_ref()
    }

    pub fn interrupt(&self) -> &CycleInterruptSettlement {
        &self.interrupt
    }
}

impl EffectReceipt {
    /// The SOLE constructor for a StartCycle-shaped receipt (#709, frozen
    /// decision 3). The postcondition is derived from the settlement
    /// exhaustively over the three D5 axes — it can never be asserted
    /// independently:
    ///
    /// - `provider_terminal_failure = Some(_)` → `Unsatisfied` (I5);
    /// - terminal not observed, or interrupt unsettled → `Unknown`;
    /// - terminal observed, no failure, no interrupt → `Satisfied`;
    /// - terminal observed, no failure, interrupt SETTLED (Host or
    ///   AdapterPolicy) → `Satisfied` — D5's fourth cell, decided by the
    ///   Brain as a Spec errata: a settled interrupt IS an observed terminal
    ///   boundary, the cause travels on the receipt (I3 is attribution, not
    ///   postcondition), and `Unsatisfied` is reserved for provider terminal
    ///   failure.
    ///
    /// Scope (decision D6): the receipt records control/effect settlement
    /// only. It does not claim semantic success, a Work result, or Host
    /// acceptance, and it carries no semantic field (invariant I6).
    /// Placed in cycle.rs per the package-boundary edge list; the Spec text
    /// names receipt_and_terminal.rs.
    pub fn for_cycle(
        effect_id: impl Into<String>,
        admission: ProviderBindingAdmission,
        settlement: CycleSettlement,
    ) -> EffectReceipt {
        let effect_id = effect_id.into();
        let postcondition = match (
            settlement.provider_terminal_failure.is_some(),
            settlement.terminal,
            &settlement.interrupt,
        ) {
            // D5 row 1 (invariant I5): never Satisfied with a failure.
            (true, _, _) => RuntimePostconditionStatus::Unsatisfied,
            // D5 row 2: terminal not observed, or interrupt never settled.
            (false, CycleTerminalStatus::NotObserved, _) => RuntimePostconditionStatus::Unknown,
            (false, _, CycleInterruptSettlement::Unsettled) => RuntimePostconditionStatus::Unknown,
            // D5 row 3: terminal observed, no failure, no interrupt.
            (false, CycleTerminalStatus::Observed, CycleInterruptSettlement::None) => {
                RuntimePostconditionStatus::Satisfied
            }
            // D5 fourth cell (Brain errata): a settled interrupt on a clean
            // observed terminal is itself the terminal boundary.
            (false, CycleTerminalStatus::Observed, CycleInterruptSettlement::Settled(_)) => {
                RuntimePostconditionStatus::Satisfied
            }
        };
        let certainty = match settlement.terminal {
            CycleTerminalStatus::Observed => RuntimeEffectCertainty::Applied,
            CycleTerminalStatus::NotObserved => RuntimeEffectCertainty::Unknown,
        };
        let mut native_evidence = vec![format!(
            "provider_input_id={}",
            settlement.correlation.provider_input_id
        )];
        if let Some(terminal_ref) = settlement.correlation.exact_terminal_ref.as_deref() {
            native_evidence.push(format!("exact_terminal_ref={terminal_ref}"));
        }
        if let Some(failure) = settlement.provider_terminal_failure.as_ref() {
            native_evidence.push(format!(
                "provider_terminal_failure={}",
                failure.to_provider_status()
            ));
        }
        match &settlement.interrupt {
            CycleInterruptSettlement::None => {}
            CycleInterruptSettlement::Settled(InterruptCause::HostControl) => {
                native_evidence.push("interrupt=host_control".to_string());
            }
            CycleInterruptSettlement::Settled(InterruptCause::AdapterPolicy { reason }) => {
                native_evidence.push(format!("interrupt=adapter_policy:{reason}"));
            }
            CycleInterruptSettlement::Settled(InterruptCause::ProviderInitiated { reason }) => {
                native_evidence.push(format!("interrupt=provider_initiated:{reason}"));
            }
            CycleInterruptSettlement::Unsettled => {
                native_evidence.push("interrupt=unsettled".to_string());
            }
        }
        EffectReceipt {
            effect_id,
            certainty,
            postcondition,
            admission,
            native_evidence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QuiesceOutcome {
    pub drained: bool,
    pub observation: CycleRuntimeObservation,
    pub evidence: String,
}

#[derive(Debug, Clone)]
pub struct ExecutionCycleOutcome {
    pub final_text: String,
    pub provider_terminal_failure: Option<ProviderTerminalFailure>,
    /// The attributed interrupt cause when the cycle was interrupted
    /// (invariant I3). `Some(HostControl)` is the only cause adapters produce
    /// on the ordinary path; after the S2 migration no adapter's normal path
    /// may yield `AdapterPolicy` (B4 is the reverse proof).
    pub interrupt: Option<InterruptCause>,
    pub close_requested_by_harness: bool,
    pub tool_call_count: u32,
    pub native_correlation: NativeCycleCorrelation,
    pub control_receipts: Vec<ControlTransportReceipt>,
    pub terminal_observation: CycleRuntimeObservation,
}

#[derive(Debug)]
pub enum SteerProviderResult {
    Acknowledged(ControlTransportReceipt),
    Unknown(String),
    NotApplied(String),
}

#[derive(Debug)]
pub struct SteerRequest {
    pub token: u64,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct CycleControl {
    pub close: bool,
    pub interrupt: bool,
    pub injects: Vec<SteerRequest>,
    pub fatal_error: Option<String>,
}

/// Provider-neutral executable binding for one persistent Agent Team member.
/// The application supervisor owns queueing, authority, and durable effects;
/// implementations own only native runtime behavior.
pub trait TeamRuntimeAdapter: RuntimeAdapter {
    type Error;

    fn provider(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn capability_bindings() -> Vec<CapabilityBinding>
    where
        Self: Sized;
    fn ensure_alive(&mut self) -> Result<(), Self::Error>;
    fn native_session_locator(&self) -> &str;
    fn native_locator_kind(&self) -> &'static str;
    fn bind_authority_session(
        &mut self,
        session: AgentSession,
        profile: &harness_core::ProviderIntegrationProfile,
    ) -> Result<(), Self::Error>;
    #[allow(clippy::type_complexity)]
    fn run_cycle(
        &mut self,
        input: &str,
        timeouts: CycleTimeouts,
        on_input_accepted: &mut dyn FnMut(&ControlTransportReceipt) -> Result<(), Self::Error>,
        on_steer_result: &mut dyn FnMut(
            &SteerRequest,
            &SteerProviderResult,
        ) -> Result<(), Self::Error>,
        on_event: &mut dyn FnMut(&serde_json::Value),
        poll_control: &mut dyn FnMut() -> CycleControl,
    ) -> Result<ExecutionCycleOutcome, Self::Error>;
    fn native_control<'a>(
        close: &'a mut bool,
        interrupt: &'a mut bool,
    ) -> Box<dyn ProviderNativeControl + 'a>
    where
        Self: Sized;
    fn supports_inject_current_cycle(&self) -> bool {
        false
    }
    fn supports_native_boundary_queue(&self) -> bool {
        false
    }
}
