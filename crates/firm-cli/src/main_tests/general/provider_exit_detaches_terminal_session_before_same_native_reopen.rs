use super::*;

#[cfg(unix)]
#[test]
fn provider_exit_detaches_terminal_session_before_same_native_reopen() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};

    let (store, root) = temp_store("provider-exit-detaches-before-reopen");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:provider-exit-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind exact provider-native session");

    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-provider-exit-reopen",
            std::process::id(),
            "test://provider-exit-reopen",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("read TeamRun");
    let members = latest_member_runs_in_append_order(&store)
        .expect("read members")
        .into_iter()
        .filter(|member| member.team_run_id == run.id)
        .collect();
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run,
        members,
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind exact Supervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
        .expect("make AgentSession idle");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("record attached provider process");

    let mut terminal = bound.clone();
    terminal.status = MemberRunStatus::Failed;
    terminal.finished_at = Some("unix-ms:provider-exit-terminal".into());
    terminal.last_event_at = Some("unix-ms:provider-exit-terminal".into());
    ledger
        .save_member_run(&bound, &terminal)
        .expect("record terminal provider outcome");
    settle_provider_attempt_release(&ledger, &terminal)
        .expect("provider-process exit must durably detach the AgentSession");

    let released = store
        .fabric_agent_sessions(&lease.execution_space_id)
        .expect("read released AgentSession")
        .into_iter()
        .find(|session| session.agent_member_id == terminal.agent_member_id)
        .expect("exact AgentSession");
    let native_session = released
        .native_session_ref
        .clone()
        .expect("native session remains bound");
    assert_eq!(released.lifecycle, AgentSessionStatus::Idle);
    assert_eq!(
        released.control_state.runtime_residency,
        RuntimeResidency::Detached
    );
    assert_eq!(released.control_state.activity, RuntimeActivity::Idle);
    assert!(released.current_turn_id.is_none());

    let mut closed = terminal.clone();
    closed.coordination_status = MemberCoordinationStatus::Closed;
    closed.status = MemberRunStatus::Stopped;
    closed.last_event_at = Some("unix-ms:provider-exit-closed".into());
    store
        .compare_and_append_member_run(&terminal, &closed)
        .expect("close terminal member coordination");
    let mut reopened = closed.clone();
    reopened.runtime_generation += 1;
    reopened.coordination_status = MemberCoordinationStatus::Active;
    reopened.status = MemberRunStatus::Idle;
    reopened.finished_at = None;
    reopened.last_event_at = Some("unix-ms:provider-exit-reopened".into());
    store
        .compare_and_advance_member_run_generation(&closed, &reopened)
        .expect("advance the MemberRun runtime generation");

    let run = latest_team_run(&store, &created.team_run.id).expect("read reopened TeamRun");
    let members = latest_member_runs_in_append_order(&store)
        .expect("read reopened members")
        .into_iter()
        .filter(|member| member.team_run_id == run.id)
        .collect();
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run,
        members,
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("rebind reopened member only after detached/idle proof");
    transition_provider_session_runtime_control(
        &ledger,
        &reopened,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("attach the same native session for Reopen");
    let settled = managed_member_runtime_reopen_is_settled(&store, &reopened)
        .expect("read Reopen postcondition")
        .expect("exact same-session Reopen must settle");
    assert_eq!(settled.runtime_generation, 2);
    let resumed = store
        .fabric_agent_sessions(&lease.execution_space_id)
        .expect("read resumed AgentSession")
        .into_iter()
        .find(|session| session.id == released.id)
        .expect("same AgentSession id");
    assert_eq!(resumed.native_session_ref, Some(native_session));
    assert_eq!(
        resumed.control_state.runtime_residency,
        RuntimeResidency::Attached
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
