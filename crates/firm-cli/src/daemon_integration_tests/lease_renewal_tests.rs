use super::adoption_tests::{adoption_fixture, AdoptionFixture};
use super::*;
use std::sync::atomic::AtomicUsize;

fn enrolled_fixture(label: &str) -> AdoptionFixture {
    let mut fixture = adoption_fixture(label);
    // Product defaults: five-second scan, Node TTL = max(scan * 4, 15s).
    fixture.daemon.set_scan_interval(Duration::from_secs(5));
    fixture.daemon.set_lease_ttl_override(None);
    fixture.daemon.set_node_identity(
        crate::daemon_support::latest_team_run(&fixture.store, &fixture.run_id)
            .unwrap()
            .execution_node_id,
    );
    let daemon = &fixture.daemon;
    if !fixture
        .store
        .latest_execution_nodes()
        .unwrap()
        .iter()
        .any(|node| node.id == daemon.node_id())
    {
        fixture
            .store
            .insert_execution_node(&harness_core::ExecutionNode {
                id: daemon.node_id().to_string(),
                display_name: "renewal test".into(),
                status: harness_core::ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })
            .unwrap();
    }
    fixture
        .store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: daemon.node_id().to_string(),
                execution_space_id: fixture.execution_space_id.clone(),
                project_binding_id: "unit-test-project".into(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            &fixture.execution_space_id,
        )
        .unwrap();
    let space = daemon
        .registered_spaces()
        .unwrap()
        .into_iter()
        .find(|(space, _)| space.id == fixture.execution_space_id)
        .unwrap()
        .0;
    daemon
        .ensure_node_authority(&space, &fixture.store)
        .unwrap();
    fixture
}

#[test]
fn default_leases_survive_ten_second_writer_contention_without_extending_ttl() {
    let fixture = enrolled_fixture("renewal-ten-second-lock");
    let daemon = &fixture.daemon;
    let store = &fixture.store;
    let node = store
        .latest_node_daemon_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    let supervisor = store
        .acquire_team_supervisor_under_node_lease(
            &fixture.run_id,
            daemon.node_id(),
            daemon.daemon_id(),
            node.generation,
            &fixture.execution_space_id,
            "unit-test-project",
            "contended-supervisor",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms_u64(),
            15_000,
        )
        .unwrap();
    let policy = crate::SupervisorHeartbeatPolicy {
        team_run_id: fixture.run_id.clone(),
        supervisor_id: supervisor.supervisor_id.clone(),
        generation: supervisor.generation,
        ttl_ms: 15_000,
        heartbeat_interval_ms: 1_000,
        initial_expires_unix_ms: supervisor.expires_unix_ms,
    };
    let stop = AtomicBool::new(false);
    let valid = AtomicBool::new(true);
    let gate = Mutex::new(());
    // Repeated valid historical projections make this a bulk same-Space read,
    // not just a lock-only canary. These are throwaway fixture records.
    let member_path = store.root().join("member_runs.jsonl");
    let member_rows = std::fs::read(&member_path).unwrap();
    std::fs::write(&member_path, member_rows.repeat(256)).unwrap();
    let read_count = AtomicUsize::new(0);
    let lock = store.acquire_exclusive_migration_guard().unwrap();
    std::thread::scope(|scope| {
        let bulk_reader = scope.spawn(|| {
            let started = Instant::now();
            while !stop.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(12) {
                assert!(store.member_runs().unwrap().len() >= 256);
                read_count.fetch_add(1, Ordering::Release);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let node_heartbeat = scope.spawn(|| -> CliResult<()> {
            let started = Instant::now();
            loop {
                daemon.refresh_held_node_authorities()?;
                if store
                    .latest_node_daemon_lease(daemon.node_id())?
                    .unwrap()
                    .renewed_unix_ms
                    > node.renewed_unix_ms
                {
                    return Ok(());
                }
                assert!(
                    started.elapsed() < Duration::from_secs(15),
                    "Node renewal must recover within its unchanged default TTL"
                );
                std::thread::sleep(daemon.next_node_authority_refresh_delay());
            }
        });
        let supervisor_heartbeat = scope.spawn(|| {
            crate::run_supervisor_heartbeat_loop(
                &policy,
                &stop,
                &valid,
                &gate,
                || {
                    let renewed = store.renew_team_supervisor_lease(
                        &fixture.run_id,
                        &supervisor.supervisor_id,
                        supervisor.generation,
                        current_unix_ms_u64(),
                        15_000,
                    )?;
                    stop.store(true, Ordering::Release);
                    Ok(renewed.expires_unix_ms)
                },
                || None,
            )
        });
        std::thread::sleep(Duration::from_secs(10));
        assert!(valid.load(Ordering::Acquire));
        assert!(!daemon.authority_lost());
        drop(lock);
        node_heartbeat.join().unwrap().unwrap();
        supervisor_heartbeat.join().unwrap();
        bulk_reader.join().unwrap();
    });
    assert!(read_count.load(Ordering::Acquire) > 3);
    assert!(valid.load(Ordering::Acquire));
    let renewed = store
        .latest_team_supervisor_lease(&fixture.run_id)
        .unwrap()
        .unwrap();
    assert!(renewed.heartbeat_unix_ms > supervisor.heartbeat_unix_ms + 9_000);
    assert_eq!(renewed.expires_unix_ms - renewed.heartbeat_unix_ms, 15_000);
    let node = store
        .latest_node_daemon_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    assert_eq!(node.expires_unix_ms - node.renewed_unix_ms, 20_000);
    let diagnostics = crate::lease_renewal_diagnostics::snapshot();
    let supervisor_diagnostic = diagnostics
        .iter()
        .find(|v| v["id"] == fixture.run_id)
        .unwrap();
    assert!(supervisor_diagnostic["failure_count"].as_u64().unwrap() > 3);
    assert!(supervisor_diagnostic["last_error"]
        .as_str()
        .unwrap()
        .contains("lock_wait_ms="));
}

#[test]
fn supervisor_transient_failure_stops_at_confirmed_expiry() {
    let expires = current_unix_ms_u64() + 80;
    let policy = crate::SupervisorHeartbeatPolicy {
        team_run_id: "expiry-test".into(),
        supervisor_id: "expiry-supervisor".into(),
        generation: 1,
        ttl_ms: 15_000,
        heartbeat_interval_ms: 5,
        initial_expires_unix_ms: expires,
    };
    let stop = AtomicBool::new(false);
    let valid = AtomicBool::new(true);
    let gate = Mutex::new(());
    let mut attempts = 0;
    crate::run_supervisor_heartbeat_loop(
        &policy,
        &stop,
        &valid,
        &gate,
        || {
            attempts += 1;
            Err(harness_store::StoreError::Io(std::io::Error::other(
                "temporary",
            )))
        },
        || None,
    );
    assert!(!valid.load(Ordering::Acquire));
    assert!(current_unix_ms_u64() >= expires);
    assert!(
        attempts > 3,
        "the old attempt limit must not decide authority"
    );
}

#[test]
fn completed_run_supervisor_loss_does_not_latch_machine_authority() {
    let fixture = enrolled_fixture("completed-run-lease-loss");
    let mut run = crate::daemon_support::latest_team_run(&fixture.store, &fixture.run_id).unwrap();
    let expected = run.clone();
    run.status = harness_core::TeamRunStatus::Completed;
    fixture
        .store
        .compare_and_append_team_run_lifecycle(&expected, &run)
        .unwrap();
    fixture
        .daemon
        .push_context(OwnedTestContext::new(TestContextConfig {
            execution_space_id: fixture.execution_space_id.clone(),
            project_binding_id: "unit-test-project".into(),
            run_id: fixture.run_id.clone(),
            daemon_generation: 1,
            supervisor_id: "expired-child".into(),
            supervisor_generation: 1,
            heartbeat_valid: Arc::new(AtomicBool::new(false)),
            serving_status: Arc::new(Mutex::new("authority_lost".into())),
            thread: Some(std::thread::spawn(|| {
                Err(crate::supervisor_lease_lost_error("completed-run"))
            })),
            started_at: Instant::now(),
        }));
    while !fixture.daemon.context_thread_finished(0) {
        std::thread::yield_now();
    }
    fixture.daemon.reap_finished().unwrap();
    assert!(!fixture.daemon.authority_lost());
    assert!(!fixture.daemon.stop_requested_flag().load(Ordering::Acquire));
    fixture.daemon.refresh_held_node_authorities().unwrap();
    assert!(fixture.daemon.context_count() == 0);
    let member = fixture
        .store
        .member_runs()
        .unwrap()
        .into_iter()
        .find(|member| member.team_run_id == fixture.run_id && member.role != "host")
        .unwrap();
    let close = crate::dispatch_local_live_member_control(
        &fixture.store,
        "expired-child",
        1,
        &AtomicBool::new(false),
        &Mutex::new(()),
        crate::LiveMemberControlRequest::Close {
            team_run_id: fixture.run_id.clone(),
            member_run_id: member.id,
            reason: "completed does not authorize stale Close".into(),
            requested_by: "test".into(),
        },
    )
    .expect_err("the expired child cannot Close its still-open member");
    assert!(close.is_supervisor_lease_lost());
    assert!(fixture
        .store
        .team_member_close_requests()
        .unwrap()
        .is_empty());
}

#[test]
fn contended_space_does_not_delay_healthy_space_to_expiry() {
    let mut fixture = enrolled_fixture("multi-space-renewal-round");
    // A still owns its original long lease. Renewed leases use a short test
    // TTL so a full-TTL joined round would expire healthy B deterministically.
    fixture.daemon.set_scan_interval(Duration::from_secs(1));
    fixture.daemon.set_lease_ttl_override(Some(1_200));
    let daemon = &fixture.daemon;
    let space = crate::execution_space::register_and_activate(
        daemon.firm_home(),
        "healthy-space",
        "Healthy Space",
        Some("healthy-project".into()),
        None,
        "unix-ms:1",
    )
    .unwrap();
    let healthy = HarnessStore::new(space.store_root.clone());
    healthy
        .insert_execution_node(&harness_core::ExecutionNode {
            id: daemon.node_id().to_string(),
            display_name: "healthy test node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .unwrap();
    healthy
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: daemon.node_id().to_string(),
                execution_space_id: space.id.clone(),
                project_binding_id: "healthy-project".into(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            &space.id,
        )
        .unwrap();
    daemon.ensure_node_authority(&space, &healthy).unwrap();
    let lock = fixture.store.acquire_exclusive_migration_guard().unwrap();
    std::thread::scope(|scope| {
        let holder = scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(1_000));
            drop(lock);
        });
        for _ in 0..4 {
            daemon
                .refresh_held_node_authorities()
                .expect("healthy B cannot be starved by A's retry");
            std::thread::sleep(daemon.next_node_authority_refresh_delay());
        }
        holder.join().unwrap();
    });
    assert!(!daemon.authority_lost());
    for store in [&fixture.store, &healthy] {
        assert!(
            store
                .latest_node_daemon_lease(daemon.node_id())
                .unwrap()
                .unwrap()
                .expires_unix_ms
                > current_unix_ms_u64()
        );
    }
}

#[test]
fn authority_shutdown_interrupts_contended_renewal_without_waiting_for_ttl() {
    let fixture = enrolled_fixture("renewal-shutdown-bound");
    let daemon = &fixture.daemon;
    let lock = fixture.store.acquire_exclusive_migration_guard().unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let holder = scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            drop(lock);
        });
        scope.spawn(|| sent.send(daemon.refresh_held_node_authorities()).unwrap());
        std::thread::sleep(Duration::from_millis(50));
        // stop_requested deliberately does not end renewal: accepted effects
        // still need authority while draining. authority_shutdown ends it.
        daemon
            .authority_shutdown_flag()
            .store(true, Ordering::SeqCst);
        received
            .recv_timeout(Duration::from_millis(500))
            .expect("shutdown must not join a full-TTL retry loop")
            .unwrap();
        holder.join().unwrap();
    });
    assert!(!daemon.authority_lost());
}

#[test]
fn expired_owned_space_still_latches_global_authority_loss() {
    let fixture = enrolled_fixture("renewal-real-expiry");
    let daemon = &fixture.daemon;
    let lease = fixture
        .store
        .latest_node_daemon_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    let short = fixture
        .store
        .renew_node_daemon_lease(
            daemon.node_id(),
            daemon.daemon_id(),
            lease.generation,
            daemon.instance_id(),
            current_unix_ms_u64(),
            80,
        )
        .unwrap();
    // The durable row has really expired; retry scheduling cannot turn that
    // into a transient success even when an older cached expiry was longer.
    std::thread::sleep(Duration::from_millis(
        short.expires_unix_ms.saturating_sub(current_unix_ms_u64()) + 10,
    ));
    assert!(daemon.refresh_held_node_authorities().is_err());
    assert!(daemon.authority_lost());
    assert!(daemon.stop_requested_flag().load(Ordering::Acquire));
}
