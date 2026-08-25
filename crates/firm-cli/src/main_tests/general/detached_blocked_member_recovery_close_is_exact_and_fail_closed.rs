use super::*;

#[test]
fn detached_blocked_member_recovery_close_is_exact_and_fail_closed() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};

    let (store, root) = temp_store("detached-blocked-member-recovery-close");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:recovery-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind native session");

    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-detached-recovery",
            std::process::id(),
            "test://detached-recovery",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("TeamRun");
    let members = latest_member_runs_in_append_order(&store)
        .expect("members")
        .into_iter()
        .filter(|member| member.team_run_id == run.id)
        .collect();
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
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
    .expect("bind Supervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
        .expect("idle session");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("attach provider runtime");
    let mut blocked = bound.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:recovery-blocked".into());
    ledger
        .save_member_run(&bound, &blocked)
        .expect("block member after provider failure");

    assert!(
        close_detached_blocked_member_for_recovery(
            &store,
            &run.id,
            &blocked,
            &lease,
            "host",
            "must not bypass a live handle",
        )
        .expect("attached runtime check")
        .is_none(),
        "an attached runtime must stay on the normal provider Close path"
    );

    settle_provider_attempt_release(&ledger, &blocked).expect("detach provider runtime");
    let admission = prepare_provider_process_effect(&ledger, &blocked, 2)
        .expect("prepare one intentionally ambiguous resume command");
    let ambiguous = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &blocked,
        &lease,
        "host",
        "ambiguous effects must fail closed",
    )
    .expect_err("ambiguous RuntimeCommand must fence recovery Close");
    assert!(ambiguous.to_string().contains("ambiguous RuntimeCommand"));
    settle_provider_effect_not_applied(
        &ledger,
        &admission,
        "deterministic negative receipt".to_string(),
    )
    .expect("settle ambiguous command as not applied");

    let mut stale_generation = blocked.clone();
    stale_generation.runtime_generation += 1;
    let stale = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &stale_generation,
        &lease,
        "host",
        "stale generation",
    )
    .expect_err("stale MemberRun generation must fail closed");
    assert!(stale.to_string().contains("MEMBER_RUN_SCOPE_MISMATCH"));

    let recovered = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &blocked,
        &lease,
        "host",
        "explicit detached recovery",
    )
    .expect("recovery Close")
    .expect("detached recovery result");
    assert_eq!(recovered["runtime_effect"], "already_detached");
    assert_eq!(recovered["provider_close_receipt"], "not_fabricated");
    let closed = latest_member_runs_in_append_order(&store)
        .expect("closed member rows")
        .into_iter()
        .find(|member| member.id == blocked.id)
        .expect("closed member");
    assert_eq!(closed.coordination_status, MemberCoordinationStatus::Closed);
    assert_eq!(closed.status, MemberRunStatus::Stopped);
    assert!(store
        .runtime_commands(&lease.execution_space_id)
        .expect("RuntimeCommands")
        .into_iter()
        .all(|command| {
            command.command != harness_core::agentfirm_api::RuntimeCommandKind::CloseMember
        }));

    let reopened = reopen_team_member_value(
        &store,
        &run.id,
        &blocked.id,
        &serde_json::json!({
            "reopened_by": "host",
            "reason": "same-session recovery"
        }),
    )
    .expect("Reopen exact same native session");
    assert_eq!(reopened["member_run"]["runtime_generation"], 2);
    assert_eq!(
        reopened["member_run"]["native_session"]["native_session_id"],
        blocked
            .native_session
            .as_ref()
            .expect("bound native session")
            .native_session_id
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
