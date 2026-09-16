use super::*;

/// ADR 0078. An idle member polls forever: 500ms doubling to a 30s ceiling,
/// then 30s apart until something happens. Writing the decision row per poll
/// would cost ~120 rows an hour per idle member for a fact that never changes.
///
/// The bargain is two rows per idle episode: one when the backoff first reaches
/// its ceiling (past that, every poll is identical), and one when a wake ends
/// the episode (written by `record_wake_decision`, which carries the poll
/// count). This proves the first half — the cap row is written once no matter
/// how long the episode runs, and re-arms for the next episode.
///
/// This drives the real `poll_idle_member_wake`, the same seam
/// `running_idle_scan_cost` uses. Only the backoff is aged by hand: reaching
/// the 30s ceiling honestly would take a minute of wall clock per episode.
#[test]
fn idle_episode_records_its_cap_once_not_once_per_poll() {
    let (store, root) = temp_store("idle-episode-cap-row");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "idle-episode-supervisor",
            std::process::id(),
            "test://idle-episode",
            current_unix_ms_u64(),
            600_000,
        )
        .unwrap();
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).unwrap();
    bind_team_runtime_supervisor(
        &store,
        &PreparedTeamRunBody {
            run_id: run.id.clone(),
            objective: run.objective.clone(),
            run: run.clone(),
            members: created.member_runs.clone(),
        },
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .unwrap();
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let mut running = run.clone();
    running.status = TeamRunStatus::Running;
    store
        .compare_and_append_team_run_lifecycle(&run, &running)
        .unwrap();
    let mut member = created.member_runs[0].clone();
    transition_provider_session_runtime_control(
        &ledger,
        &member,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        harness_core::agentfirm_api::RuntimeActivity::Idle,
    )
    .unwrap();
    let before = ledger.latest_member_run(&member.id).unwrap().unwrap();
    member = before.clone();
    member.status = MemberRunStatus::Idle;
    store
        .compare_and_append_member_run(&before, &member)
        .unwrap();

    let (_sender, controls) = std::sync::mpsc::channel();
    let policy = supervisor_wake::WakePolicy::default();
    let mut backoff = supervisor_wake::WakeBackoff::new();
    let cap_rows = |store: &HarnessStore| -> Vec<String> {
        store
            .current_team_run_events(&run.id)
            .unwrap()
            .into_iter()
            .filter(|event| event.operation == "wake_idle_capped")
            .map(|event| event.summary)
            .collect()
    };
    let poll = |backoff: &mut supervisor_wake::WakeBackoff,
                member: &mut ProviderRuntimeProjection| {
        assert!(matches!(
            poll_idle_member_wake(
                &ledger,
                member,
                &controls,
                &mut || Ok(()),
                0,
                None,
                &policy,
                backoff,
            )
            .unwrap(),
            IdleWakeStep::Retry
        ));
    };

    // An episode that has not reached the ceiling writes nothing at all: the
    // early, cheap polls are exactly the ones worth staying silent about.
    for _ in 0..5 {
        poll(&mut backoff, &mut member);
        backoff.tick();
    }
    assert!(
        !backoff.at_cap(&policy),
        "premise: still growing after 5 sleeps"
    );
    assert_eq!(cap_rows(&store), Vec::<String>::new());

    // The 6th tick reaches the 30s ceiling. One row, then silence however long
    // the member stays idle.
    backoff.tick();
    assert!(backoff.at_cap(&policy));
    for _ in 0..40 {
        poll(&mut backoff, &mut member);
        backoff.tick();
    }
    let rows = cap_rows(&store);
    assert_eq!(
        rows.len(),
        1,
        "41 capped polls must leave ONE row, not 41: {rows:?}"
    );
    assert!(
        rows[0].contains("30000ms") && rows[0].contains("after 6 polls"),
        "the row must say which ceiling and when it was reached: {rows:?}"
    );

    // A wake ends the episode. The next idle stretch is a NEW episode and
    // records its own ceiling — the latch must not silence it forever.
    backoff.reset();
    assert!(!backoff.cap_recorded());
    // Seven, not six: the cap is observed at the TOP of a poll, so the poll
    // that sees the 6th sleep is the 7th.
    for _ in 0..7 {
        poll(&mut backoff, &mut member);
        backoff.tick();
    }
    let rows = cap_rows(&store);
    assert_eq!(
        rows.len(),
        2,
        "the second episode records its own cap: {rows:?}"
    );

    std::fs::remove_dir_all(root).unwrap();
}
