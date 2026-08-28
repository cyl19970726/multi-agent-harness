use harness_core::agentfirm_api::AgentSession;

use crate::{CapabilityBinding, ProviderNativeControl, ProviderTerminalFailure, RuntimeAdapter};

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
    pub interrupted: bool,
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
        idle_timeout: std::time::Duration,
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
