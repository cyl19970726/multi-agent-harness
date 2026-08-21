use super::*;

#[test]
fn runtime_command_hostile_member_and_permission_widening_have_zero_side_effects() {
    let (store, root) = fabric_store();
    for identity_id in ["runtime-owner", "runtime-sibling"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "host",
                    "identity.create",
                    &format!("identity-{identity_id}"),
                    0,
                ),
                identity(identity_id),
            )
            .unwrap();
    }
    let owner_session = session("session-runtime-owner", "runtime-owner");
    store
        .create_agent_session(
            &service_context("session.create", "session-runtime-owner", 0),
            owner_session.clone(),
        )
        .unwrap();

    let (mut hostile_command, mut hostile_context) = runtime_command_fixture(
        "runtime-hostile-sibling",
        RuntimeCommandKind::StopSession,
        &owner_session,
        "stop_session",
    );
    hostile_command.authenticated_actor = ActorRef {
        kind: ActorKind::AgentMember,
        id: "runtime-sibling".into(),
    };
    hostile_context.authenticated_actor = hostile_command.authenticated_actor.clone();
    hostile_context.authority_actor = Some(hostile_command.authenticated_actor.clone());
    hostile_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&hostile_command).unwrap());
    let operations_before_hostile = store.canonical_operations().unwrap();
    let sessions_before_hostile = store.fabric_agent_sessions("space-test").unwrap();
    let commands_before_hostile = store.runtime_commands("space-test").unwrap();
    let error = store
        .prepare_runtime_command(
            &hostile_context,
            &hostile_command,
            current_unix_ms(),
            "t-hostile",
        )
        .expect_err("an ordinary sibling Member cannot control this AgentSession");
    assert!(error.to_string().contains("exact self or exact machine"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_hostile
    );
    assert_eq!(
        store.fabric_agent_sessions("space-test").unwrap(),
        sessions_before_hostile
    );
    assert_eq!(
        store.runtime_commands("space-test").unwrap(),
        commands_before_hostile
    );

    let mut widened = session("session-runtime-widened", "runtime-owner");
    widened.effective_permission_ceiling = PermissionCeiling::FullAccess;
    let payload = serde_json::json!({"session": widened});
    let widening_command = ControlCommandEnvelope {
        id: "runtime-permission-widening".into(),
        execution_space_id: "space-test".into(),
        target_node_id: owner_session.node_id.clone(),
        target_node_daemon_id: owner_session.node_daemon_id.clone(),
        target_node_daemon_generation: owner_session.node_daemon_generation,
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: owner_session.node_daemon_id.clone(),
        },
        command: RuntimeCommandKind::StartSession,
        required_capability: "agent_session.start".into(),
        idempotency_key: "runtime-permission-widening".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 60_000,
        binding: test_runtime_binding("session-runtime-widened"),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&payload),
        payload,
        issued_at: "t-widening".into(),
    };
    let mut widening_context = service_context(
        "node_daemon.runtime.prepare",
        "runtime-permission-widening",
        0,
    );
    widening_context.authority_actor = Some(widening_command.authenticated_actor.clone());
    widening_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&widening_command).unwrap());
    let operations_before_widening = store.canonical_operations().unwrap();
    let sessions_before_widening = store.fabric_agent_sessions("space-test").unwrap();
    let commands_before_widening = store.runtime_commands("space-test").unwrap();
    let error = store
        .prepare_runtime_command(
            &widening_context,
            &widening_command,
            current_unix_ms(),
            "t-widening",
        )
        .expect_err("StartSession cannot widen the AgentIdentity ceiling");
    assert!(error.to_string().contains("cannot widen"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_widening
    );
    assert_eq!(
        store.fabric_agent_sessions("space-test").unwrap(),
        sessions_before_widening
    );
    assert_eq!(
        store.runtime_commands("space-test").unwrap(),
        commands_before_widening
    );
    fs::remove_dir_all(root).unwrap();
}
