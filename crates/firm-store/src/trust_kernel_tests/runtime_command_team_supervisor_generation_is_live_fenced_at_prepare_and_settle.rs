use super::*;

#[test]
fn runtime_command_team_supervisor_generation_is_live_fenced_at_prepare_and_settle() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-supervisor", "run-supervisor");
    let lease = store
        .acquire_team_supervisor_under_node_lease(
            "run-supervisor",
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "space-test",
            "project-1",
            "supervisor-1",
            std::process::id(),
            "loopback://supervisor-1",
            current_unix_ms(),
            60_000,
        )
        .unwrap();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "supervised-agent", 0),
            identity("supervised-agent"),
        )
        .unwrap();
    let mut target = session("session-supervised", "supervised-agent");
    target.control_state.driver_ref = RuntimeDriverRef::TeamSupervisor {
        team_run_id: "run-supervisor".into(),
        team_supervisor_id: lease.supervisor_id.clone(),
        team_supervisor_generation: lease.generation,
    };
    store
        .create_agent_session(
            &service_context("session.create", "session-supervised", 0),
            target.clone(),
        )
        .unwrap();

    let (command, admission) = runtime_command_fixture(
        "supervisor-live-command",
        RuntimeCommandKind::OpenRuntime,
        &target,
        "open_runtime",
    );
    let accepted = store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
        .unwrap();
    assert_eq!(
        accepted.projection.request_fingerprint,
        runtime_command_envelope_fingerprint(&command).unwrap(),
        "RuntimeCommandRecord must snapshot the full command, not only payload"
    );
    store
        .release_team_supervisor_lease(
            "run-supervisor",
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms(),
        )
        .unwrap();
    let before_settle = store.canonical_operations().unwrap();
    let error = store
        .settle_runtime_command(
            &service_context(
                "node_daemon.runtime.settle",
                "supervisor-live-command:settle",
                accepted.projection.version,
            ),
            &command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            Some(serde_json::json!({"provider_receipt": "must-not-land"})),
            None,
            "t-settle",
        )
        .expect_err("a released Supervisor cannot settle Applied");
    assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_settle);

    let successor = store
        .acquire_team_supervisor_under_node_lease(
            "run-supervisor",
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "space-test",
            "project-1",
            "supervisor-2",
            std::process::id(),
            "loopback://supervisor-2",
            current_unix_ms(),
            60_000,
        )
        .unwrap();

    let mut stale = session("session-stale-supervisor", "supervised-agent");
    stale.id = "session-stale-supervisor".into();
    stale.agent_member_id = "another-supervised-agent".into();
    stale.control_state.driver_ref = RuntimeDriverRef::TeamSupervisor {
        team_run_id: "run-supervisor".into(),
        team_supervisor_id: successor.supervisor_id,
        team_supervisor_generation: successor.generation.saturating_add(1),
    };
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "another-supervised-agent", 0),
            identity("another-supervised-agent"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "session-stale-supervisor", 0),
            stale.clone(),
        )
        .unwrap();
    let (stale_command, stale_admission) = runtime_command_fixture(
        "supervisor-stale-command",
        RuntimeCommandKind::OpenRuntime,
        &stale,
        "open_runtime",
    );
    let before_prepare = store.canonical_operations().unwrap();
    let error = store
        .prepare_runtime_command(
            &stale_admission,
            &stale_command,
            current_unix_ms(),
            "t-stale",
        )
        .expect_err("a stale Supervisor generation must not reach Accepted");
    assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_prepare);
    fs::remove_dir_all(root).unwrap();
}
