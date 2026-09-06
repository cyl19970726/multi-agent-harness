use super::*;

#[test]
fn running_idle_poll_does_not_rescan_member_history_for_member_or_managed_host() {
    let (store, root) = temp_store("running-idle-scan-cost");
    let created = create_two_member_team_run(&store);
    assert!(store.latest_works().unwrap().is_empty());
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "idle-cost-supervisor",
            std::process::id(),
            "test://idle-cost",
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
    let mut members = vec![
        created.member_runs[0].clone(),
        created
            .member_runs
            .iter()
            .find(|m| m.role == "host")
            .unwrap()
            .clone(),
    ];
    for member in &mut members {
        transition_provider_session_runtime_control(
            &ledger,
            member,
            harness_core::agentfirm_api::RuntimeResidency::Attached,
            harness_core::agentfirm_api::RuntimeActivity::Idle,
        )
        .unwrap();
        let before = ledger.latest_member_run(&member.id).unwrap().unwrap();
        *member = before.clone();
        member.status = MemberRunStatus::Idle;
        store
            .compare_and_append_member_run(&before, member)
            .unwrap();
    }
    let (_sender, controls) = std::sync::mpsc::channel();
    let policy = supervisor_wake::WakePolicy::default();
    let mut backoff = supervisor_wake::WakeBackoff::new();
    let mut observed_revision_count = 0;
    let mut previous_clone_cost = None;
    for revisions in [10, 100] {
        for index in observed_revision_count..revisions {
            let before = members[0].clone();
            members[0].last_event_at = Some(format!("unix-ms:idle-history-{index}"));
            store
                .compare_and_append_member_run(&before, &members[0])
                .unwrap();
        }
        observed_revision_count = revisions;
        // Exercise the real production poll, replacing only transport liveness
        // with a deterministic receipt. This is not a live provider canary.
        let post_write_before = store.read_scan_metrics();
        let post_write_started = Instant::now();
        for member in &mut members {
            assert!(matches!(
                poll_idle_member_wake(
                    &ledger,
                    member,
                    &controls,
                    &mut || Ok(()),
                    0,
                    None,
                    &policy,
                    &mut backoff
                )
                .unwrap(),
                IdleWakeStep::Retry
            ));
        }
        let before = store.read_scan_metrics();
        eprintln!(
            "{}",
            serde_json::json!({"real_member_revisions":revisions,"phase":"first_post_write_observation","elapsed_us":post_write_started.elapsed().as_micros(),"read_bytes":before.iter().map(|m| m.total_bytes_read).sum::<u64>() - post_write_before.iter().map(|m| m.total_bytes_read).sum::<u64>(),"decoded_rows":before.iter().map(|m| m.total_decoded_rows).sum::<u64>() - post_write_before.iter().map(|m| m.total_decoded_rows).sum::<u64>()})
        );
        let started = Instant::now();
        for _ in 0..10 {
            for member in &mut members {
                assert!(matches!(
                    poll_idle_member_wake(
                        &ledger,
                        member,
                        &controls,
                        &mut || Ok(()),
                        0,
                        None,
                        &policy,
                        &mut backoff
                    )
                    .unwrap(),
                    IdleWakeStep::Retry
                ));
            }
        }
        let after = store.read_scan_metrics();
        for ledger_name in ["member_runs.jsonl", "agentfirm_trust_operations.jsonl"] {
            let a = after.iter().find(|m| m.ledger == ledger_name).unwrap();
            let b = before.iter().find(|m| m.ledger == ledger_name).unwrap();
            assert_eq!(
                a.total_bytes_read, b.total_bytes_read,
                "{ledger_name} N={revisions}"
            );
            assert_eq!(
                a.total_decoded_rows, b.total_decoded_rows,
                "{ledger_name} N={revisions}"
            );
        }
        let clone_cost = after.iter().map(|m| m.total_cloned_rows).sum::<u64>()
            - before.iter().map(|m| m.total_cloned_rows).sum::<u64>();
        if let Some(previous) = previous_clone_cost {
            assert_eq!(clone_cost, previous, "same current members, longer history");
        }
        previous_clone_cost = Some(clone_cost);
        eprintln!(
            "{}",
            serde_json::json!({"real_member_revisions":revisions,"production_idle_polls":20,"elapsed_us":started.elapsed().as_micros(),"selected_current_rows_cloned":clone_cost,"metrics":after})
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn outer_drive_fresh_lease_checks_stay_bounded_across_same_generation_heartbeats() {
    let (store, root) = temp_store("outer-drive-lease-cost");
    let created = create_two_member_team_run(&store);
    let run_id = &created.team_run.id;
    let lease = store
        .acquire_test_supervisor_lease(
            run_id,
            "outer",
            std::process::id(),
            "test://outer",
            current_unix_ms_u64(),
            600_000,
        )
        .unwrap();
    let ledger = TeamRunLedger::new(
        &store,
        run_id,
        "outer",
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    // A separate Store is a real writer, rather than publishing through a
    // process-local cache. The outer drive invokes this exact fresh predicate.
    let writer = HarnessStore::new(&root);
    let mut previous = 0;
    let mut cost = None;
    for heartbeats in [10, 100] {
        for _ in previous..heartbeats {
            writer
                .renew_team_supervisor_lease(
                    run_id,
                    "outer",
                    lease.generation,
                    current_unix_ms_u64(),
                    600_000,
                )
                .unwrap();
        }
        previous = heartbeats;
        let before = store.read_scan_metrics();
        let started = Instant::now();
        for _ in 0..20 {
            ledger.require_supervisor_lease().unwrap();
        }
        let after = store.read_scan_metrics();
        let a = after
            .iter()
            .find(|m| m.ledger == "team_supervisor_leases.jsonl")
            .unwrap();
        let b = before
            .iter()
            .find(|m| m.ledger == "team_supervisor_leases.jsonl");
        let bytes = a.total_bytes_read - b.map_or(0, |m| m.total_bytes_read);
        let rows = a.total_decoded_rows - b.map_or(0, |m| m.total_decoded_rows);
        assert_eq!(
            rows, 40,
            "one run retains only two rows for every fresh read"
        );
        if let Some(old) = cost {
            assert_eq!(bytes, old);
        }
        cost = Some(bytes);
        eprintln!(
            "{}",
            serde_json::json!({"same_generation_heartbeats":heartbeats,
            "outer_drive_lease_checks":20,"read_bytes":bytes,"decoded_rows":rows,
            "elapsed_us":started.elapsed().as_micros()})
        );
    }
    writer
        .release_team_supervisor_lease(run_id, "outer", lease.generation, current_unix_ms_u64())
        .unwrap();
    assert!(
        ledger.require_supervisor_lease().is_err(),
        "foreign Store release is observed immediately"
    );
    let next = writer
        .acquire_test_supervisor_lease(
            run_id,
            "replacement",
            std::process::id(),
            "test://replacement",
            current_unix_ms_u64(),
            600_000,
        )
        .unwrap();
    assert_eq!(next.generation, lease.generation + 1);
    // A fresh local validity flag cannot conceal the new durable owner.
    let stale = TeamRunLedger::new(
        &store,
        run_id,
        "outer",
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    assert!(stale.require_supervisor_lease().is_err());
    writer
        .release_team_supervisor_lease(
            run_id,
            "replacement",
            next.generation,
            current_unix_ms_u64(),
        )
        .unwrap();
    let expiring = writer
        .acquire_test_supervisor_lease(
            run_id,
            "expiry",
            std::process::id(),
            "test://expiry",
            current_unix_ms_u64(),
            1,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(3));
    let expired = TeamRunLedger::new(
        &store,
        run_id,
        "expiry",
        expiring.generation,
        Arc::new(AtomicBool::new(true)),
    );
    assert!(
        expired.require_supervisor_lease().is_err(),
        "fresh owner cannot outlive durable expiry"
    );
    std::fs::remove_dir_all(root).unwrap();
}
