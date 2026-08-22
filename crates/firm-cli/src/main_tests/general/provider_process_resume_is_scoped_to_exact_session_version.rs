use super::*;

#[test]
fn provider_process_resume_is_scoped_to_exact_session_version() {
    use harness_core::agentfirm_api::{RuntimeActivity, RuntimeResidency};

    let (store, root) = temp_store("provider-process-exact-session-version");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "provider-process-exact-session-version",
            std::process::id(),
            "test://provider-process-exact-session-version",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire canonical test Supervisor");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("read TeamRun");
    let members = latest_member_runs_in_append_order(&store)
        .expect("read MemberRuns")
        .into_iter()
        .filter(|member| member.team_run_id == run.id)
        .collect();
    bind_team_runtime_supervisor(
        &store,
        &PreparedTeamRunBody {
            run_id: run.id.clone(),
            objective: run.objective.clone(),
            run,
            members,
        },
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind TeamSupervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let initial = ledger
        .latest_member_run(&created.member_runs[0].id)
        .expect("read member")
        .expect("member exists");
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some(now_string());
    ledger
        .save_member_run(&initial, &bound)
        .expect("bind provider-native session");

    let first = prepare_provider_process_effect(&ledger, &bound)
        .expect("prepare first exact-session resume");
    let first_session_version = first.target_session.version;
    let duplicate = prepare_provider_process_effect(&ledger, &bound)
        .expect_err("same exact precondition must not prepare a second effect");
    assert!(
        duplicate
            .to_string()
            .contains("RUNTIME_COMMAND_RECOVERY_REQUIRED"),
        "same-version replay is fenced explicitly: {duplicate}"
    );
    assert_eq!(
        store
            .runtime_commands(&lease.execution_space_id)
            .expect("RuntimeCommands after duplicate")
            .len(),
        1,
        "same exact AgentSession version has one durable effect"
    );

    settle_provider_effect(
        &ledger,
        &first,
        true,
        Some(serde_json::json!({"phase":"runtime_attached"})),
        None,
    )
    .expect("settle first attachment");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("record attached control state");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("record later transport detach");

    let second = prepare_provider_process_effect(&ledger, &bound)
        .expect("new session projection may prepare a new resume");
    assert!(second.target_session.version > first_session_version);
    assert_ne!(second.command_id, first.command_id);
    let commands = store
        .runtime_commands(&lease.execution_space_id)
        .expect("RuntimeCommands after reattach preparation");
    assert_eq!(commands.len(), 2);
    assert!(commands
        .iter()
        .any(|command| command.id == first.command_id));
    assert!(commands
        .iter()
        .any(|command| command.id == second.command_id));

    std::fs::remove_dir_all(root).expect("cleanup");
}
