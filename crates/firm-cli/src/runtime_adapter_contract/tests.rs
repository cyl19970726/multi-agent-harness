use std::cell::Cell;
use std::rc::Rc;

use harness_core::agentfirm_api::{
    AgentSessionControlState, AgentSessionStatus, MemberExecutionDriver, NativeContinuationBudget,
    NativeContinuationDefinition, NativeContinuationPhase, PermissionCeiling, RuntimeActivity,
    RuntimeDriverRef, RuntimeResidency,
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
        agent_member_id: "identity-1".to_string(),
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

    fn quiesce(&mut self, fence: RuntimeFence<'_>) -> Result<QuiesceReceipt, RuntimeContractError> {
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

    fn release(&mut self, fence: RuntimeFence<'_>) -> Result<ReleaseReceipt, RuntimeContractError> {
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
        let mut adapter = DeepSeekShapedAdapter::new(full_bindings(), Rc::clone(&dispose_count));
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
