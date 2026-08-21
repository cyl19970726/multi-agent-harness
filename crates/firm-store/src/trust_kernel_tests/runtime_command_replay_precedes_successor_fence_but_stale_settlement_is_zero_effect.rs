use super::*;

#[test]
fn runtime_command_replay_precedes_successor_fence_but_stale_settlement_is_zero_effect() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-runtime-fence", 0),
            identity("runtime-fence"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "runtime-session-fence", 0),
            session("session-runtime-fence", "runtime-fence"),
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "runtime-session-fence-active", 1),
            "session-runtime-fence",
            AgentSessionStatus::Active,
            "t2",
        )
        .unwrap();
    let payload = serde_json::json!({
        "session_id": "session-runtime-fence",
        "session_generation": 1,
        "delivery_id": "delivery-1",
    });
    let command = ControlCommandEnvelope {
        id: "runtime-command-fence".into(),
        execution_space_id: "space-test".into(),
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        target_node_daemon_id: "daemon-1".into(),
        target_node_daemon_generation: 1,
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        },
        command: firm_core::agentfirm_api::RuntimeCommandKind::DispatchProvider,
        required_capability: "provider.dispatch".into(),
        idempotency_key: "runtime-command-fence".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 120_000,
        binding: test_runtime_binding("session-runtime-fence"),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&payload),
        payload,
        issued_at: "t2".into(),
    };
    let mut admission_context = service_context(
        "runtime.provider_effect.prepare",
        "runtime-command-fence",
        0,
    );
    admission_context.authority_actor = Some(ActorRef {
        kind: ActorKind::Service,
        id: "daemon-1".into(),
    });
    admission_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&command).unwrap());
    store
        .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
        .unwrap();

    let successor_time = current_unix_ms() + 60_001;
    store
        .acquire_node_daemon_lease(
            "11111111-1111-4111-8111-111111111111",
            "daemon-2",
            "instance-2",
            successor_time,
            60_000,
        )
        .unwrap();

    let replay = store
        .prepare_runtime_command(&admission_context, &command, successor_time, "t3")
        .expect("exact replay is resolved before mutable successor state");
    assert!(replay.replayed);

    let operations_before = store.canonical_operations().unwrap();
    let settle_context = MutationContext {
        command_name: "runtime.provider_effect.settle".into(),
        idempotency_key: "runtime-command-fence:settle".into(),
        expected_version: 1,
        ..service_context("unused", "unused", 0)
    };
    let error = store
        .settle_runtime_command(
            &settle_context,
            "runtime-command-fence",
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            Some(serde_json::json!({"provider_receipt": "spoofed"})),
            None,
            "t4",
        )
        .expect_err("superseded daemon cannot settle an effect");
    assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), operations_before);
    fs::remove_dir_all(root).unwrap();
}
