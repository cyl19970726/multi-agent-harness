//! Provider-neutral operational contract for persistent coding-agent runtimes.
//!
//! Durable identity, capability, command-fence, and continuation shapes live
//! in `firm-core`. This module intentionally adds only process-local runtime
//! operations, admission/fence validation, lifecycle receipts, and a
//! conformance harness. A provider binding must pass preflight before it may
//! call any native bridge.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use harness_core::agentfirm_api::{
    AgentSession, NativeContinuationActivation, NativeContinuationProjection,
    RuntimeCommandBinding, RuntimeEffectCertainty, RuntimePostconditionStatus,
};
use harness_core::{
    ProviderBindingAdmission, ProviderCapabilityBinding, ProviderCapabilityEvidenceKind,
    ProviderCapabilityStatus,
};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Closed semantic surface over canonical capability bindings
// ---------------------------------------------------------------------------

/// Provider-neutral operations exposed by a coding-agent runtime binding.
/// Provider protocol names are forbidden here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SemanticCapability {
    OpenOrResume,
    StartCycle,
    InjectCurrentCycle,
    QueueNativeBoundary,
    Interrupt,
    Observe,
    InspectEffect,
    Reconcile,
    InspectContinuation,
    InhibitContinuation,
    ResumeContinuation,
    Quiesce,
    Release,
}

impl SemanticCapability {
    pub(crate) const ALL: [Self; 13] = [
        Self::OpenOrResume,
        Self::StartCycle,
        Self::InjectCurrentCycle,
        Self::QueueNativeBoundary,
        Self::Interrupt,
        Self::Observe,
        Self::InspectEffect,
        Self::Reconcile,
        Self::InspectContinuation,
        Self::InhibitContinuation,
        Self::ResumeContinuation,
        Self::Quiesce,
        Self::Release,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::OpenOrResume => "open_or_resume",
            Self::StartCycle => "start_cycle",
            Self::InjectCurrentCycle => "inject_current_cycle",
            Self::QueueNativeBoundary => "queue_at_native_boundary",
            Self::Interrupt => "interrupt_current_cycle",
            Self::Observe => "observe",
            Self::InspectEffect => "inspect_effect",
            Self::Reconcile => "reconcile_effect",
            Self::InspectContinuation => "inspect_continuation",
            Self::InhibitContinuation => "inhibit_continuation",
            Self::ResumeContinuation => "resume_continuation",
            Self::Quiesce => "quiesce",
            Self::Release => "release",
        }
    }
}

impl std::fmt::Display for SemanticCapability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdmissionDecision {
    pub capability: SemanticCapability,
    pub admission: ProviderBindingAdmission,
    pub required_closure: Vec<String>,
    pub reasons: Vec<String>,
}

impl AdmissionDecision {
    fn allows_effect(&self) -> bool {
        matches!(
            self.admission,
            ProviderBindingAdmission::Active | ProviderBindingAdmission::Degraded
        )
    }
}

/// Process-local resolver over canonical [`ProviderCapabilityBinding`] rows.
/// It never creates a second capability profile.
pub(crate) struct CapabilityResolver<'a> {
    bindings: BTreeMap<&'a str, &'a ProviderCapabilityBinding>,
}

impl<'a> CapabilityResolver<'a> {
    pub(crate) fn new(
        bindings: &'a [ProviderCapabilityBinding],
    ) -> Result<Self, RuntimeContractError> {
        let mut indexed = BTreeMap::new();
        for binding in bindings {
            if indexed
                .insert(binding.capability.as_str(), binding)
                .is_some()
            {
                return Err(RuntimeContractError::InvalidCapabilityBindings(format!(
                    "duplicate capability binding: {}",
                    binding.capability
                )));
            }
        }
        Ok(Self { bindings: indexed })
    }

    /// Resolve the required transitive closure and optional dependencies.
    /// Required nodes must be verified and executable; missing/review-required
    /// nodes deny the operation before a provider effect. Optional gaps lower
    /// the result to degraded without granting any required capability.
    pub(crate) fn admit(
        &self,
        capability: SemanticCapability,
        optional: &[SemanticCapability],
    ) -> AdmissionDecision {
        let mut closure = BTreeSet::new();
        let mut stack = Vec::new();
        let mut reasons = Vec::new();
        let mut hard = HardAdmission::Verified;
        let mut degraded = false;
        self.walk_required(
            capability.as_str(),
            &mut closure,
            &mut stack,
            &mut hard,
            &mut degraded,
            &mut reasons,
        );

        if matches!(hard, HardAdmission::Verified) {
            for optional_capability in optional {
                let mut optional_closure = BTreeSet::new();
                let mut optional_stack = Vec::new();
                let mut optional_hard = HardAdmission::Verified;
                let mut optional_degraded = false;
                let mut optional_reasons = Vec::new();
                self.walk_required(
                    optional_capability.as_str(),
                    &mut optional_closure,
                    &mut optional_stack,
                    &mut optional_hard,
                    &mut optional_degraded,
                    &mut optional_reasons,
                );
                if !matches!(optional_hard, HardAdmission::Verified) || optional_degraded {
                    degraded = true;
                    reasons.push(format!(
                        "optional capability {} is not fully verified",
                        optional_capability
                    ));
                }
            }
        }

        let admission = match hard {
            HardAdmission::Verified if degraded => ProviderBindingAdmission::Degraded,
            HardAdmission::Verified => ProviderBindingAdmission::Active,
            HardAdmission::Pending => ProviderBindingAdmission::PendingDependency,
            HardAdmission::Failed => ProviderBindingAdmission::Failed,
        };
        AdmissionDecision {
            capability,
            admission,
            required_closure: closure.into_iter().map(str::to_string).collect(),
            reasons,
        }
    }

    pub(crate) fn require_effect(
        &self,
        capability: SemanticCapability,
        optional: &[SemanticCapability],
    ) -> Result<AdmissionDecision, RuntimeContractError> {
        let decision = self.admit(capability, optional);
        if decision.allows_effect() {
            Ok(decision)
        } else {
            Err(RuntimeContractError::CapabilityAdmissionDenied {
                capability,
                admission: decision.admission,
                reasons: decision.reasons,
            })
        }
    }

    fn walk_required(
        &self,
        capability: &'a str,
        closure: &mut BTreeSet<&'a str>,
        stack: &mut Vec<&'a str>,
        hard: &mut HardAdmission,
        degraded: &mut bool,
        reasons: &mut Vec<String>,
    ) {
        if let Some(cycle_start) = stack.iter().position(|item| *item == capability) {
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(capability);
            reasons.push(format!("required dependency cycle: {}", cycle.join(" -> ")));
            *hard = HardAdmission::Failed;
            return;
        }
        if closure.contains(capability) {
            return;
        }
        closure.insert(capability);

        let Some(binding) = self.bindings.get(capability).copied() else {
            reasons.push(format!("required capability {capability} is missing"));
            *hard = HardAdmission::Failed;
            return;
        };
        match binding.status {
            ProviderCapabilityStatus::Verified => {
                if binding.evidence.is_empty()
                    || binding
                        .evidence
                        .iter()
                        .all(|item| item.evidence_ref.trim().is_empty())
                {
                    reasons.push(format!(
                        "verified capability {capability} has no evidence reference"
                    ));
                    *hard = HardAdmission::Failed;
                    return;
                }
                if binding.admission == ProviderBindingAdmission::Active {
                    let has_deterministic = binding.evidence.iter().any(|evidence| {
                        evidence.kind == ProviderCapabilityEvidenceKind::DeterministicAcceptance
                    });
                    let has_live_canary = binding.evidence.iter().any(|evidence| {
                        evidence.kind == ProviderCapabilityEvidenceKind::LiveCanary
                    });
                    if !has_deterministic || !has_live_canary {
                        reasons.push(format!(
                            "active capability {capability} lacks deterministic acceptance or live canary evidence"
                        ));
                        *hard = HardAdmission::Failed;
                        return;
                    }
                }
            }
            ProviderCapabilityStatus::ReviewRequired => {
                reasons.push(format!("required capability {capability} awaits review"));
                if !matches!(*hard, HardAdmission::Failed) {
                    *hard = HardAdmission::Pending;
                }
                return;
            }
            ProviderCapabilityStatus::Degraded | ProviderCapabilityStatus::Unsupported => {
                reasons.push(format!(
                    "required capability {capability} is {:?}",
                    binding.status
                ));
                *hard = HardAdmission::Failed;
                return;
            }
        }
        match binding.admission {
            ProviderBindingAdmission::Active => {}
            ProviderBindingAdmission::Degraded => *degraded = true,
            ProviderBindingAdmission::PendingDependency => {
                reasons.push(format!(
                    "required capability {capability} is pending dependency"
                ));
                if !matches!(*hard, HardAdmission::Failed) {
                    *hard = HardAdmission::Pending;
                }
                return;
            }
            ProviderBindingAdmission::Failed => {
                reasons.push(format!("required capability {capability} failed admission"));
                *hard = HardAdmission::Failed;
                return;
            }
        }

        stack.push(capability);
        for dependency in &binding.required_dependencies {
            let Some(canonical_name) = self
                .bindings
                .get_key_value(dependency.as_str())
                .map(|(k, _)| *k)
            else {
                reasons.push(format!(
                    "required capability {} for {capability} is missing",
                    dependency
                ));
                *hard = HardAdmission::Failed;
                continue;
            };
            self.walk_required(canonical_name, closure, stack, hard, degraded, reasons);
        }
        stack.pop();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HardAdmission {
    Verified,
    Pending,
    Failed,
}

// ---------------------------------------------------------------------------
// Exact command fence over canonical durable types
// ---------------------------------------------------------------------------

/// Process-local view of the exact durable command target. The binding itself
/// remains the canonical [`RuntimeCommandBinding`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct RuntimeFence<'a> {
    pub binding: &'a RuntimeCommandBinding,
    pub target_node_daemon_id: &'a str,
    pub target_node_daemon_generation: u64,
}

impl<'a> RuntimeFence<'a> {
    pub(crate) fn validate_exact(
        &self,
        session: &AgentSession,
    ) -> Result<(), RuntimeContractError> {
        let mut fields = Vec::new();
        if self.binding.target_session_id.as_deref() != Some(session.id.as_str()) {
            fields.push("target_session_id".to_string());
        }
        if self.binding.target_runtime_generation != Some(session.runtime_generation) {
            fields.push("target_runtime_generation".to_string());
        }
        if self.binding.target_driver_generation != Some(session.control_state.driver_generation) {
            fields.push("target_driver_generation".to_string());
        }
        if self.binding.target_driver != session.control_state.driver_ref {
            fields.push("target_driver".to_string());
        }
        if self.target_node_daemon_id != session.node_daemon_id {
            fields.push("target_node_daemon_id".to_string());
        }
        if self.target_node_daemon_generation != session.node_daemon_generation {
            fields.push("target_node_daemon_generation".to_string());
        }
        if self.binding.composition_fingerprint.as_deref()
            != session.control_state.composition_fingerprint.as_deref()
        {
            fields.push("composition_fingerprint".to_string());
        }
        if self.binding.capability_fingerprint.as_deref()
            != session.control_state.capability_fingerprint.as_deref()
        {
            fields.push("capability_fingerprint".to_string());
        }
        if self.binding.permission_envelope_ref.as_deref()
            != Some(session.permission_envelope_ref.as_str())
        {
            fields.push("permission_envelope_ref".to_string());
        }
        if self.binding.native_session_ref.as_ref() != session.native_session_ref.as_ref() {
            fields.push("native_session_ref".to_string());
        }

        if fields.is_empty() {
            Ok(())
        } else {
            Err(RuntimeContractError::FenceMismatch { fields })
        }
    }
}

/// Reject a stale continuation definition or process-local activation before
/// compiling continuation control into a native operation.
fn validate_continuation_exact(
    expected: &NativeContinuationProjection,
    session: &AgentSession,
) -> Result<(), RuntimeContractError> {
    let current = &session.control_state.continuation;
    let mut fields = Vec::new();
    if expected.definition.continuation_ref != current.definition.continuation_ref {
        fields.push("continuation.definition.continuation_ref".to_string());
    }
    if expected.definition.revision != current.definition.revision {
        fields.push("continuation.definition.revision".to_string());
    }
    if expected.definition.phase != current.definition.phase {
        fields.push("continuation.definition.phase".to_string());
    }
    if expected.definition.budget != current.definition.budget {
        fields.push("continuation.definition.budget".to_string());
    }
    if expected.activation != current.activation {
        fields.push("continuation.activation".to_string());
    }
    if let NativeContinuationActivation::Armed {
        runtime_generation,
        driver_generation,
    } = &expected.activation
    {
        if *runtime_generation != session.runtime_generation {
            fields.push("continuation.activation.runtime_generation".to_string());
        }
        if *driver_generation != session.control_state.driver_generation {
            fields.push("continuation.activation.driver_generation".to_string());
        }
    }
    if fields.is_empty() {
        Ok(())
    } else {
        Err(RuntimeContractError::StaleContinuation { fields })
    }
}

// ---------------------------------------------------------------------------
// Provider-neutral operations and receipts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeDescription {
    pub binding_id: String,
    pub native_protocol: String,
    pub composition_fingerprint: String,
    pub capability_fingerprint: String,
    pub capability_bindings: Vec<ProviderCapabilityBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlIntent {
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
    fn capability(&self) -> SemanticCapability {
        match self {
            Self::StartCycle { .. } => SemanticCapability::StartCycle,
            Self::InjectCurrentCycle { .. } => SemanticCapability::InjectCurrentCycle,
            Self::QueueNativeBoundary { .. } => SemanticCapability::QueueNativeBoundary,
            Self::Interrupt => SemanticCapability::Interrupt,
            Self::InhibitContinuation { .. } => SemanticCapability::InhibitContinuation,
            Self::ResumeContinuation { .. } => SemanticCapability::ResumeContinuation,
        }
    }

    fn validate(&self, session: &AgentSession) -> Result<(), RuntimeContractError> {
        match self {
            Self::InhibitContinuation { expected } | Self::ResumeContinuation { expected } => {
                validate_continuation_exact(expected, session)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlRequest {
    pub effect_id: String,
    pub intent: ControlIntent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffectReceipt {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub admission: ProviderBindingAdmission,
    pub native_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RuntimeObservation {
    pub native_session_ref: Option<String>,
    pub active_effect_id: Option<String>,
    pub continuation: NativeContinuationProjection,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffectInspection {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub native_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReconcileReceipt {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub native_evidence: Vec<String>,
}

/// Complete postcondition required before composition swap, driver handoff,
/// or runtime release. Every field must be `Satisfied`; `Unknown` and
/// `Unsatisfied` are both failure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct QuiesceReceipt {
    pub admission_fenced: RuntimePostconditionStatus,
    pub continuation_inhibited: RuntimePostconditionStatus,
    pub active_cycle_terminal: RuntimePostconditionStatus,
    pub native_queue_empty: RuntimePostconditionStatus,
    pub writable_children_drained: RuntimePostconditionStatus,
    pub idle_observed: RuntimePostconditionStatus,
    pub flush_confirmed: RuntimePostconditionStatus,
    pub evidence: Vec<String>,
}

impl QuiesceReceipt {
    pub(crate) fn verify(&self) -> Result<(), RuntimeContractError> {
        let mut fields = Vec::new();
        macro_rules! satisfied {
            ($field:ident) => {
                if self.$field != RuntimePostconditionStatus::Satisfied {
                    fields.push(format!("{}={:?}", stringify!($field), self.$field));
                }
            };
        }
        satisfied!(admission_fenced);
        satisfied!(continuation_inhibited);
        satisfied!(active_cycle_terminal);
        satisfied!(native_queue_empty);
        satisfied!(writable_children_drained);
        satisfied!(idle_observed);
        satisfied!(flush_confirmed);
        if fields.is_empty() {
            Ok(())
        } else {
            Err(RuntimeContractError::QuiesceIncomplete { fields })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuiesceStep {
    FenceAdmission,
    InhibitContinuation,
    SettleActiveCycle,
    DrainNativeQueue,
    DrainWritableChildren,
    ObserveIdle,
    ConfirmFlush,
}

impl QuiesceStep {
    const ORDER: [Self; 7] = [
        Self::FenceAdmission,
        Self::InhibitContinuation,
        Self::SettleActiveCycle,
        Self::DrainNativeQueue,
        Self::DrainWritableChildren,
        Self::ObserveIdle,
        Self::ConfirmFlush,
    ];
}

pub(crate) struct QuiesceReceiptBuilder {
    next: usize,
    conditions: [RuntimePostconditionStatus; 7],
    evidence: Vec<String>,
}

impl QuiesceReceiptBuilder {
    pub(crate) fn new() -> Self {
        Self {
            next: 0,
            conditions: [RuntimePostconditionStatus::Unknown; 7],
            evidence: Vec::new(),
        }
    }

    pub(crate) fn record(
        &mut self,
        step: QuiesceStep,
        condition: RuntimePostconditionStatus,
        evidence: impl Into<String>,
    ) -> Result<(), RuntimeContractError> {
        let expected = QuiesceStep::ORDER.get(self.next).copied();
        if expected != Some(step) {
            return Err(RuntimeContractError::QuiesceOrder {
                expected,
                actual: step,
            });
        }
        self.conditions[self.next] = condition;
        self.evidence.push(evidence.into());
        self.next += 1;
        Ok(())
    }

    pub(crate) fn finish(self) -> QuiesceReceipt {
        QuiesceReceipt {
            admission_fenced: self.conditions[0],
            continuation_inhibited: self.conditions[1],
            active_cycle_terminal: self.conditions[2],
            native_queue_empty: self.conditions[3],
            writable_children_drained: self.conditions[4],
            idle_observed: self.conditions[5],
            flush_confirmed: self.conditions[6],
            evidence: self.evidence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ReleaseReceipt {
    pub native_runtime_released: RuntimePostconditionStatus,
    pub live_handle_disposed: RuntimePostconditionStatus,
    pub authority_detached: RuntimePostconditionStatus,
    pub flush_confirmed: RuntimePostconditionStatus,
    pub evidence: Vec<String>,
}

impl ReleaseReceipt {
    fn verify(&self) -> Result<(), RuntimeContractError> {
        if self.native_runtime_released == RuntimePostconditionStatus::Satisfied
            && self.live_handle_disposed == RuntimePostconditionStatus::Satisfied
            && self.authority_detached == RuntimePostconditionStatus::Satisfied
            && self.flush_confirmed == RuntimePostconditionStatus::Satisfied
        {
            Ok(())
        } else {
            Err(RuntimeContractError::ReleaseIncomplete)
        }
    }
}

/// Operational provider-neutral adapter. Durable authorization remains the
/// RuntimeCommand/AgentSession types passed through [`RuntimeFence`].
pub(crate) trait RuntimeAdapter {
    fn describe(&self) -> &RuntimeDescription;

    fn open_or_resume(
        &mut self,
        fence: RuntimeFence<'_>,
        native_session_ref: Option<&str>,
    ) -> Result<RuntimeObservation, RuntimeContractError>;

    fn execute_control(
        &mut self,
        fence: RuntimeFence<'_>,
        request: ControlRequest,
    ) -> Result<EffectReceipt, RuntimeContractError>;

    fn observe(
        &mut self,
        fence: RuntimeFence<'_>,
    ) -> Result<RuntimeObservation, RuntimeContractError>;

    fn inspect_effect(
        &mut self,
        fence: RuntimeFence<'_>,
        effect_id: &str,
    ) -> Result<EffectInspection, RuntimeContractError>;

    fn reconcile(
        &mut self,
        fence: RuntimeFence<'_>,
        inspection: &EffectInspection,
    ) -> Result<ReconcileReceipt, RuntimeContractError>;

    fn quiesce(&mut self, fence: RuntimeFence<'_>) -> Result<QuiesceReceipt, RuntimeContractError>;

    fn release(&mut self, fence: RuntimeFence<'_>) -> Result<ReleaseReceipt, RuntimeContractError>;
}

/// Shared fail-closed preflight. It returns only after both the exact command
/// fence and capability dependency closure have been verified.
pub(crate) fn preflight_effect(
    description: &RuntimeDescription,
    session: &AgentSession,
    fence: RuntimeFence<'_>,
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
pub(crate) struct CompositionLifecycle {
    composition_fingerprint: String,
    capability_fingerprint: String,
    verified_quiesce: bool,
}

impl CompositionLifecycle {
    pub(crate) fn new(composition: impl Into<String>, capability: impl Into<String>) -> Self {
        Self {
            composition_fingerprint: composition.into(),
            capability_fingerprint: capability.into(),
            verified_quiesce: false,
        }
    }

    pub(crate) fn mark_effect_started(&mut self) {
        self.verified_quiesce = false;
    }

    pub(crate) fn record_quiesce(
        &mut self,
        receipt: &QuiesceReceipt,
    ) -> Result<(), RuntimeContractError> {
        receipt.verify()?;
        self.verified_quiesce = true;
        Ok(())
    }

    fn require_quiesced(&self) -> Result<(), RuntimeContractError> {
        if self.verified_quiesce {
            Ok(())
        } else {
            Err(RuntimeContractError::CompositionSwapRequiresQuiesce)
        }
    }

    pub(crate) fn swap(
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
pub(crate) struct OneShotDisposer {
    disposer: Option<Box<dyn FnMut()>>,
    explicit_release_succeeded: bool,
}

impl OneShotDisposer {
    pub(crate) fn new(disposer: impl FnMut() + 'static) -> Self {
        Self {
            disposer: Some(Box::new(disposer)),
            explicit_release_succeeded: false,
        }
    }

    pub(crate) fn finish_release(
        &mut self,
        receipt: &ReleaseReceipt,
    ) -> Result<(), RuntimeContractError> {
        receipt.verify()?;
        let Some(mut disposer) = self.disposer.take() else {
            return Err(RuntimeContractError::AlreadyReleased);
        };
        disposer();
        self.explicit_release_succeeded = true;
        Ok(())
    }

    pub(crate) fn explicit_release_succeeded(&self) -> bool {
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
pub(crate) enum RuntimeContractError {
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
    #[error("runtime handle has already been explicitly released")]
    AlreadyReleased,
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use harness_core::agentfirm_api::{
        AgentSessionControlState, AgentSessionStatus, MemberExecutionDriver,
        NativeContinuationBudget, NativeContinuationDefinition, NativeContinuationPhase,
        PermissionCeiling, RuntimeActivity, RuntimeDriverRef, RuntimeResidency,
    };
    use harness_core::{ProviderCapabilityEvidence, ProviderCapabilityEvidenceKind};

    use super::*;

    fn verified_binding(capability: SemanticCapability) -> ProviderCapabilityBinding {
        ProviderCapabilityBinding {
            capability: capability.as_str().to_string(),
            status: ProviderCapabilityStatus::Verified,
            admission: ProviderBindingAdmission::Active,
            provider_version: Some("test-bridge-v1".to_string()),
            adapter_revision: Some("test-revision".to_string()),
            feature_fingerprint: Some(format!("feature-{capability}")),
            required_dependencies: Vec::new(),
            evidence: vec![
                ProviderCapabilityEvidence {
                    kind: ProviderCapabilityEvidenceKind::DeterministicAcceptance,
                    evidence_ref: format!("test://{capability}"),
                    observed_at: None,
                    note: None,
                },
                ProviderCapabilityEvidence {
                    kind: ProviderCapabilityEvidenceKind::LiveCanary,
                    evidence_ref: format!("canary://{capability}"),
                    observed_at: None,
                    note: None,
                },
            ],
        }
    }

    fn full_bindings() -> Vec<ProviderCapabilityBinding> {
        let mut bindings = SemanticCapability::ALL
            .into_iter()
            .map(verified_binding)
            .collect::<Vec<_>>();
        for binding in &mut bindings {
            binding.required_dependencies = match binding.capability.as_str() {
                "start_cycle" => vec!["open_or_resume".to_string(), "observe".to_string()],
                "inject_current_cycle" | "queue_at_native_boundary" | "interrupt_current_cycle" => {
                    vec!["observe".to_string()]
                }
                "inhibit_continuation" | "resume_continuation" => {
                    vec!["inspect_continuation".to_string()]
                }
                "quiesce" => vec![
                    "interrupt_current_cycle".to_string(),
                    "observe".to_string(),
                    "inspect_continuation".to_string(),
                    "inhibit_continuation".to_string(),
                ],
                "release" => vec!["quiesce".to_string()],
                _ => Vec::new(),
            };
        }
        bindings
    }

    fn continuation() -> NativeContinuationProjection {
        NativeContinuationProjection {
            definition: NativeContinuationDefinition {
                continuation_ref: Some("deepseek-goal:44".to_string()),
                revision: Some(7),
                phase: NativeContinuationPhase::Active,
                budget: Some(NativeContinuationBudget {
                    remaining_cycles: Some(12),
                    remaining_tokens: None,
                    deadline: Some("2026-08-15T01:00:00Z".to_string()),
                    provider_budget_ref: Some("native-budget:1".to_string()),
                }),
            },
            activation: NativeContinuationActivation::Armed {
                runtime_generation: 8,
                driver_generation: 12,
            },
            observed_at: Some("2026-08-15T00:00:00Z".to_string()),
        }
    }

    fn session() -> AgentSession {
        let driver_ref = RuntimeDriverRef::TeamSupervisor {
            team_run_id: "team-run-1".to_string(),
            team_supervisor_id: "supervisor-1".to_string(),
            team_supervisor_generation: 12,
        };
        AgentSession {
            id: "session-1".to_string(),
            agent_identity_id: "identity-1".to_string(),
            node_id: "node-1".to_string(),
            execution_space_id: "space-1".to_string(),
            node_daemon_id: "daemon-1".to_string(),
            node_daemon_generation: 3,
            provider_kind: "test-only-native-bridge".to_string(),
            provider_profile_ref: "profile-1".to_string(),
            permission_envelope_ref: "permission-1".to_string(),
            effective_permission_ceiling: PermissionCeiling::FullAccess,
            lifecycle: AgentSessionStatus::Idle,
            runtime_generation: 8,
            control_state: AgentSessionControlState {
                runtime_residency: RuntimeResidency::Attached,
                activity: RuntimeActivity::Idle,
                execution_driver: MemberExecutionDriver::HostDriven,
                driver_generation: 12,
                driver_ref,
                handoff_state: Default::default(),
                continuation: continuation(),
                composition_fingerprint: Some("composition-a".to_string()),
                capability_fingerprint: Some("capabilities-a".to_string()),
                last_reconciled_at: None,
            },
            native_session_ref: None,
            current_turn_id: None,
            queued_input_count: 0,
            version: 1,
            opened_at: "2026-08-15T00:00:00Z".to_string(),
            last_active_at: "2026-08-15T00:00:00Z".to_string(),
            closed_at: None,
        }
    }

    fn binding(session: &AgentSession) -> RuntimeCommandBinding {
        RuntimeCommandBinding {
            target_session_id: Some(session.id.clone()),
            target_runtime_generation: Some(session.runtime_generation),
            target_driver_generation: Some(session.control_state.driver_generation),
            target_driver: session.control_state.driver_ref.clone(),
            native_session_ref: session.native_session_ref.clone(),
            composition_fingerprint: session.control_state.composition_fingerprint.clone(),
            capability_fingerprint: session.control_state.capability_fingerprint.clone(),
            capability_profile_version: Some("test-profile-v1".to_string()),
            permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
        }
    }

    fn fence_view<'a>(binding: &'a RuntimeCommandBinding) -> RuntimeFence<'a> {
        RuntimeFence {
            binding,
            target_node_daemon_id: "daemon-1",
            target_node_daemon_generation: 3,
        }
    }

    fn description(bindings: Vec<ProviderCapabilityBinding>) -> RuntimeDescription {
        RuntimeDescription {
            binding_id: "test-only-composable-native-bridge".to_string(),
            native_protocol: "deepseek-shaped-test-shim".to_string(),
            composition_fingerprint: "composition-a".to_string(),
            capability_fingerprint: "capabilities-a".to_string(),
            capability_bindings: bindings,
        }
    }

    #[derive(Default)]
    struct SessionBridge {
        calls: usize,
    }
    #[derive(Default)]
    struct CycleBridge {
        calls: usize,
    }
    #[derive(Default)]
    struct ObservationBridge {
        calls: usize,
    }
    #[derive(Default)]
    struct ContinuationBridge {
        calls: usize,
    }

    /// A composable native-bridge shape used only for contract conformance.
    /// It is not a production provider registration.
    struct DeepSeekShapedAdapter {
        description: RuntimeDescription,
        session: AgentSession,
        session_bridge: SessionBridge,
        cycle_bridge: CycleBridge,
        observation_bridge: ObservationBridge,
        continuation_bridge: ContinuationBridge,
        lifecycle: CompositionLifecycle,
        quiesce_conditions: [RuntimePostconditionStatus; 7],
        quiesce_log: Vec<QuiesceStep>,
        disposer: OneShotDisposer,
        released: bool,
    }

    impl DeepSeekShapedAdapter {
        fn new(bindings: Vec<ProviderCapabilityBinding>, dispose_count: Rc<Cell<usize>>) -> Self {
            Self {
                description: description(bindings),
                session: session(),
                session_bridge: SessionBridge::default(),
                cycle_bridge: CycleBridge::default(),
                observation_bridge: ObservationBridge::default(),
                continuation_bridge: ContinuationBridge::default(),
                lifecycle: CompositionLifecycle::new("composition-a", "capabilities-a"),
                quiesce_conditions: [RuntimePostconditionStatus::Satisfied; 7],
                quiesce_log: Vec::new(),
                disposer: OneShotDisposer::new(move || dispose_count.set(dispose_count.get() + 1)),
                released: false,
            }
        }

        fn preflight(
            &self,
            fence: RuntimeFence<'_>,
            capability: SemanticCapability,
            optional: &[SemanticCapability],
        ) -> Result<AdmissionDecision, RuntimeContractError> {
            preflight_effect(
                &self.description,
                &self.session,
                fence,
                capability,
                optional,
            )
        }
    }

    impl RuntimeAdapter for DeepSeekShapedAdapter {
        fn describe(&self) -> &RuntimeDescription {
            &self.description
        }

        fn open_or_resume(
            &mut self,
            fence: RuntimeFence<'_>,
            native_session_ref: Option<&str>,
        ) -> Result<RuntimeObservation, RuntimeContractError> {
            self.preflight(fence, SemanticCapability::OpenOrResume, &[])?;
            self.session_bridge.calls += 1;
            self.lifecycle.mark_effect_started();
            Ok(RuntimeObservation {
                native_session_ref: Some(native_session_ref.unwrap_or("deepseek-session:1").into()),
                active_effect_id: None,
                continuation: self.session.control_state.continuation.clone(),
                observed_at: "2026-08-15T00:00:00Z".to_string(),
            })
        }

        fn execute_control(
            &mut self,
            fence: RuntimeFence<'_>,
            request: ControlRequest,
        ) -> Result<EffectReceipt, RuntimeContractError> {
            let admission = self.preflight(fence, request.intent.capability(), &[])?;
            request.intent.validate(&self.session)?;
            match request.intent {
                ControlIntent::InhibitContinuation { .. }
                | ControlIntent::ResumeContinuation { .. } => self.continuation_bridge.calls += 1,
                _ => self.cycle_bridge.calls += 1,
            }
            self.lifecycle.mark_effect_started();
            Ok(EffectReceipt {
                effect_id: request.effect_id,
                certainty: RuntimeEffectCertainty::Applied,
                postcondition: RuntimePostconditionStatus::Satisfied,
                admission: admission.admission,
                native_evidence: vec!["test native bridge acknowledgement".to_string()],
            })
        }

        fn observe(
            &mut self,
            fence: RuntimeFence<'_>,
        ) -> Result<RuntimeObservation, RuntimeContractError> {
            self.preflight(fence, SemanticCapability::Observe, &[])?;
            self.observation_bridge.calls += 1;
            Ok(RuntimeObservation {
                native_session_ref: Some("deepseek-session:1".to_string()),
                active_effect_id: None,
                continuation: self.session.control_state.continuation.clone(),
                observed_at: "2026-08-15T00:00:00Z".to_string(),
            })
        }

        fn inspect_effect(
            &mut self,
            fence: RuntimeFence<'_>,
            effect_id: &str,
        ) -> Result<EffectInspection, RuntimeContractError> {
            self.preflight(fence, SemanticCapability::InspectEffect, &[])?;
            self.observation_bridge.calls += 1;
            Ok(EffectInspection {
                effect_id: effect_id.to_string(),
                certainty: RuntimeEffectCertainty::Applied,
                postcondition: RuntimePostconditionStatus::Satisfied,
                native_evidence: vec!["native effect inspection".to_string()],
            })
        }

        fn reconcile(
            &mut self,
            fence: RuntimeFence<'_>,
            inspection: &EffectInspection,
        ) -> Result<ReconcileReceipt, RuntimeContractError> {
            self.preflight(fence, SemanticCapability::Reconcile, &[])?;
            self.observation_bridge.calls += 1;
            Ok(ReconcileReceipt {
                effect_id: inspection.effect_id.clone(),
                certainty: inspection.certainty,
                postcondition: inspection.postcondition,
                native_evidence: vec!["native effect reconciliation".to_string()],
            })
        }

        fn quiesce(
            &mut self,
            fence: RuntimeFence<'_>,
        ) -> Result<QuiesceReceipt, RuntimeContractError> {
            self.preflight(fence, SemanticCapability::Quiesce, &[])?;
            let mut builder = QuiesceReceiptBuilder::new();
            self.quiesce_log.clear();
            for (index, step) in QuiesceStep::ORDER.into_iter().enumerate() {
                self.quiesce_log.push(step);
                builder.record(step, self.quiesce_conditions[index], format!("{step:?}"))?;
            }
            let receipt = builder.finish();
            self.lifecycle.record_quiesce(&receipt)?;
            Ok(receipt)
        }

        fn release(
            &mut self,
            fence: RuntimeFence<'_>,
        ) -> Result<ReleaseReceipt, RuntimeContractError> {
            self.preflight(fence, SemanticCapability::Release, &[])?;
            self.lifecycle.require_quiesced()?;
            if self.released {
                return Err(RuntimeContractError::AlreadyReleased);
            }
            let receipt = ReleaseReceipt {
                native_runtime_released: RuntimePostconditionStatus::Satisfied,
                live_handle_disposed: RuntimePostconditionStatus::Satisfied,
                authority_detached: RuntimePostconditionStatus::Satisfied,
                flush_confirmed: RuntimePostconditionStatus::Satisfied,
                evidence: vec!["test native release acknowledgement".to_string()],
            };
            self.disposer.finish_release(&receipt)?;
            self.session_bridge.calls += 1;
            self.released = true;
            Ok(receipt)
        }
    }

    fn start_request() -> ControlRequest {
        ControlRequest {
            effect_id: "effect-1".to_string(),
            intent: ControlIntent::StartCycle {
                input: "implement the change".to_string(),
            },
        }
    }

    #[test]
    fn required_dependency_missing_or_pending_has_zero_native_effect() {
        for pending in [false, true] {
            let mut start = verified_binding(SemanticCapability::StartCycle);
            start.required_dependencies = vec![SemanticCapability::OpenOrResume.to_string()];
            let mut bindings = vec![start];
            if pending {
                let mut open = verified_binding(SemanticCapability::OpenOrResume);
                open.status = ProviderCapabilityStatus::ReviewRequired;
                open.admission = ProviderBindingAdmission::PendingDependency;
                bindings.push(open);
            }
            let dispose_count = Rc::new(Cell::new(0));
            let mut adapter = DeepSeekShapedAdapter::new(bindings, dispose_count);
            let durable_binding = binding(&adapter.session);

            let error = adapter
                .execute_control(fence_view(&durable_binding), start_request())
                .unwrap_err();
            let expected = if pending {
                ProviderBindingAdmission::PendingDependency
            } else {
                ProviderBindingAdmission::Failed
            };
            assert!(matches!(
                error,
                RuntimeContractError::CapabilityAdmissionDenied { admission, .. }
                    if admission == expected
            ));
            assert_eq!(adapter.cycle_bridge.calls, 0);
        }
    }

    #[test]
    fn required_dependency_cycle_fails_closed() {
        let mut start = verified_binding(SemanticCapability::StartCycle);
        start.required_dependencies = vec![SemanticCapability::Observe.to_string()];
        let mut observe = verified_binding(SemanticCapability::Observe);
        observe.required_dependencies = vec![SemanticCapability::StartCycle.to_string()];
        let bindings = vec![start, observe];
        let resolver = CapabilityResolver::new(&bindings).unwrap();
        let decision = resolver.admit(SemanticCapability::StartCycle, &[]);
        assert_eq!(decision.admission, ProviderBindingAdmission::Failed);
        assert!(decision
            .reasons
            .iter()
            .any(|reason| reason.contains("cycle")));
    }

    #[test]
    fn optional_missing_is_degraded_but_executable() {
        let bindings = vec![verified_binding(SemanticCapability::StartCycle)];
        let resolver = CapabilityResolver::new(&bindings).unwrap();
        let decision = resolver
            .require_effect(
                SemanticCapability::StartCycle,
                &[SemanticCapability::QueueNativeBoundary],
            )
            .unwrap();
        assert_eq!(decision.admission, ProviderBindingAdmission::Degraded);
    }

    #[test]
    fn durable_continuation_definition_does_not_follow_activation() {
        let mut projection = continuation();
        let definition_before = projection.definition.clone();
        projection.activation = NativeContinuationActivation::Disarmed;
        projection.observed_at = Some("2026-08-15T00:01:00Z".to_string());
        assert_eq!(projection.definition, definition_before);
    }

    #[test]
    fn stale_revision_driver_composition_and_capability_have_zero_effect() {
        enum Case {
            Revision,
            Driver,
            Composition,
            Capability,
        }
        for case in [
            Case::Revision,
            Case::Driver,
            Case::Composition,
            Case::Capability,
        ] {
            let dispose_count = Rc::new(Cell::new(0));
            let mut adapter = DeepSeekShapedAdapter::new(full_bindings(), dispose_count);
            let mut durable_binding = binding(&adapter.session);
            let mut expected = adapter.session.control_state.continuation.clone();
            match case {
                Case::Revision => expected.definition.revision = Some(6),
                Case::Driver => durable_binding.target_driver_generation = Some(11),
                Case::Composition => {
                    durable_binding.composition_fingerprint = Some("composition-old".to_string())
                }
                Case::Capability => {
                    durable_binding.capability_fingerprint = Some("capabilities-old".to_string())
                }
            }
            let request = ControlRequest {
                effect_id: "continuation-effect".to_string(),
                intent: ControlIntent::ResumeContinuation { expected },
            };
            assert!(adapter
                .execute_control(fence_view(&durable_binding), request)
                .is_err());
            assert_eq!(adapter.cycle_bridge.calls, 0);
            assert_eq!(adapter.continuation_bridge.calls, 0);
        }
    }

    #[test]
    fn composition_swap_requires_verified_quiesce() {
        let mut lifecycle = CompositionLifecycle::new("composition-a", "capabilities-a");
        assert_eq!(
            lifecycle.swap("composition-b", "capabilities-b"),
            Err(RuntimeContractError::CompositionSwapRequiresQuiesce)
        );
        let mut builder = QuiesceReceiptBuilder::new();
        for step in QuiesceStep::ORDER {
            builder
                .record(step, RuntimePostconditionStatus::Satisfied, "verified")
                .unwrap();
        }
        lifecycle.record_quiesce(&builder.finish()).unwrap();
        lifecycle.swap("composition-b", "capabilities-b").unwrap();
    }

    #[test]
    fn quiesce_order_is_enforced_and_incomplete_is_not_success() {
        let mut builder = QuiesceReceiptBuilder::new();
        assert!(matches!(
            builder.record(
                QuiesceStep::ObserveIdle,
                RuntimePostconditionStatus::Satisfied,
                "out of order"
            ),
            Err(RuntimeContractError::QuiesceOrder { .. })
        ));

        for incomplete in [
            RuntimePostconditionStatus::Unknown,
            RuntimePostconditionStatus::Unsatisfied,
        ] {
            let dispose_count = Rc::new(Cell::new(0));
            let mut adapter = DeepSeekShapedAdapter::new(full_bindings(), dispose_count);
            adapter.quiesce_conditions[5] = incomplete;
            let durable_binding = binding(&adapter.session);
            assert!(matches!(
                adapter.quiesce(fence_view(&durable_binding)),
                Err(RuntimeContractError::QuiesceIncomplete { .. })
            ));
            assert_eq!(adapter.quiesce_log, QuiesceStep::ORDER);
            assert_eq!(
                adapter.lifecycle.swap("composition-b", "capabilities-b"),
                Err(RuntimeContractError::CompositionSwapRequiresQuiesce)
            );
        }
    }

    #[test]
    fn disposer_is_exactly_once_and_drop_is_not_release_success() {
        let dispose_count = Rc::new(Cell::new(0));
        {
            let mut adapter =
                DeepSeekShapedAdapter::new(full_bindings(), Rc::clone(&dispose_count));
            let durable_binding = binding(&adapter.session);
            adapter.quiesce(fence_view(&durable_binding)).unwrap();
            adapter.release(fence_view(&durable_binding)).unwrap();
            assert!(adapter.disposer.explicit_release_succeeded());
            assert_eq!(dispose_count.get(), 1);
            assert_eq!(
                adapter.release(fence_view(&durable_binding)),
                Err(RuntimeContractError::AlreadyReleased)
            );
        }
        assert_eq!(dispose_count.get(), 1, "drop must not dispose twice");

        let drop_only_count = Rc::new(Cell::new(0));
        {
            let observed = Rc::clone(&drop_only_count);
            let disposer = OneShotDisposer::new(move || observed.set(observed.get() + 1));
            assert!(!disposer.explicit_release_succeeded());
        }
        assert_eq!(drop_only_count.get(), 1, "drop prevents a process leak");
    }

    #[test]
    fn composable_shim_exercises_the_complete_operational_contract() {
        let dispose_count = Rc::new(Cell::new(0));
        let mut adapter = DeepSeekShapedAdapter::new(full_bindings(), Rc::clone(&dispose_count));
        let durable_binding = binding(&adapter.session);

        assert_eq!(
            adapter.describe().native_protocol,
            "deepseek-shaped-test-shim"
        );
        adapter
            .open_or_resume(fence_view(&durable_binding), None)
            .unwrap();

        let controls = [
            ControlIntent::InjectCurrentCycle {
                input: "steer".to_string(),
            },
            ControlIntent::QueueNativeBoundary {
                input: "follow up".to_string(),
            },
            ControlIntent::Interrupt,
            ControlIntent::InhibitContinuation {
                expected: adapter.session.control_state.continuation.clone(),
            },
            ControlIntent::ResumeContinuation {
                expected: adapter.session.control_state.continuation.clone(),
            },
        ];
        for (index, intent) in controls.into_iter().enumerate() {
            adapter
                .execute_control(
                    fence_view(&durable_binding),
                    ControlRequest {
                        effect_id: format!("effect-{index}"),
                        intent,
                    },
                )
                .unwrap();
        }

        adapter.observe(fence_view(&durable_binding)).unwrap();
        let inspection = adapter
            .inspect_effect(fence_view(&durable_binding), "effect-0")
            .unwrap();
        adapter
            .reconcile(fence_view(&durable_binding), &inspection)
            .unwrap();
        adapter.quiesce(fence_view(&durable_binding)).unwrap();
        adapter.release(fence_view(&durable_binding)).unwrap();

        assert_eq!(adapter.session_bridge.calls, 2);
        assert_eq!(adapter.cycle_bridge.calls, 3);
        assert_eq!(adapter.continuation_bridge.calls, 2);
        assert_eq!(adapter.observation_bridge.calls, 3);
        assert_eq!(dispose_count.get(), 1);
    }
}
