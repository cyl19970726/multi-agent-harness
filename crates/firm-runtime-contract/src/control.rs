use harness_core::agentfirm_api::{AgentSession, NativeContinuationProjection};
use harness_core::ProviderCapabilityBinding;

use crate::{validate_continuation_exact, RuntimeContractError, SemanticCapability};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderControlAction {
    CancelProviderTurn,
    CloseSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeControlPrimitive {
    CodexTurnInterrupt,
    ClaudeAgentSdkInterrupt,
    DeepSeekHarnessCancel,
    KimiAcpCancel,
    PiRpcInterrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProviderControlPlan {
    pub provider: String,
    pub action: ProviderControlAction,
    pub primitive: NativeControlPrimitive,
    pub requires_terminal_ack: bool,
}

/// Executable provider-native control boundary. Durable RuntimeCommand
/// preparation and settlement remain the application supervisor's concern.
pub trait ProviderNativeControl {
    fn provider(&self) -> &'static str;
    fn dispatch(&mut self, plan: &ProviderControlPlan) -> Result<(), String>;
}

impl ProviderNativeControl for Box<dyn ProviderNativeControl + '_> {
    fn provider(&self) -> &'static str {
        (**self).provider()
    }

    fn dispatch(&mut self, plan: &ProviderControlPlan) -> Result<(), String> {
        (**self).dispatch(plan)
    }
}

// ---------------------------------------------------------------------------
// Closed semantic surface over canonical capability bindings
// ---------------------------------------------------------------------------

/// Provider-neutral operations exposed by a coding-agent runtime binding.
/// Provider protocol names are forbidden here.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDescription {
    pub binding_id: String,
    pub native_protocol: String,
    pub composition_fingerprint: String,
    pub capability_fingerprint: String,
    pub capability_bindings: Vec<ProviderCapabilityBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlIntent {
    StartCycle {
        input: String,
    },
    InjectCurrentCycle {
        input: String,
    },
    QueueNativeBoundary {
        input: String,
    },
    Interrupt,
    InhibitContinuation {
        expected: NativeContinuationProjection,
    },
    ResumeContinuation {
        expected: NativeContinuationProjection,
    },
}

impl ControlIntent {
    /// Canonical durable command for this control intent. Lifecycle and
    /// inspection commands are separate adapter operations, not control intents.
    pub fn command_kind(&self) -> harness_core::agentfirm_api::RuntimeCommandKind {
        use harness_core::agentfirm_api::RuntimeCommandKind;
        match self {
            Self::StartCycle { .. } => RuntimeCommandKind::StartCycle,
            Self::InjectCurrentCycle { .. } => RuntimeCommandKind::InjectCurrentCycle,
            Self::QueueNativeBoundary { .. } => RuntimeCommandKind::QueueAtNativeBoundary,
            Self::Interrupt => RuntimeCommandKind::InterruptCurrentCycle,
            Self::InhibitContinuation { .. } => RuntimeCommandKind::InhibitContinuation,
            Self::ResumeContinuation { .. } => RuntimeCommandKind::ResumeContinuation,
        }
    }

    pub fn capability(&self) -> SemanticCapability {
        match self {
            Self::StartCycle { .. } => SemanticCapability::StartCycle,
            Self::InjectCurrentCycle { .. } => SemanticCapability::InjectCurrentCycle,
            Self::QueueNativeBoundary { .. } => SemanticCapability::QueueNativeBoundary,
            Self::Interrupt => SemanticCapability::Interrupt,
            Self::InhibitContinuation { .. } => SemanticCapability::InhibitContinuation,
            Self::ResumeContinuation { .. } => SemanticCapability::ResumeContinuation,
        }
    }

    pub(crate) fn validate(&self, session: &AgentSession) -> Result<(), RuntimeContractError> {
        match self {
            Self::InhibitContinuation { expected } | Self::ResumeContinuation { expected } => {
                validate_continuation_exact(expected, session)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlRequest {
    pub effect_id: String,
    pub intent: ControlIntent,
    /// Caller-selected budgets; adapters never choose cycle policy.
    pub timeouts: crate::CycleTimeouts,
}
