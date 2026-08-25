use super::*;

#[test]
fn detached_blocked_member_recovery_close_is_exact_and_fail_closed() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    use harness_core::CurrentWorkDraft;

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
    let make_received_work = |id: &str, at: &str| {
        let mut draft = CurrentWorkDraft::new(
            id.into(),
            created.team_run.id.clone(),
            created.team_run.agent_team_id.clone(),
            format!("stale recovery Work {id}"),
            "Exercise explicit Host reconciliation before detached recovery".into(),
            "Host cancellation preserves provider receipt evidence".into(),
            WorkClaimMode::HostAssign,
            WorkPriority::Normal,
            compatibility_team_actor("host", "test"),
            at.into(),
        );
        draft.owner_member_id = Some(bound.agent_member_id.clone());
        draft.active_member_run_id = Some(bound.id.clone());
        draft.eligible_member_ids = vec![bound.agent_member_id.clone()];
        let work = store
            .insert_work(
                draft.into_work(),
                WorkCommandContext {
                    event_id: format!("{id}-created"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-create"),
                    created_at: at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("create recovery Work");
        let claimed = claim_canonical_work_for_member(&ledger, &bound)
            .expect("claim recovery Work")
            .expect("one recovery Work claim");
        assert_eq!(claimed.work.id, work.id);
        ledger
            .complete_work_delivery(&claimed, &format!("receipt-{id}"))
            .expect("record provider receipt");
        work
    };
    let stale_work_a = make_received_work("recovery-stale-a", "unix-ms:recovery-work-a");
    let stale_work_b = make_received_work("recovery-stale-b", "unix-ms:recovery-work-b");
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

    let multiple = close_detached_blocked_member_for_recovery(
        &store,
        &run.id,
        &probation_blocked,
        &lease,
        "host",
        "multiple received Works require Host reconciliation",
    )
    .expect_err("multiple provider-received active Work revisions must fence recovery Close");
    assert!(multiple
        .to_string()
        .contains("multiple provider-received active Work revisions"));
    let deliveries_before_cancel = store
        .fabric_work_deliveries(&lease.execution_space_id)
        .expect("provider-received evidence before Host reconciliation");
    let bindings_before_cancel = store
        .fabric_work_execution_bindings(&lease.execution_space_id)
        .expect("execution bindings before Host reconciliation");
    let commands_before_cancel = store
        .runtime_commands(&lease.execution_space_id)
        .expect("RuntimeCommands before Host reconciliation");
    for (index, work) in [stale_work_a, stale_work_b].into_iter().enumerate() {
        store
            .cancel_work(
                &work.id,
                work.version,
                "obsolete after detached provider recovery",
                WorkCommandContext {
                    event_id: format!("recovery-stale-cancel-{index}"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("recovery-stale-cancel-{index}"),
                    created_at: format!("unix-ms:recovery-cancel-{index}"),
                    duplicate_ok: false,
                },
            )
            .expect("Host reconciles one provider-received Work");
    }
    assert_eq!(
        store
            .fabric_work_deliveries(&lease.execution_space_id)
            .expect("provider receipts after Host reconciliation"),
        deliveries_before_cancel,
        "Host reconciliation must preserve delivery evidence"
    );
    assert_eq!(
        store
            .fabric_work_execution_bindings(&lease.execution_space_id)
            .expect("bindings after Host reconciliation"),
        bindings_before_cancel,
        "Host reconciliation must not fabricate binding release"
    );
    assert_eq!(
        store
            .runtime_commands(&lease.execution_space_id)
            .expect("RuntimeCommands after Host reconciliation"),
        commands_before_cancel,
        "Host reconciliation must not issue a provider effect"
    );

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

    assert_eq!(
        closed.native_session, blocked.native_session,
        "recovery Close must preserve the exact native session for the real provider-backed Reopen path"
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

#[test]
fn detached_blocked_recovery_samples_lease_time_after_writer_lock_wait() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    use std::sync::mpsc;

    let (store, root) = temp_store("detached-recovery-lock-expiry");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:expiry-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind native session");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-recovery-expiry",
            std::process::id(),
            "test://recovery-expiry",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
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
    .expect("attach runtime");
    let mut blocked = bound.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:expiry-blocked".into());
    ledger
        .save_member_run(&bound, &blocked)
        .expect("block member");
    settle_provider_attempt_release(&ledger, &blocked).expect("detach runtime");

    let near_expiry = store
        .renew_team_supervisor_lease(
            &run.id,
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms_u64(),
            100,
        )
        .expect("renew near-expiry Supervisor lease");
    let member_rows_before = store.member_runs().expect("member rows before").len();
    let guard = store
        .acquire_exclusive_migration_guard()
        .expect("hold Store writer lock across lease expiry");
    let worker_store = store.clone();
    let worker_run_id = run.id.clone();
    let worker_member = blocked.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).expect("signal recovery start");
        close_detached_blocked_member_for_recovery(
            &worker_store,
            &worker_run_id,
            &worker_member,
            &near_expiry,
            "host",
            "writer contention crosses lease expiry",
        )
    });
    started_rx.recv().expect("recovery thread started");
    std::thread::sleep(Duration::from_millis(250));
    drop(guard);

    let error = worker
        .join()
        .expect("recovery thread")
        .expect_err("expired authority must fail after writer-lock wait");
    assert!(
        error.is_supervisor_lease_lost(),
        "unexpected error: {error}"
    );
    assert_eq!(
        store.member_runs().expect("member rows after").len(),
        member_rows_before,
        "expired authority must append no MemberRun revision"
    );
    assert!(
        store
            .team_member_close_requests()
            .expect("Close requests")
            .is_empty(),
        "expired authority must not even latch Close intent"
    );
    let latest = latest_member_runs_in_append_order(&store)
        .expect("latest member")
        .into_iter()
        .find(|member| member.id == blocked.id)
        .expect("blocked member");
    assert!(latest.coordination_is_active());
    assert_eq!(latest.status, MemberRunStatus::Blocked);

    std::fs::remove_dir_all(root).expect("cleanup");
}
