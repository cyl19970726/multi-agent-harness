use super::*;

#[test]
fn team_host_cannot_stop_shared_session_and_active_bindings_require_explicit_release() {
    let (store, root) = fabric_store();
    for identity_id in ["shared-agent", "host-a", "host-b"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "operator",
                    "identity.create",
                    &format!("identity-{identity_id}"),
                    0,
                ),
                identity(identity_id),
            )
            .unwrap();
    }
    let shared_session = session("session-shared", "shared-agent");
    store
        .create_agent_session(
            &service_context("session.create", "session-shared", 0),
            shared_session.clone(),
        )
        .unwrap();
    append_runtime_team(&store, "team-a", "team-run-a");
    append_runtime_team(&store, "team-b", "team-run-b");
    let shared_a = join_runtime_membership(
        &store,
        "membership-shared-a",
        "team-a",
        "shared-agent",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let shared_b = join_runtime_membership(
        &store,
        "membership-shared-b",
        "team-b",
        "shared-agent",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    assert_eq!(
        store
            .team_host_membership("space-test", "team-a", true)
            .unwrap()
            .agent_member_id,
        "host-a"
    );
    assert_eq!(
        store
            .team_host_membership("space-test", "team-b", true)
            .unwrap()
            .agent_member_id,
        "host-b"
    );
    let work_a = insert_runtime_work(&store, "work-a", "team-a", "team-run-a");
    let work_b = insert_runtime_work(&store, "work-b", "team-b", "team-run-b");
    for (id, work, membership) in [
        ("binding-a", &work_a, &shared_a),
        ("binding-b", &work_b, &shared_b),
    ] {
        store
            .bind_work_execution(
                &context("fixture-host", "work.bind", id, 0),
                WorkExecutionBinding {
                    id: id.into(),
                    work_id: work.id.clone(),
                    work_revision: work.version,
                    team_id: membership.team_id.clone(),
                    team_membership_id: membership.id.clone(),
                    agent_member_id: "shared-agent".into(),
                    agent_session_id: shared_session.id.clone(),
                    agent_session_generation: shared_session.runtime_generation,
                    delivery_id: format!("delivery-{id}"),
                    binding_generation: 1,
                    status: WorkExecutionBindingStatus::Active,
                    version: 1,
                    created_by: actor("fixture-host"),
                    bound_at: "t-bound".into(),
                    ended_at: None,
                },
            )
            .unwrap();
    }

    let (mut host_command, mut host_context) = runtime_command_fixture(
        "runtime-host-a-stop-shared",
        RuntimeCommandKind::StopSession,
        &shared_session,
        "stop_session",
    );
    host_command.authenticated_actor = ActorRef {
        kind: ActorKind::AgentMember,
        id: "host-a".into(),
    };
    host_context.authenticated_actor = host_command.authenticated_actor.clone();
    host_context.authority_actor = Some(host_command.authenticated_actor.clone());
    host_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&host_command).unwrap());
    let before_host = (
        store.canonical_operations().unwrap(),
        store.fabric_agent_sessions("space-test").unwrap(),
        store.fabric_work_execution_bindings("space-test").unwrap(),
        store.runtime_commands("space-test").unwrap(),
    );
    let host_error = store
        .prepare_runtime_command(&host_context, &host_command, current_unix_ms(), "t-host-a")
        .expect_err("Team A Host has no authority over the shared machine Session");
    assert!(host_error
        .to_string()
        .contains("Team Host authority is Team-scoped"));
    assert_eq!(
        (
            store.canonical_operations().unwrap(),
            store.fabric_agent_sessions("space-test").unwrap(),
            store.fabric_work_execution_bindings("space-test").unwrap(),
            store.runtime_commands("space-test").unwrap(),
        ),
        before_host,
        "cross-Team Host rejection must have zero canonical/session/binding/command side effects"
    );

    let (operator_command, operator_context) = runtime_command_fixture(
        "runtime-operator-stop-bound",
        RuntimeCommandKind::StopSession,
        &shared_session,
        "stop_session",
    );
    let before_bound_stop = (
        store.canonical_operations().unwrap(),
        store.fabric_agent_sessions("space-test").unwrap(),
        store.fabric_work_execution_bindings("space-test").unwrap(),
        store.runtime_commands("space-test").unwrap(),
    );
    let bound_error = store
        .prepare_runtime_command(
            &operator_context,
            &operator_command,
            current_unix_ms(),
            "t-bound-stop",
        )
        .expect_err("StopSession must not auto-release cross-Team Work bindings");
    assert!(bound_error
        .to_string()
        .contains("WORK_EXECUTION_BINDING_ACTIVE"));
    assert!(bound_error
        .to_string()
        .contains("explicit release, rebind, or quiesce"));
    assert_eq!(
        (
            store.canonical_operations().unwrap(),
            store.fabric_agent_sessions("space-test").unwrap(),
            store.fabric_work_execution_bindings("space-test").unwrap(),
            store.runtime_commands("space-test").unwrap(),
        ),
        before_bound_stop,
        "binding-fenced StopSession must have zero side effects"
    );

    for binding_id in ["binding-a", "binding-b"] {
        let mut release_context = context(
            "shared-agent",
            "work_binding.release",
            &format!("release-{binding_id}"),
            1,
        );
        release_context.authenticated_actor.kind = ActorKind::AgentMember;
        store
            .release_work_execution_binding(&release_context, binding_id, "t-release")
            .unwrap();
    }
    let accepted = store
        .prepare_runtime_command(
            &operator_context,
            &operator_command,
            current_unix_ms(),
            "t-stop-after-release",
        )
        .expect("explicit release makes the exact StopSession admissible");
    assert_eq!(accepted.projection.status, RuntimeCommandStatus::Accepted);
    assert!(store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .iter()
        .all(|binding| binding.status == WorkExecutionBindingStatus::Released));
    fs::remove_dir_all(root).unwrap();
}
