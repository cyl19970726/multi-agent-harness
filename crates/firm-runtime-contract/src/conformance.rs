use harness_core::agentfirm_api::AgentSession;
use harness_core::ProviderBindingAdmission;
use thiserror::Error;

use crate::{
    AdmissionDecision, CapabilityResolver, ControlRequest, EffectInspection, EffectReceipt,
    MemberRuntimeCloseReceipt, QuiesceReceipt, QuiesceStep, ReconcileReceipt, ReleaseReceipt,
    RuntimeBindingFence, RuntimeDescription, RuntimeObservation, SemanticCapability,
};

/// Operational provider-neutral adapter. Durable authorization remains the
/// RuntimeCommand/AgentSession types passed through [`RuntimeBindingFence`].
pub trait RuntimeAdapter {
    fn describe(&self) -> &RuntimeDescription;

    fn open_or_resume(
        &mut self,
        fence: RuntimeBindingFence,
        native_session_ref: Option<&str>,
    ) -> Result<RuntimeObservation, RuntimeContractError>;

    fn execute_control(
        &mut self,
        fence: RuntimeBindingFence,
        request: ControlRequest,
    ) -> Result<EffectReceipt, RuntimeContractError>;

    fn observe(
        &mut self,
        fence: RuntimeBindingFence,
    ) -> Result<RuntimeObservation, RuntimeContractError>;

    fn inspect_effect(
        &mut self,
        fence: RuntimeBindingFence,
        effect_id: &str,
    ) -> Result<EffectInspection, RuntimeContractError>;

    fn reconcile(
        &mut self,
        fence: RuntimeBindingFence,
        inspection: &EffectInspection,
    ) -> Result<ReconcileReceipt, RuntimeContractError>;

    fn close_runtime(
        &mut self,
        _fence: RuntimeBindingFence,
    ) -> Result<MemberRuntimeCloseReceipt, RuntimeContractError> {
        Err(RuntimeContractError::MemberCloseIncomplete {
            fields: vec!["binding has no close_runtime implementation".to_string()],
        })
    }

    fn quiesce(
        &mut self,
        fence: RuntimeBindingFence,
    ) -> Result<QuiesceReceipt, RuntimeContractError>;

    fn release(
        &mut self,
        fence: RuntimeBindingFence,
    ) -> Result<ReleaseReceipt, RuntimeContractError>;
}

/// Shared fail-closed preflight. It returns only after both the exact command
/// fence and capability dependency closure have been verified.
pub fn preflight_effect(
    description: &RuntimeDescription,
    session: &AgentSession,
    fence: RuntimeBindingFence,
    capability: SemanticCapability,
    optional: &[SemanticCapability],
) -> Result<AdmissionDecision, RuntimeContractError> {
    fence.validate_exact(session)?;
    if session.control_state.composition_fingerprint.as_deref()
        != Some(description.composition_fingerprint.as_str())
    {
        return Err(RuntimeContractError::FenceMismatch {
            fields: vec!["description.composition_fingerprint".to_string()],
        });
    }
    if session.control_state.capability_fingerprint.as_deref()
        != Some(description.capability_fingerprint.as_str())
    {
        return Err(RuntimeContractError::FenceMismatch {
            fields: vec!["description.capability_fingerprint".to_string()],
        });
    }
    CapabilityResolver::new(&description.capability_bindings)?.require_effect(capability, optional)
}

#[derive(Debug, Clone)]
pub struct CompositionLifecycle {
    composition_fingerprint: String,
    capability_fingerprint: String,
    verified_quiesce: bool,
}

impl CompositionLifecycle {
    pub fn new(composition: impl Into<String>, capability: impl Into<String>) -> Self {
        Self {
            composition_fingerprint: composition.into(),
            capability_fingerprint: capability.into(),
            verified_quiesce: false,
        }
    }

    pub fn mark_effect_started(&mut self) {
        self.verified_quiesce = false;
    }

    pub fn record_quiesce(&mut self, receipt: &QuiesceReceipt) -> Result<(), RuntimeContractError> {
        receipt.verify()?;
        self.verified_quiesce = true;
        Ok(())
    }

    pub(crate) fn require_quiesced(&self) -> Result<(), RuntimeContractError> {
        if self.verified_quiesce {
            Ok(())
        } else {
            Err(RuntimeContractError::CompositionSwapRequiresQuiesce)
        }
    }

    pub fn swap(
        &mut self,
        composition: impl Into<String>,
        capability: impl Into<String>,
    ) -> Result<(), RuntimeContractError> {
        self.require_quiesced()?;
        self.composition_fingerprint = composition.into();
        self.capability_fingerprint = capability.into();
        self.verified_quiesce = false;
        Ok(())
    }
}

/// Physically disposes a process-local handle at most once. `Drop` prevents a
/// leak but deliberately does not count as successful explicit release.
pub struct OneShotDisposer {
    disposer: Option<Box<dyn FnMut()>>,
    explicit_release_succeeded: bool,
}

impl OneShotDisposer {
    pub fn new(disposer: impl FnMut() + 'static) -> Self {
        Self {
            disposer: Some(Box::new(disposer)),
            explicit_release_succeeded: false,
        }
    }

    pub fn finish_release(&mut self, receipt: &ReleaseReceipt) -> Result<(), RuntimeContractError> {
        receipt.verify()?;
        let Some(mut disposer) = self.disposer.take() else {
            return Err(RuntimeContractError::AlreadyReleased);
        };
        disposer();
        self.explicit_release_succeeded = true;
        Ok(())
    }

    pub fn explicit_release_succeeded(&self) -> bool {
        self.explicit_release_succeeded
    }
}

impl Drop for OneShotDisposer {
    fn drop(&mut self) {
        if let Some(mut disposer) = self.disposer.take() {
            disposer();
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RuntimeContractError {
    #[error("invalid canonical capability bindings: {0}")]
    InvalidCapabilityBindings(String),
    #[error("capability {capability} admission is {admission:?}: {reasons:?}")]
    CapabilityAdmissionDenied {
        capability: SemanticCapability,
        admission: ProviderBindingAdmission,
        reasons: Vec<String>,
    },
    #[error("runtime fence mismatch: {fields:?}")]
    FenceMismatch { fields: Vec<String> },
    #[error("stale continuation projection: {fields:?}")]
    StaleContinuation { fields: Vec<String> },
    #[error("quiesce checks are incomplete: {fields:?}")]
    QuiesceIncomplete { fields: Vec<String> },
    #[error("unsafe quiesce order: expected {expected:?}, got {actual:?}")]
    QuiesceOrder {
        expected: Option<QuiesceStep>,
        actual: QuiesceStep,
    },
    #[error("composition swap or release requires a verified quiesce")]
    CompositionSwapRequiresQuiesce,
    #[error("release receipt is incomplete")]
    ReleaseIncomplete,
    #[error("member runtime close receipt is incomplete: {fields:?}")]
    MemberCloseIncomplete { fields: Vec<String> },
    #[error("runtime handle has already been explicitly released")]
    AlreadyReleased,
}
