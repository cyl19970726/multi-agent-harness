use super::*;

#[test]
fn admitted_stop_closes_exact_session_once_and_replays_after_terminal_state() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-runtime-stop", 0),
            identity("runtime-stop"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "runtime-stop-session", 0),
            session("session-runtime-stop", "runtime-stop"),
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "runtime-stop-active", 1),
            "session-runtime-stop",
            AgentSessionStatus::Active,
            "t2",
        )
        .unwrap();
    let daemon = ActorRef {
        kind: ActorKind::Service,
        id: "daemon-1".into(),
    };
    let payload = serde_json::json!({
        "session_id": "session-runtime-stop",
        "session_generation": 1,
        "delivery_id": "stop-control",
    });
    let command = ControlCommandEnvelope {
        id: "runtime-stop-command".into(),
        execution_space_id: "space-test".into(),
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        target_node_daemon_id: "daemon-1".into(),
        target_node_daemon_generation: 1,
        authenticated_actor: daemon.clone(),
        command: RuntimeCommandKind::StopSession,
        required_capability: "agent_session.stop".into(),
        idempotency_key: "runtime-stop-once".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 60_000,
        binding: test_runtime_binding("session-runtime-stop"),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&payload),
        payload,
        issued_at: "t2".into(),
    };
    let mut admission_context = service_context("runtime.stopsession", "runtime-stop-once", 0);
    admission_context.authority_actor = Some(daemon);
    admission_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&command).unwrap());
    let admitted = store
        .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
        .unwrap();
    store
        .transition_agent_session(
            &service_context("runtime.stopsession.effect", "runtime-stop-once:effect", 2),
            "session-runtime-stop",
            AgentSessionStatus::Closed,
            "t3",
        )
        .unwrap();
    store
        .settle_runtime_command(
            &service_context(
                "runtime.stopsession.settle",
                "runtime-stop-once:settle",
                admitted.projection.version,
            ),
            &command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            Some(serde_json::json!({"closed": true})),
            None,
            "t3",
        )
        .unwrap();
    let operations_before_replay = store.canonical_operations().unwrap();
    let replay = store
        .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t4")
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.projection.status, RuntimeCommandStatus::Applied);
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_replay
    );
    assert_eq!(
        store.fabric_agent_sessions("space-test").unwrap()[0].lifecycle,
        AgentSessionStatus::Closed
    );
    fs::remove_dir_all(root).unwrap();
}
