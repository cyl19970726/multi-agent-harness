use super::*;
use firm_core::agentfirm_api::{ActorKind, ActorRef, MutationContext, RuntimeDriverRef};

#[test]
fn real_trust_acceptance_selects_blocked_responsibility_without_changing_it() {
    let (root, store, run, member, other) = work_test_fixture("acceptance-wake");
    let make_active = |id: &str| {
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, id),
                host_work_context(
                    &format!("create-{id}"),
                    &format!("create-{id}"),
                    "unix-ms:2",
                ),
            )
            .unwrap();
        let work = assign_test_work_to_member(
            &store,
            &run,
            &work,
            &member,
            &format!("assign-{id}"),
            &format!("assign-{id}"),
            "unix-ms:3",
        );
        start_claimed_work_for_test(
            &store,
            &work,
            &member,
            &format!("start-{id}"),
            &format!("start-{id}"),
            "unix-ms:4",
        )
    };
    let standing = make_active("standing");
    let standing = store
        .block_work_as_host(
            &standing.id,
            standing.version,
            "waiting for review",
            host_work_context("block-standing", "block-standing", "unix-ms:5"),
        )
        .unwrap();
    let slice = make_active("slice");
    let submitted = submit_started_work_for_test(
        &store,
        &slice,
        &member,
        "slice-result",
        "done",
        (vec!["artifact://slice".into()], vec![]),
        "unix-ms:6",
    );
    let space = store.current_team_run_execution_space(&run).unwrap();
    let lease = store
        .acquire_team_supervisor_under_node_lease(
            &run.id,
            &run.execution_node_id,
            "test-node-daemon",
            1,
            &space,
            &run.project_binding_id,
            "test-supervisor",
            std::process::id(),
            "loopback://test",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            60_000,
        )
        .unwrap();
    let session = store
        .fabric_agent_sessions(&space)
        .unwrap()
        .into_iter()
        .find(|s| s.agent_member_id == member.agent_member_id)
        .unwrap();
    let mut control = session.control_state.clone();
    control.driver_generation += 1;
    control.driver_ref = RuntimeDriverRef::TeamSupervisor {
        team_run_id: run.id.clone(),
        team_supervisor_id: lease.supervisor_id,
        team_supervisor_generation: lease.generation,
    };
    store
        .bind_agent_session_control_state(
            &MutationContext {
                execution_space_id: space.clone(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: "test-node-daemon".into(),
                },
                authority_actor: None,
                command_name: "test.bind-control".into(),
                idempotency_key: "bind-control".into(),
                expected_version: session.version,
                request_fingerprint: None,
            },
            &session.id,
            session.runtime_generation,
            control,
            "unix-ms:7",
        )
        .unwrap();
    assert!(store
        .pending_work_acceptance_wake(&space, &member.id)
        .unwrap()
        .is_none());
    accept_result_for_test(
        &store,
        &submitted,
        "slice-result",
        "accept-slice",
        "unix-ms:8",
    );
    let wake = store
        .pending_work_acceptance_wake(&space, &member.id)
        .unwrap()
        .unwrap();
    assert_eq!(wake.accepted_work_id, slice.id);
    assert_eq!(
        wake.blocked_work_ids.as_slice(),
        std::slice::from_ref(&standing.id)
    );
    assert!(store
        .pending_work_acceptance_wake(&space, &other.id)
        .unwrap()
        .is_none());
    assert!(store
        .pending_work_acceptance_wake("foreign-space", &member.id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|w| w.id == standing.id)
            .unwrap(),
        standing
    );
    assert!(
        !store
            .work_operations()
            .unwrap()
            .iter()
            .any(|op| op.event.id == wake.acceptance_event_id),
        "acceptance must come from the trust reader, not an old WorkOperation fixture"
    );
    let target = store
        .fabric_agent_sessions(&space)
        .unwrap()
        .into_iter()
        .find(|s| s.id == session.id)
        .unwrap();
    let (command, admission) = acceptance_command(
        &target,
        &member.id,
        &wake.source_record_id(),
        "accept-wake-1",
    );
    store
        .transition_agent_session(
            &MutationContext {
                command_name: "test.session.active".into(),
                idempotency_key: "session-active".into(),
                expected_version: target.version,
                request_fingerprint: None,
                ..admission.clone()
            },
            &target.id,
            firm_core::agentfirm_api::AgentSessionStatus::Active,
            "unix-ms:9",
        )
        .unwrap();
    let prepared = store
        .prepare_runtime_command(&admission, &command, now_ms(), "unix-ms:9")
        .unwrap();
    assert!(store
        .pending_work_acceptance_wake(&space, &member.id)
        .unwrap()
        .is_none());
    assert!(
        store
            .prepare_runtime_command(&admission, &command, now_ms(), "unix-ms:9")
            .unwrap()
            .replayed
    );
    let (duplicate, duplicate_admission) = acceptance_command(
        &target,
        &member.id,
        &wake.source_record_id(),
        "accept-wake-round-2",
    );
    assert!(store
        .prepare_runtime_command(&duplicate_admission, &duplicate, now_ms(), "unix-ms:10")
        .is_err());
    let settle = MutationContext {
        command_name: "node_daemon.runtime.settle".into(),
        idempotency_key: "wake-not-applied".into(),
        expected_version: prepared.projection.version,
        request_fingerprint: None,
        ..admission.clone()
    };
    store
        .settle_runtime_command(
            &settle,
            &command.id,
            firm_core::agentfirm_api::RuntimeCommandPhase::Settled,
            firm_core::agentfirm_api::RuntimeEffectCertainty::NotApplied,
            Some(serde_json::json!({"fixture": "input never reached provider"})),
            None,
            "unix-ms:11",
        )
        .unwrap();
    assert!(
        store
            .pending_work_acceptance_wake(&space, &member.id)
            .unwrap()
            .is_none(),
        "a proven NotApplied preparation still consumes this automatic attempt"
    );
    assert!(store
        .prepare_runtime_command(&duplicate_admission, &duplicate, now_ms(), "unix-ms:12")
        .unwrap_err()
        .to_string()
        .contains("WORK_ACCEPTANCE_WAKE_UNAVAILABLE"));
    // Close through a proven provider Close receipt before replacing Session.
    let close = firm_core::TeamMemberCloseRequest {
        id: "wake-close".into(),
        team_run_id: run.id.clone(),
        member_run_id: member.id.clone(),
        requested_by: "agent-host".into(),
        reason: "fixture runtime closed".into(),
        status: firm_core::TeamMemberCloseStatus::Pending,
        requested_at: "unix-ms:12".into(),
        applied_at: None,
        detached_recovery_fence: None,
    };
    store.latch_team_member_close(&close).unwrap();
    let (mut close_command, mut close_admission) = acceptance_command(
        &target,
        &member.id,
        "wake-close:idle:close-runtime",
        "wake-close-command",
    );
    close_command.command = firm_core::agentfirm_api::RuntimeCommandKind::CloseMember;
    close_command.required_capability = "member.close".into();
    close_command.payload["operation"] = "close_member".into();
    close_command.payload_fingerprint = canonical_json_fingerprint(&close_command.payload);
    close_command.postcondition.desired_ack_level =
        firm_core::agentfirm_api::RuntimeAcknowledgementLevel::ProviderReceipt;
    close_command.postcondition.desired_postcondition =
        firm_core::agentfirm_api::RuntimeDesiredPostcondition::RuntimeReleased;
    close_admission.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&close_command).unwrap());
    let prepared_close = store
        .prepare_runtime_command(&close_admission, &close_command, now_ms(), "unix-ms:12")
        .unwrap();
    store
        .settle_runtime_command_with_postcondition(
            &MutationContext {
                command_name: "runtime.closemember.settle".into(),
                idempotency_key: "wake-close-settle".into(),
                expected_version: prepared_close.projection.version,
                request_fingerprint: None,
                ..admission.clone()
            },
            &close_command.id,
            firm_core::agentfirm_api::RuntimeCommandPhase::Settled,
            firm_core::agentfirm_api::RuntimeEffectCertainty::Applied,
            firm_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({"closed": true})),
            None,
            "unix-ms:12",
        )
        .unwrap();
    let binding = store
        .fabric_work_execution_bindings(&space)
        .unwrap()
        .into_iter()
        .find(|b| b.work_id == standing.id)
        .unwrap();
    store
        .release_work_execution_binding_for_member_close(
            &MutationContext {
                command_name: "work.release.close".into(),
                idempotency_key: "wake-release-close".into(),
                expected_version: binding.version,
                request_fingerprint: None,
                ..admission.clone()
            },
            &binding.id,
            &close.id,
            &close_command.id,
            &member.id,
            1,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "unix-ms:12",
        )
        .unwrap();
    for (key, status) in [
        (
            "old-idle",
            firm_core::agentfirm_api::AgentSessionStatus::Idle,
        ),
        (
            "old-closed",
            firm_core::agentfirm_api::AgentSessionStatus::Closed,
        ),
    ] {
        let current = store
            .fabric_agent_sessions(&space)
            .unwrap()
            .into_iter()
            .find(|s| s.id == target.id)
            .unwrap();
        store
            .transition_agent_session(
                &MutationContext {
                    command_name: "test.session.transition".into(),
                    idempotency_key: key.into(),
                    expected_version: current.version,
                    request_fingerprint: None,
                    ..admission.clone()
                },
                &current.id,
                status,
                "unix-ms:12",
            )
            .unwrap();
    }
    store
        .complete_team_member_close(&run.id, &member.id, &close.id, "unix-ms:12")
        .unwrap();
    let mut successor = target.clone();
    successor.id = "session-successor".into();
    successor.runtime_generation += 1;
    successor.version = 1;
    successor.lifecycle = firm_core::agentfirm_api::AgentSessionStatus::Idle;
    store
        .create_agent_session(
            &MutationContext {
                command_name: "test.session.create".into(),
                idempotency_key: "session-successor".into(),
                expected_version: 0,
                request_fingerprint: None,
                ..admission.clone()
            },
            successor.clone(),
        )
        .unwrap();
    store
        .transition_agent_session(
            &MutationContext {
                command_name: "test.session.active".into(),
                idempotency_key: "successor-active".into(),
                expected_version: 1,
                request_fingerprint: None,
                ..admission.clone()
            },
            &successor.id,
            firm_core::agentfirm_api::AgentSessionStatus::Active,
            "unix-ms:13",
        )
        .unwrap();
    let (next_command, next_admission) = acceptance_command(
        &successor,
        &member.id,
        &wake.source_record_id(),
        "accept-wake-new-session",
    );
    assert!(
        store
            .prepare_runtime_command(&next_admission, &next_command, now_ms(), "unix-ms:14")
            .unwrap_err()
            .to_string()
            .contains("WORK_ACCEPTANCE_WAKE_UNAVAILABLE"),
        "changing the Session ID and runtime generation cannot consume acceptance twice"
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn acceptance_command(
    session: &firm_core::agentfirm_api::AgentSession,
    member_run_id: &str,
    source: &str,
    id: &str,
) -> (
    firm_core::agentfirm_api::ControlCommandEnvelope,
    MutationContext,
) {
    use firm_core::agentfirm_api::*;
    let payload = serde_json::json!({"session_id": session.id,
        "session_generation": session.runtime_generation, "operation": "start_cycle", "provider_attempt": 1, "delivery_id": source});
    let actor = ActorRef {
        kind: ActorKind::Service,
        id: session.node_daemon_id.clone(),
    };
    let command = ControlCommandEnvelope {
        id: id.into(),
        execution_space_id: session.execution_space_id.clone(),
        target_node_id: session.node_id.clone(),
        target_node_daemon_id: session.node_daemon_id.clone(),
        target_node_daemon_generation: session.node_daemon_generation,
        authenticated_actor: actor.clone(),
        command: RuntimeCommandKind::StartCycle,
        required_capability: "cycle.start".into(),
        idempotency_key: id.into(),
        expected_version: 0,
        expires_unix_ms: now_ms() + 60_000,
        binding: RuntimeCommandBinding {
            target_session_id: Some(session.id.clone()),
            target_runtime_generation: Some(session.runtime_generation),
            target_member_run_id: Some(member_run_id.into()),
            target_member_run_generation: Some(1),
            target_driver_generation: Some(session.control_state.driver_generation),
            target_driver: session.control_state.driver_ref.clone(),
            native_session_ref: session.native_session_ref.clone(),
            composition_fingerprint: session.control_state.composition_fingerprint.clone(),
            capability_fingerprint: session.control_state.capability_fingerprint.clone(),
            permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
            ..Default::default()
        },
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&payload),
        payload,
        issued_at: "unix-ms:9".into(),
    };
    let context = MutationContext {
        execution_space_id: session.execution_space_id.clone(),
        authenticated_actor: actor.clone(),
        authority_actor: Some(actor),
        command_name: "node_daemon.runtime.prepare".into(),
        idempotency_key: id.into(),
        expected_version: 0,
        request_fingerprint: Some(runtime_command_envelope_fingerprint(&command).unwrap()),
    };
    (command, context)
}
