use super::*;

#[test]
fn standalone_session_is_machine_owned_and_team_membership_is_only_an_overlay() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.create", "standalone-identity", 0),
            identity("standalone-agent"),
        )
        .unwrap();
    assert!(store
        .fabric_team_memberships("space-test")
        .unwrap()
        .is_empty());

    let standalone = session("session-standalone", "standalone-agent");
    let payload = serde_json::json!({
        "session_id": standalone.id,
        "session_generation": standalone.runtime_generation,
        "session": standalone,
    });
    let command = ControlCommandEnvelope {
        id: "runtime-start-standalone".into(),
        execution_space_id: "space-test".into(),
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        target_node_daemon_id: "daemon-1".into(),
        target_node_daemon_generation: 1,
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        },
        command: RuntimeCommandKind::StartSession,
        required_capability: "agent_session.start".into(),
        idempotency_key: "runtime-start-standalone".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 60_000,
        binding: test_runtime_binding("session-standalone"),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&payload),
        payload,
        issued_at: "t-start".into(),
    };
    let mut start_context =
        service_context("node_daemon.runtime.prepare", "runtime-start-standalone", 0);
    start_context.authority_actor = Some(command.authenticated_actor.clone());
    start_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&command).unwrap());
    store
        .prepare_runtime_command(&start_context, &command, current_unix_ms(), "t-start")
        .expect("standalone StartSession admission does not require TeamMembership");
    store
        .create_agent_session(
            &service_context("session.create", "session-standalone", 0),
            session("session-standalone", "standalone-agent"),
        )
        .unwrap();

    append_runtime_team(&store, "team-a", "team-run-a");
    append_runtime_team(&store, "team-b", "team-run-b");
    let membership_a = join_runtime_membership(
        &store,
        "membership-standalone-a",
        "team-a",
        "standalone-agent",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    join_runtime_membership(
        &store,
        "membership-standalone-b",
        "team-b",
        "standalone-agent",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let sessions_before_leave = store.fabric_agent_sessions("space-test").unwrap();
    let mut leave_context = context(
        "standalone-agent",
        "membership.leave",
        "membership-standalone-a:leave",
        1,
    );
    leave_context.authenticated_actor.kind = ActorKind::AgentMember;
    store
        .leave_team_membership(&leave_context, &membership_a.id, "t-leave-a")
        .unwrap();
    assert_eq!(
        store.fabric_agent_sessions("space-test").unwrap(),
        sessions_before_leave,
        "joining or leaving Team overlays must not create, close, or rewrite the machine AgentSession"
    );
    assert!(store
        .fabric_team_memberships("space-test")
        .unwrap()
        .iter()
        .any(|membership| {
            membership.team_id == "team-b"
                && membership.agent_member_id == "standalone-agent"
                && membership.state == TeamMembershipStatus::Active
        }));
    fs::remove_dir_all(root).unwrap();
}
