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

    transition_provider_session_for_member(&ledger, &blocked, AgentSessionStatus::Active)
        .expect("seed active provider turn");
    let active_turn = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &blocked,
        &lease,
        "host",
        "active turn must fail closed",
    )
    .expect_err("an active provider turn must fence recovery Close");
    assert!(active_turn
        .to_string()
        .contains("not detached+idle at a terminal turn boundary"));
    transition_provider_session_for_member(&ledger, &blocked, AgentSessionStatus::Idle)
        .expect("return to terminal turn boundary");

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

    let mut probation_blocked = blocked.clone();
    probation_blocked.zero_output_streak = 2;
    probation_blocked.last_event_at = Some("unix-ms:recovery-probation".into());
    ledger
        .save_member_run(&blocked, &probation_blocked)
        .expect("seed a nonzero probation continuation streak");

    let recovered = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &probation_blocked,
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
    assert_eq!(
        closed.zero_output_streak, 0,
        "the consumed provider-received revision cannot probation-continue after Reopen"
    );
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

#[test]
fn detached_blocked_recovery_authority_takeover_never_persists_closed_blocked() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    use std::cell::Cell;

    let (store, root) = temp_store("detached-recovery-authority-takeover");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:race-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind native session");
    let first = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-recovery-race-first",
            std::process::id(),
            "test://recovery-race-first",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire first Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &first);
    let run = latest_team_run(&store, &created.team_run.id).expect("TeamRun");
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members: latest_member_runs_in_append_order(&store)
            .expect("members")
            .into_iter()
            .filter(|member| member.team_run_id == run.id)
            .collect(),
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &first.execution_space_id,
        &first.node_daemon_id,
        &first.supervisor_id,
        first.generation,
    )
    .expect("bind first Supervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &first.supervisor_id,
        first.generation,
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
    .expect("attach runtime");
    let mut blocked = bound.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:race-blocked".into());
    ledger
        .save_member_run(&bound, &blocked)
        .expect("block member");
    settle_provider_attempt_release(&ledger, &blocked).expect("detach runtime");

    let successor_generation = Cell::new(0_u64);
    let error = close_detached_blocked_member_for_recovery_with_hook(
        &store,
        &run.id,
        &blocked,
        &first,
        "host",
        "successor races terminal recovery CAS",
        |_| {
            store.release_team_supervisor_lease(
                &run.id,
                &first.supervisor_id,
                first.generation,
                current_unix_ms_u64(),
            )?;
            let successor = store.acquire_test_supervisor_lease(
                &run.id,
                "supervisor-recovery-race-successor",
                std::process::id(),
                "test://recovery-race-successor",
                current_unix_ms_u64(),
                60_000,
            )?;
            successor_generation.set(successor.generation);
            Ok(())
        },
    )
    .expect_err("the stale Supervisor must lose terminal recovery CAS");
    assert!(error.is_supervisor_lease_lost());
    assert!(successor_generation.get() > first.generation);
    let latest = latest_member_runs_in_append_order(&store)
        .expect("latest member")
        .into_iter()
        .find(|member| member.id == blocked.id)
        .expect("member row");
    assert!(latest.coordination_is_active());
    assert_eq!(latest.status, MemberRunStatus::Blocked);
    assert!(
        !(latest.coordination_is_closed() && latest.status == MemberRunStatus::Blocked),
        "authority loss must never strand Closed + Blocked"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
