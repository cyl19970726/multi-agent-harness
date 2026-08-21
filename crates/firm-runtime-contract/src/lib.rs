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

/// Per-capability execution status. Every claimed status is accompanied by
/// provider evidence in [`CapabilityBinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    Degraded,
    Experimental,
}

impl CapabilityStatus {
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CapabilityBinding {
    pub capability: &'static str,
    pub status: CapabilityStatus,
    pub evidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_enforcement_locus: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QuiesceOutcome {
    pub drained: bool,
    pub observation: CycleRuntimeObservation,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTerminalFailure {
    pub reason: String,
    pub http_status: Option<i64>,
}

impl ProviderTerminalFailure {
    const STATUS_PREFIX: &'static str = "provider_terminal";

    pub fn to_provider_status(&self) -> String {
        let status = self
            .http_status
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_string());
        format!("{}:{}:{status}", Self::STATUS_PREFIX, self.reason.trim())
    }

    pub fn parse(provider_status: &str) -> Option<Self> {
        let rest = provider_status.strip_prefix(Self::STATUS_PREFIX)?;
        let (reason, status) = rest.strip_prefix(':')?.rsplit_once(':')?;
        Some(Self {
            reason: reason.to_string(),
            http_status: status.parse().ok(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionCycleOutcome {
    pub final_text: String,
    pub provider_terminal_failure: Option<ProviderTerminalFailure>,
    pub interrupted: bool,
    pub close_requested_by_harness: bool,
    pub tool_call_count: u32,
    pub input_acceptance_receipt: ControlTransportReceipt,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveProviderActivityKind {
    Thinking,
    ResponseStreaming,
    ToolStarted,
    ToolCompleted,
    ToolFailed,
    InteractionWaiting,
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticCapability {
    OpenOrResume,
    StartCycle,
    InjectCurrentCycle,
    QueueNativeBoundary,
    Interrupt,
    /// Reversible Team-member runtime shutdown. This closes only the owned
    /// adapter/process handle and retains the provider-native session for
    /// Reopen. It is deliberately weaker than Quiesce + Release, which prove
    /// workspace/queue/flush postconditions for driver or composition change.
    CloseRuntime,
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
    pub const ALL: [Self; 14] = [
        Self::OpenOrResume,
        Self::StartCycle,
        Self::InjectCurrentCycle,
        Self::QueueNativeBoundary,
        Self::Interrupt,
        Self::CloseRuntime,
        Self::Observe,
        Self::InspectEffect,
        Self::Reconcile,
        Self::InspectContinuation,
        Self::InhibitContinuation,
        Self::ResumeContinuation,
        Self::Quiesce,
        Self::Release,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenOrResume => "open_or_resume",
            Self::StartCycle => "start_cycle",
            Self::InjectCurrentCycle => "inject_current_cycle",
            Self::QueueNativeBoundary => "queue_at_native_boundary",
            Self::Interrupt => "interrupt_current_cycle",
            Self::CloseRuntime => "close_runtime",
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
pub struct AdmissionDecision {
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
pub struct CapabilityResolver<'a> {
    bindings: BTreeMap<&'a str, &'a ProviderCapabilityBinding>,
}

impl<'a> CapabilityResolver<'a> {
    pub fn new(bindings: &'a [ProviderCapabilityBinding]) -> Result<Self, RuntimeContractError> {
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
    pub fn admit(
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

    pub fn require_effect(
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
pub struct RuntimeFence<'a> {
    pub binding: &'a RuntimeCommandBinding,
    pub target_node_daemon_id: &'a str,
    pub target_node_daemon_generation: u64,
}

impl<'a> RuntimeFence<'a> {
    pub fn validate_exact(&self, session: &AgentSession) -> Result<(), RuntimeContractError> {
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
pub struct ControlRequest {
    pub effect_id: String,
    pub intent: ControlIntent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectReceipt {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub admission: ProviderBindingAdmission,
    pub native_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeObservation {
    pub native_session_ref: Option<String>,
    pub active_effect_id: Option<String>,
    pub continuation: NativeContinuationProjection,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectInspection {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub native_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReceipt {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub native_evidence: Vec<String>,
}

/// Complete postcondition required before composition swap, driver handoff,
/// or runtime release. Every field must be `Satisfied`; `Unknown` and
/// `Unsatisfied` are both failure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QuiesceReceipt {
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
    pub fn verify(&self) -> Result<(), RuntimeContractError> {
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
pub enum QuiesceStep {
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

pub struct QuiesceReceiptBuilder {
    next: usize,
    conditions: [RuntimePostconditionStatus; 7],
    evidence: Vec<String>,
}

impl Default for QuiesceReceiptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl QuiesceReceiptBuilder {
    pub fn new() -> Self {
        Self {
            next: 0,
            conditions: [RuntimePostconditionStatus::Unknown; 7],
            evidence: Vec::new(),
        }
    }

    pub fn record(
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

    pub fn finish(self) -> QuiesceReceipt {
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
pub struct ReleaseReceipt {
    pub native_runtime_released: RuntimePostconditionStatus,
    pub live_handle_disposed: RuntimePostconditionStatus,
    pub authority_detached: RuntimePostconditionStatus,
    pub flush_confirmed: RuntimePostconditionStatus,
    pub evidence: Vec<String>,
}

/// Receipt for reversible Team-member Close. It does not claim a complete
/// execution-lane quiesce, writable-workspace drain, or native-store flush;
/// those stronger facts belong exclusively to [`QuiesceReceipt`] and
/// [`ReleaseReceipt`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MemberRuntimeCloseReceipt {
    pub control_acknowledged: RuntimePostconditionStatus,
    pub current_cycle_terminal: RuntimePostconditionStatus,
    pub managed_runtime_released: RuntimePostconditionStatus,
    pub live_handle_disposed: RuntimePostconditionStatus,
    pub native_session_retained: RuntimePostconditionStatus,
    pub evidence: Vec<String>,
}

impl MemberRuntimeCloseReceipt {
    pub fn verify(&self) -> Result<(), RuntimeContractError> {
        let mut fields = Vec::new();
        macro_rules! satisfied {
            ($field:ident) => {
                if self.$field != RuntimePostconditionStatus::Satisfied {
                    fields.push(format!("{}={:?}", stringify!($field), self.$field));
                }
            };
        }
        satisfied!(control_acknowledged);
        satisfied!(current_cycle_terminal);
        satisfied!(managed_runtime_released);
        satisfied!(live_handle_disposed);
        satisfied!(native_session_retained);
        if fields.is_empty() {
            Ok(())
        } else {
            Err(RuntimeContractError::MemberCloseIncomplete { fields })
        }
    }
}

impl ReleaseReceipt {
    pub fn verify(&self) -> Result<(), RuntimeContractError> {
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
pub trait RuntimeAdapter {
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

    fn close_runtime(
        &mut self,
        _fence: RuntimeFence<'_>,
    ) -> Result<MemberRuntimeCloseReceipt, RuntimeContractError> {
        Err(RuntimeContractError::MemberCloseIncomplete {
            fields: vec!["binding has no close_runtime implementation".to_string()],
        })
    }

    fn quiesce(&mut self, fence: RuntimeFence<'_>) -> Result<QuiesceReceipt, RuntimeContractError>;

    fn release(&mut self, fence: RuntimeFence<'_>) -> Result<ReleaseReceipt, RuntimeContractError>;
}

/// Shared fail-closed preflight. It returns only after both the exact command
/// fence and capability dependency closure have been verified.
pub fn preflight_effect(
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

    fn require_quiesced(&self) -> Result<(), RuntimeContractError> {
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

#[cfg(test)]
mod tests;
