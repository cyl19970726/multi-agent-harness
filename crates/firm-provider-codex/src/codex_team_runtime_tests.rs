use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use harness_core::agentfirm_api::{
    AgentSessionControlState, AgentSessionStatus, NativeSessionAvailability, NativeSessionRef,
    PermissionCeiling, RuntimeActivity, RuntimeCommandBinding, RuntimeDriverRef, RuntimeResidency,
};
use harness_core::{
    AgentRuntimeProvider, ControlTopology, OrdinaryMessageBoundary, ProviderBindingAdmission,
    ProviderCapabilityBinding, ProviderCapabilityEvidence, ProviderCapabilityEvidenceKind,
    ProviderCapabilityStatus, ProviderCompatibilityStatus, ProviderEventFidelity,
    ProviderFeatureMode, ProviderInteractionMode, SecurityEnforcementLocus,
    SecurityEnforcementLocusKind,
};

use super::*;

struct FakeBridge {
    thread_id: String,
    frames: RefCell<VecDeque<Result<Value, RecvTimeoutError>>>,
    thread_status_before_terminal: &'static str,
    thread_status_after_terminal: &'static str,
    goal_status: Option<&'static str>,
    goal_sets: Vec<String>,
    turn_id: String,
    starts: usize,
    interrupts: usize,
    steers: Vec<String>,
    shutdowns: usize,
}

impl FakeBridge {
    fn completed(status: &'static str) -> Self {
        let turn_id = "turn-1".to_string();
        Self {
            thread_id: "thread-1".to_string(),
            frames: RefCell::new(VecDeque::from([Ok(serde_json::json!({
                "method": "turn/completed",
                "params": {
                    "threadId": "thread-1",
                    "turn": {
                        "id": turn_id,
                        "status": status,
                        "items": [{"type": "agentMessage", "text": "## RESULT\ndone"}]
                    }
                }
            }))])),
            thread_status_before_terminal: "idle",
            thread_status_after_terminal: "idle",
            goal_status: None,
            goal_sets: Vec::new(),
            turn_id: "turn-1".to_string(),
            starts: 0,
            interrupts: 0,
            steers: Vec::new(),
            shutdowns: 0,
        }
    }
}

impl CodexAppServerBridge for FakeBridge {
    fn ensure_transport_alive(&mut self) -> CliResult<()> {
        Ok(())
    }
    fn thread_id(&self) -> &str {
        &self.thread_id
    }
    fn start_turn(&mut self, _text: &str) -> CliResult<String> {
        self.starts += 1;
        Ok(self.turn_id.clone())
    }
    fn steer(&mut self, turn_id: &str, text: &str) -> CliResult<String> {
        self.steers.push(text.to_string());
        Ok(turn_id.to_string())
    }
    fn interrupt(&mut self, _turn_id: &str) -> CliResult<()> {
        self.interrupts += 1;
        Ok(())
    }
    fn recv(&self, _timeout: Duration) -> Result<Value, RecvTimeoutError> {
        self.frames
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(RecvTimeoutError::Disconnected))
    }
    fn read_thread(&mut self, include_turns: bool) -> CliResult<Value> {
        let before_terminal = !self.frames.borrow().is_empty();
        let status = if before_terminal {
            self.thread_status_before_terminal
        } else {
            self.thread_status_after_terminal
        };
        let turns = if include_turns && status == "active" {
            serde_json::json!([{
                "id": self.turn_id,
                "status": "inProgress",
            }])
        } else {
            serde_json::json!([])
        };
        Ok(serde_json::json!({
            "id": self.thread_id,
            "status": {"type": status},
            "turns": turns,
        }))
    }
    fn read_thread_goal(&mut self) -> CliResult<Option<Value>> {
        Ok(self.goal_status.map(|status| {
            serde_json::json!({
                "threadId": self.thread_id,
                "status": status,
                "updatedAt": 1,
                "tokensUsed": 0,
            })
        }))
    }
    fn set_thread_goal_status(&mut self, status: &str) -> CliResult<Value> {
        self.goal_status = Some(if status == "paused" {
            "paused"
        } else {
            "active"
        });
        self.goal_sets.push(status.to_string());
        Ok(serde_json::json!({
            "threadId": self.thread_id,
            "status": status,
            "updatedAt": 1,
        }))
    }
    fn shutdown_with_receipt(&mut self) -> CliResult<CodexAppServerShutdownReceipt> {
        self.shutdowns += 1;
        if self.shutdowns != 1 {
            return Err(CliError::Usage("duplicate shutdown".to_string()));
        }
        Ok(CodexAppServerShutdownReceipt {
            process_was_running: true,
            process_reaped: true,
            stdout_reader_joined: true,
            thread_id_retained: true,
            exit_status: "signal: 9".to_string(),
        })
    }
}

fn close_profile_and_session() -> (ProviderIntegrationProfile, AgentSession) {
    let mut profile = ProviderIntegrationProfile {
        agent_runtime_provider: Some(AgentRuntimeProvider("codex".to_string())),
        model_route: None,
        provider: "codex".to_string(),
        execution_mode: "codex_app_server".to_string(),
        execution_driver: harness_core::agentfirm_api::MemberExecutionDriver::HostDriven,
        provider_version: None,
        adapter_contract_version: Some("codex-app-server-v1".to_string()),
        reviewed_provider_versions: vec![REVIEWED_CODEX_APP_SERVER_VERSION.to_string()],
        compatibility_status: ProviderCompatibilityStatus::Unknown,
        adapter_reviewed_at: None,
        compatibility_note: None,
        interaction_mode: ProviderInteractionMode::PauseAndResume,
        ordinary_message_boundary: OrdinaryMessageBoundary::NextRoundBatched,
        plan_mode: ProviderFeatureMode::Native,
        goal_mode: ProviderFeatureMode::Native,
        tool_event_fidelity: ProviderEventFidelity::Structured,
        artifact_event_fidelity: ProviderEventFidelity::Structured,
        supports_cancel: true,
        supports_resume: true,
        observes_native_subagents: false,
        observes_background_tasks: false,
        thinking_transient_only: true,
        control_topology: ControlTopology::ExternalProtocol,
        composition_fingerprint: None,
        capability_fingerprint: None,
        capability_bindings: Vec::new(),
        binding_admission: ProviderBindingAdmission::Failed,
        adapter_bridge_revision: Some("codex-app-server-v1".to_string()),
        security_enforcement_locus: SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::ProviderNativePolicy,
            note: None,
        },
    };
    profile.composition_fingerprint = Some("composition-codex-test".to_string());
    profile.capability_fingerprint = Some("capabilities-codex-test".to_string());
    profile.binding_admission = ProviderBindingAdmission::Active;
    profile.capability_bindings = vec![ProviderCapabilityBinding {
        capability: SemanticCapability::CloseRuntime.as_str().to_string(),
        status: ProviderCapabilityStatus::Verified,
        admission: ProviderBindingAdmission::Active,
        provider_version: Some(REVIEWED_CODEX_APP_SERVER_VERSION.to_string()),
        adapter_revision: Some("codex-app-server-v1".to_string()),
        feature_fingerprint: Some("feature-close".to_string()),
        required_dependencies: Vec::new(),
        evidence: vec![
            ProviderCapabilityEvidence {
                kind: ProviderCapabilityEvidenceKind::DeterministicAcceptance,
                evidence_ref: "test:codex_close_runtime".to_string(),
                observed_at: None,
                note: None,
            },
            ProviderCapabilityEvidence {
                kind: ProviderCapabilityEvidenceKind::LiveCanary,
                evidence_ref: "live:DEV-26:codex_app_server@0.148.0-alpha.9:close_runtime"
                    .to_string(),
                observed_at: None,
                note: None,
            },
        ],
    }];
    let session = AgentSession {
        id: "agent-session-1".to_string(),
        agent_member_id: "identity-1".to_string(),
        node_id: "node-1".to_string(),
        execution_space_id: "space-1".to_string(),
        node_daemon_id: "daemon-1".to_string(),
        node_daemon_generation: 3,
        provider_kind: "codex".to_string(),
        provider_profile_ref: "profile-1".to_string(),
        permission_envelope_ref: "permission-1".to_string(),
        effective_permission_ceiling: PermissionCeiling::FullAccess,
        workspace_cwd: Some("/tmp".to_string()),
        lifecycle: AgentSessionStatus::Idle,
        runtime_generation: 8,
        control_state: AgentSessionControlState {
            runtime_residency: RuntimeResidency::Attached,
            activity: RuntimeActivity::Idle,
            execution_driver: MemberExecutionDriver::HostDriven,
            driver_generation: 12,
            driver_ref: RuntimeDriverRef::NodeDaemon {
                node_daemon_id: "daemon-1".to_string(),
                node_daemon_generation: 3,
            },
            composition_fingerprint: profile.composition_fingerprint.clone(),
            capability_fingerprint: profile.capability_fingerprint.clone(),
            ..Default::default()
        },
        native_session_ref: Some(NativeSessionRef {
            provider: "codex".to_string(),
            execution_mode: "codex_app_server".to_string(),
            native_session_id: "thread-1".to_string(),
            native_locator_kind: "codex_rollout".to_string(),
            provider_version: Some(REVIEWED_CODEX_APP_SERVER_VERSION.to_string()),
            adapter_contract_version: "codex-app-server-v1".to_string(),
            availability: NativeSessionAvailability::Available,
            supports_resume: true,
            last_verified_at: None,
            parent_native_session_id: None,
        }),
        current_turn_id: None,
        queued_input_count: 0,
        version: 1,
        opened_at: "2026-08-15T00:00:00Z".to_string(),
        last_active_at: "2026-08-15T00:00:00Z".to_string(),
        closed_at: None,
    };
    (profile, session)
}

fn binding(session: &AgentSession) -> RuntimeCommandBinding {
    RuntimeCommandBinding {
        target_member_run_id: Some("member-run-1".to_string()),
        target_member_run_generation: Some(session.runtime_generation),
        target_session_id: Some(session.id.clone()),
        target_runtime_generation: Some(session.runtime_generation),
        target_driver_generation: Some(session.control_state.driver_generation),
        target_driver: session.control_state.driver_ref.clone(),
        native_session_ref: session.native_session_ref.clone(),
        composition_fingerprint: session.control_state.composition_fingerprint.clone(),
        capability_fingerprint: session.control_state.capability_fingerprint.clone(),
        capability_profile_version: Some("codex-app-server-v1".to_string()),
        permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
    }
}

fn admitted_fence(session: &AgentSession) -> RuntimeBindingFence {
    let member = harness_core::agentfirm_api::MemberRun {
        id: "member-run-1".to_string(),
        agent_member_id: session.agent_member_id.clone(),
        team_run_id: "team-run-1".to_string(),
        role_snapshot: "member".to_string(),
        provider_profile_snapshot: None,
        requested_controls: serde_json::json!({}),
        effective_controls: serde_json::json!({}),
        coordination_status: harness_core::agentfirm_api::MemberCoordinationStatus::Active,
        runtime_status: harness_core::agentfirm_api::MemberRuntimeStatus::Idle,
        runtime_generation: session.runtime_generation,
        workspace_binding_id: None,
        native_session: session.native_session_ref.clone(),
        version: 1,
        started_at: "t0".to_string(),
        last_event_at: None,
        finished_at: None,
    };
    let daemon = harness_core::NodeDaemonLease {
        node_id: session.node_id.clone(),
        daemon_id: session.node_daemon_id.clone(),
        generation: session.node_daemon_generation,
        instance_id: "instance-1".to_string(),
        status: harness_core::NodeDaemonLeaseStatus::Active,
        acquired_unix_ms: 1,
        renewed_unix_ms: 1,
        expires_unix_ms: 100,
        released_unix_ms: None,
    };
    let command = harness_core::agentfirm_api::RuntimeCommandRecord {
        id: "command-1".to_string(),
        execution_space_id: session.execution_space_id.clone(),
        target_node_id: session.node_id.clone(),
        target_node_daemon_id: daemon.daemon_id.clone(),
        target_node_daemon_generation: daemon.generation,
        authenticated_actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: daemon.daemon_id.clone(),
        },
        command: harness_core::agentfirm_api::RuntimeCommandKind::StartCycle,
        required_capability: "cycle.start".to_string(),
        idempotency_key: "command-1".to_string(),
        request_fingerprint: "fingerprint-1".to_string(),
        status: harness_core::agentfirm_api::RuntimeCommandStatus::Accepted,
        phase: harness_core::agentfirm_api::RuntimeCommandPhase::Prepared,
        effect_certainty: harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown,
        postcondition_status: harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown,
        binding: binding(session),
        precondition: Default::default(),
        postcondition: Default::default(),
        target_session_id: Some(session.id.clone()),
        target_session_generation: Some(session.runtime_generation),
        source_record_id: None,
        provider_attempt: None,
        result: None,
        cycle_correlation: None,
        failure_code: None,
        version: 1,
        created_at: "t0".to_string(),
        updated_at: "t0".to_string(),
    };
    RuntimeBindingFence::from_admitted_command(&command, session, &member, &daemon, None, 2)
        .expect("exact admitted runtime binding")
}

#[test]
fn capability_report_separates_close_from_strong_release() {
    let bindings = capability_bindings();
    let status = |name| {
        bindings
            .iter()
            .find(|binding| binding.capability == name)
            .map(|binding| binding.status)
            .unwrap()
    };
    assert_eq!(status("close_runtime"), CapabilityStatus::Supported);
    assert_eq!(status("quiesce"), CapabilityStatus::Degraded);
    assert_eq!(status("release"), CapabilityStatus::Degraded);
    assert_eq!(
        status("queue_at_native_boundary"),
        CapabilityStatus::Unsupported
    );
}

#[test]
fn cycle_requires_turn_completed_and_exact_idle_observation() {
    let mut adapter = CodexTeamRuntime::new(FakeBridge::completed("completed"));
    let mut accepted = None;
    let outcome = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "hello",
        Duration::from_secs(1),
        &mut |receipt| {
            accepted = receipt.response_id.clone();
            Ok(())
        },
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut CycleControl::default,
    )
    .unwrap();
    assert_eq!(accepted.as_deref(), Some("turn-1"));
    assert_eq!(outcome.native_correlation.provider_input_id, "turn-1");
    assert_eq!(
        outcome
            .native_correlation
            .terminal_provider_input_id
            .as_deref(),
        Some("turn-1")
    );
    assert_eq!(
        outcome.native_correlation.exact_terminal_ref.as_deref(),
        Some("codex.turn.completed:turn-1:completed")
    );
    assert!(outcome.terminal_observation.settled_boundary_observed);
    assert_eq!(outcome.final_text, "## RESULT\ndone");
}

#[test]
fn unknown_thread_status_fails_closed_after_terminal_frame() {
    let mut bridge = FakeBridge::completed("completed");
    bridge.thread_status_after_terminal = "active";
    let mut adapter = CodexTeamRuntime::new(bridge);
    let error = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "hello",
        Duration::from_secs(1),
        &mut |_receipt| Ok(()),
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut CycleControl::default,
    )
    .unwrap_err();
    assert!(error.to_string().contains("not idle"), "{error}");
}

#[test]
fn started_and_terminal_frames_require_the_exact_owned_thread() {
    let mut started_bridge = FakeBridge::completed("completed");
    started_bridge.frames = RefCell::new(VecDeque::from([Ok(serde_json::json!({
        "method": "turn/started",
        "params": {
            "threadId": "thread-other",
            "turn": {"id": "turn-1", "status": "inProgress"}
        }
    }))]));
    let mut started_adapter = CodexTeamRuntime::new(started_bridge);
    let started_error = TeamRuntimeAdapter::run_cycle(
        &mut started_adapter,
        "hello",
        Duration::from_secs(1),
        &mut |_receipt| Ok(()),
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut CycleControl::default,
    )
    .unwrap_err();
    assert!(
        started_error.to_string().contains("ONE_DRIVER_VIOLATION"),
        "{started_error}"
    );

    let terminal_bridge = FakeBridge::completed("completed");
    terminal_bridge.frames.borrow_mut()[0].as_mut().unwrap()["params"]["threadId"] =
        serde_json::json!("thread-other");
    let mut terminal_adapter = CodexTeamRuntime::new(terminal_bridge);
    let terminal_error = TeamRuntimeAdapter::run_cycle(
        &mut terminal_adapter,
        "hello",
        Duration::from_secs(1),
        &mut |_receipt| Ok(()),
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut CycleControl::default,
    )
    .unwrap_err();
    assert!(
        terminal_error.to_string().contains("ONE_DRIVER_VIOLATION"),
        "{terminal_error}"
    );
}

#[test]
fn failed_terminal_is_settled_and_close_does_not_interrupt_it_again() {
    let (profile, session) = close_profile_and_session();
    let fence = admitted_fence(&session);
    let bridge = FakeBridge::completed("failed");
    bridge.frames.borrow_mut()[0].as_mut().unwrap()["params"]["turn"]["error"] = serde_json::json!({
        "message": "provider overloaded",
        "codexErrorInfo": {
            "httpConnectionFailed": {"httpStatusCode": 503}
        },
        "additionalDetails": null
    });
    let mut adapter = CodexTeamRuntime::new(bridge);
    TeamRuntimeAdapter::bind_authority_session(&mut adapter, session, &profile).unwrap();

    let outcome = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "hello",
        Duration::from_secs(1),
        &mut |_receipt| Ok(()),
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut CycleControl::default,
    )
    .unwrap();
    assert_eq!(
        outcome.provider_terminal_failure,
        Some(ProviderTerminalFailure {
            reason: "httpConnectionFailed".to_string(),
            http_status: Some(503),
        })
    );
    assert!(outcome.terminal_observation.settled_boundary_observed);

    let close =
        harness_runtime_contract::RuntimeAdapter::close_runtime(&mut adapter, fence).unwrap();
    close.verify().unwrap();
    let bridge = adapter.into_inner();
    assert_eq!(bridge.interrupts, 0);
    assert_eq!(bridge.shutdowns, 1);
}

#[test]
fn request_user_input_rejects_secret_or_nonblocking_questions_before_routing() {
    let handled = Rc::new(Cell::new(0));
    let handled_by_handler = Rc::clone(&handled);
    let mut adapter = CodexTeamRuntime::new(FakeBridge::completed("completed"))
        .with_provider_request_handler(move |_bridge, _frame| {
            handled_by_handler.set(handled_by_handler.get() + 1);
            Ok(())
        });

    let secret = serde_json::json!({
        "id": 7,
        "method": "item/tool/requestUserInput",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "item-1",
            "isBlocking": true,
            "questions": [{"id": "q1", "header": "Token", "question": "Secret?", "isSecret": true}]
        }
    });
    let error = adapter.handle_provider_request(&secret).unwrap_err();
    assert!(
        error.to_string().contains("secret or unclassified"),
        "{error}"
    );
    assert_eq!(handled.get(), 0);

    let nonblocking = serde_json::json!({
        "id": 8,
        "method": "item/tool/requestUserInput",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "item-2",
            "isBlocking": false,
            "questions": [{"id": "q2", "header": "Choice", "question": "Continue?", "isSecret": false}]
        }
    });
    let error = adapter.handle_provider_request(&nonblocking).unwrap_err();
    assert!(error.to_string().contains("non-blocking"), "{error}");
    assert_eq!(handled.get(), 0);

    let ordinary_blocking = serde_json::json!({
        "id": 9,
        "method": "item/tool/requestUserInput",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "item-3",
            "isBlocking": true,
            "questions": [{"id": "q3", "header": "Choice", "question": "Continue?", "isSecret": false}]
        }
    });
    adapter.handle_provider_request(&ordinary_blocking).unwrap();
    assert_eq!(handled.get(), 1);
}

#[test]
fn interrupt_is_transport_ack_until_matching_terminal_frame() {
    let mut adapter = CodexTeamRuntime::new(FakeBridge::completed("interrupted"));
    let mut first = true;
    let outcome = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "hello",
        Duration::from_secs(1),
        &mut |_receipt| Ok(()),
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut || {
            if std::mem::take(&mut first) {
                CycleControl {
                    interrupt: true,
                    ..Default::default()
                }
            } else {
                CycleControl::default()
            }
        },
    )
    .unwrap();
    assert!(outcome.interrupted);
    assert_eq!(outcome.control_receipts.len(), 1);
    assert_eq!(outcome.control_receipts[0].command, "abort");
    let bridge = adapter.into_inner();
    assert_eq!(bridge.interrupts, 1);
}

#[test]
fn host_driven_cycle_fails_before_turn_start_when_native_goal_is_active() {
    let (profile, session) = close_profile_and_session();
    let mut bridge = FakeBridge::completed("completed");
    bridge.goal_status = Some("active");
    let mut adapter = CodexTeamRuntime::new(bridge);
    TeamRuntimeAdapter::bind_authority_session(&mut adapter, session, &profile).unwrap();
    let error = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "must not start",
        Duration::from_secs(1),
        &mut |_receipt| Ok(()),
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut CycleControl::default,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("native Goal is active"),
        "{error}"
    );
    assert_eq!(adapter.into_inner().starts, 0);
}

#[test]
fn host_driven_cycle_fails_closed_on_an_unclassified_native_goal() {
    let (profile, session) = close_profile_and_session();
    let mut bridge = FakeBridge::completed("completed");
    bridge.goal_status = Some("futureStatus");
    let mut adapter = CodexTeamRuntime::new(bridge);
    TeamRuntimeAdapter::bind_authority_session(&mut adapter, session, &profile).unwrap();
    let error = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "must not start",
        Duration::from_secs(1),
        &mut |_receipt| Ok(()),
        &mut |_pending, _result| Ok(()),
        &mut |_event| {},
        &mut CycleControl::default,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("cannot classify native Goal"),
        "{error}"
    );
    assert_eq!(adapter.into_inner().starts, 0);
}

#[test]
fn close_reaps_once_and_retains_the_native_thread_without_claiming_quiesce() {
    let (profile, session) = close_profile_and_session();
    let fence = admitted_fence(&session);
    let mut adapter = CodexTeamRuntime::new(FakeBridge::completed("completed"));
    TeamRuntimeAdapter::bind_authority_session(&mut adapter, session, &profile).unwrap();
    let receipt =
        harness_runtime_contract::RuntimeAdapter::close_runtime(&mut adapter, fence.clone())
            .unwrap();
    receipt.verify().unwrap();
    assert_eq!(
        receipt.native_session_retained,
        RuntimePostconditionStatus::Satisfied
    );
    assert!(receipt
        .evidence
        .iter()
        .all(|item| !item.contains("flush") && !item.contains("writable")));
    let error =
        harness_runtime_contract::RuntimeAdapter::close_runtime(&mut adapter, fence).unwrap_err();
    assert_eq!(error, RuntimeContractError::AlreadyReleased);
}

#[test]
fn close_inhibits_provider_driven_goal_before_interrupting_its_active_turn() {
    let (profile, mut session) = close_profile_and_session();
    session.control_state.execution_driver = MemberExecutionDriver::ProviderDriven;
    session.control_state.continuation.activation = NativeContinuationActivation::Armed {
        runtime_generation: session.runtime_generation,
        driver_generation: session.control_state.driver_generation,
    };
    let fence = admitted_fence(&session);
    let mut bridge = FakeBridge::completed("interrupted");
    bridge.thread_status_before_terminal = "active";
    bridge.goal_status = Some("active");
    let mut adapter = CodexTeamRuntime::new(bridge);
    TeamRuntimeAdapter::bind_authority_session(&mut adapter, session, &profile).unwrap();

    let receipt =
        harness_runtime_contract::RuntimeAdapter::close_runtime(&mut adapter, fence).unwrap();

    receipt.verify().unwrap();
    let bridge = adapter.into_inner();
    assert_eq!(bridge.goal_sets, vec!["paused"]);
    assert_eq!(bridge.interrupts, 1);
    assert_eq!(bridge.shutdowns, 1);
}

#[test]
fn strong_quiesce_controls_an_active_goal_but_fails_closed_on_unprovable_drain_and_flush() {
    let (mut profile, mut session) = close_profile_and_session();
    profile.capability_bindings[0].capability = SemanticCapability::Quiesce.as_str().to_string();
    session.control_state.execution_driver = MemberExecutionDriver::ProviderDriven;
    session.control_state.continuation.activation = NativeContinuationActivation::Armed {
        runtime_generation: session.runtime_generation,
        driver_generation: session.control_state.driver_generation,
    };
    let fence = admitted_fence(&session);
    let mut bridge = FakeBridge::completed("interrupted");
    bridge.thread_status_before_terminal = "active";
    bridge.goal_status = Some("active");
    let mut adapter = CodexTeamRuntime::new(bridge);
    TeamRuntimeAdapter::bind_authority_session(&mut adapter, session, &profile).unwrap();

    let error = harness_runtime_contract::RuntimeAdapter::quiesce(&mut adapter, fence).unwrap_err();

    assert!(error.to_string().contains("quiesce"), "{error}");
    let bridge = adapter.into_inner();
    assert_eq!(bridge.goal_sets, vec!["paused"]);
    assert_eq!(bridge.interrupts, 1);
    assert_eq!(bridge.shutdowns, 0);
}
