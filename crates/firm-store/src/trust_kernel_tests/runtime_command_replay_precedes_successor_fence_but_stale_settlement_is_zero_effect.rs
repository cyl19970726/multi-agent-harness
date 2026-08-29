use super::*;

#[test]
fn expired_predecessor_replays_and_settles_but_cannot_admit_a_new_effect() {
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
    let successor_error = store
        .acquire_node_daemon_lease(
            "11111111-1111-4111-8111-111111111111",
            "daemon-2",
            "instance-2",
            successor_time,
            60_000,
        )
        .expect_err("an unsettled predecessor cannot be bypassed by a successor");
    assert!(successor_error
        .to_string()
        .contains("NODE_DAEMON_PREDECESSOR_RECOVERY_REQUIRED"));

    let replay = store
        .prepare_runtime_command(&admission_context, &command, successor_time, "t3")
        .expect("exact replay is resolved before expired mutable authority state");
    assert!(replay.replayed);

    let settle_context = MutationContext {
        command_name: "runtime.provider_effect.settle".into(),
        idempotency_key: "runtime-command-fence:settle".into(),
        expected_version: 1,
        ..service_context("unused", "unused", 0)
    };
    let settled = store
        .settle_runtime_command(
            &settle_context,
            "runtime-command-fence",
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            Some(serde_json::json!({"provider_receipt": "spoofed"})),
            None,
            "t4",
        )
        .expect("the exact expired predecessor retains settlement-only authority");
    assert_eq!(settled.projection.status, RuntimeCommandStatus::Applied);
    assert_eq!(
        settled.projection.effect_certainty,
        RuntimeEffectCertainty::Applied
    );

    let mut new_command = command.clone();
    new_command.id = "runtime-command-after-expiry".into();
    new_command.idempotency_key = new_command.id.clone();
    let mut new_context = admission_context.clone();
    new_context.idempotency_key = new_command.id.clone();
    new_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&new_command).unwrap());
    let new_effect_error = store
        .prepare_runtime_command(&new_context, &new_command, successor_time, "t5")
        .expect_err("expired predecessor cannot prepare a new provider effect");
    assert!(
        new_effect_error
            .to_string()
            .contains("SUPERVISOR_GENERATION_FENCED"),
        "{new_effect_error}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn draining_predecessor_settles_prepared_command_but_cannot_prepare_another() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-draining-fence", 0),
            identity("draining-fence"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "runtime-session-draining", 0),
            session("session-runtime-draining", "draining-fence"),
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "runtime-session-draining-active", 1),
            "session-runtime-draining",
            AgentSessionStatus::Active,
            "t2",
        )
        .unwrap();
    let active_session = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|session| session.id == "session-runtime-draining")
        .unwrap();
    let (command, admission) = runtime_command_fixture(
        "runtime-command-before-drain",
        RuntimeCommandKind::DispatchProvider,
        &active_session,
        "dispatch-provider",
    );
    let prepared = store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t3")
        .expect("prepare exact command before drain");

    store
        .drain_node_daemon_lease(
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "instance-1",
            current_unix_ms(),
            60_000,
        )
        .expect("drain exact predecessor generation");

    let mut next_command = command.clone();
    next_command.id = "runtime-command-after-drain".into();
    next_command.idempotency_key = next_command.id.clone();
    let mut next_admission = admission.clone();
    next_admission.idempotency_key = next_command.id.clone();
    next_admission.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&next_command).unwrap());
    let operations_before_rejection = store.canonical_operations().unwrap();
    let admission_error = store
        .prepare_runtime_command(&next_admission, &next_command, current_unix_ms(), "t4")
        .expect_err("draining predecessor cannot prepare another effect");
    assert!(
        admission_error
            .to_string()
            .contains("SUPERVISOR_GENERATION_FENCED"),
        "{admission_error}"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_rejection,
        "rejected admission must have zero durable effect"
    );

    let settled = store
        .settle_runtime_command(
            &service_context(
                "node_daemon.runtime.settle",
                "runtime-command-before-drain:settle",
                prepared.projection.version,
            ),
            &command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            Some(serde_json::json!({"provider_receipt": "receipt-before-drain"})),
            None,
            "t5",
        )
        .expect("draining predecessor retains settlement-only authority");
    assert_eq!(settled.projection.status, RuntimeCommandStatus::Applied);
    assert_eq!(
        settled.projection.effect_certainty,
        RuntimeEffectCertainty::Applied
    );
    fs::remove_dir_all(root).unwrap();
}
