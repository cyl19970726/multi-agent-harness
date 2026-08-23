use super::*;

#[test]
fn runtime_command_replay_and_ambiguous_effect_fail_closed() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-runtime", 0),
            identity("runtime-agent"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "runtime-session", 0),
            session("session-runtime", "runtime-agent"),
        )
        .unwrap();
    let payload = serde_json::json!({
        "session_id": "session-runtime",
        "session_generation": 1,
    });
    let fingerprint = canonical_json_fingerprint(&payload);
    let command = ControlCommandEnvelope {
        id: "runtime-command-1".into(),
        execution_space_id: "space-test".into(),
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        target_node_daemon_id: "daemon-1".into(),
        target_node_daemon_generation: 1,
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        },
        command: firm_core::agentfirm_api::RuntimeCommandKind::StopSession,
        required_capability: "agent_session.stop".into(),
        idempotency_key: "runtime-command-1".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 60_000,
        binding: test_runtime_binding("session-runtime"),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload,
        payload_fingerprint: fingerprint.clone(),
        issued_at: "t2".into(),
    };
    let command_fingerprint = runtime_command_envelope_fingerprint(&command).unwrap();
    let admission_context = MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        },
        authority_actor: Some(ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        }),
        command_name: "runtime.stop".into(),
        idempotency_key: "runtime-command-1".into(),
        expected_version: 0,
        request_fingerprint: Some(command_fingerprint),
    };
    let accepted = store
        .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
        .unwrap();
    assert_eq!(accepted.projection.status, RuntimeCommandStatus::Accepted);
    assert_eq!(
        accepted.projection.effect_certainty,
        RuntimeEffectCertainty::Unknown
    );
    let replay = store
        .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t2")
        .unwrap();
    assert!(replay.replayed);

    let mut second = command.clone();
    second.id = "runtime-command-2".into();
    second.idempotency_key = "runtime-command-2".into();
    let before = store.canonical_operations().unwrap().len();
    let mut second_context = admission_context.clone();
    second_context.idempotency_key = "runtime-command-2".into();
    second_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&second).unwrap());
    let error = store
        .prepare_runtime_command(&second_context, &second, current_unix_ms(), "t3")
        .expect_err("ambiguous accepted command fences a successor");
    assert_eq!(
        error
            .trust_error()
            .expect("ambiguous effect remains a typed TrustError")
            .code,
        TrustErrorCode::RuntimeEffectUnknown
    );
    assert_eq!(store.canonical_operations().unwrap().len(), before);

    let settle_context = MutationContext {
        command_name: "runtime.stop.settle".into(),
        idempotency_key: "runtime-command-1:settle".into(),
        expected_version: 1,
        authority_actor: Some(actor("host")),
        ..service_context("unused", "unused", 0)
    };
    store
        .settle_runtime_command(
            &settle_context,
            "runtime-command-1",
            RuntimeCommandStatus::RecoveryRequired,
            RuntimeEffectCertainty::Unknown,
            None,
            Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
            "t4",
        )
        .unwrap();
    fs::remove_dir_all(root).unwrap();
}
