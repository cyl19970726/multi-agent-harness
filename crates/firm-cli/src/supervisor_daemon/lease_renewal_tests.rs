use super::adoption_tests::{adoption_fixture, AdoptionFixture};
use super::*;
use std::sync::atomic::AtomicUsize;

fn enrolled_fixture(label: &str) -> AdoptionFixture {
    let mut fixture = adoption_fixture(label);
    fixture.daemon.lease_ttl_override_ms = Some(60_000);
    fixture.daemon.node_id = crate::latest_team_run(&fixture.store, &fixture.run_id)
        .unwrap()
        .execution_node_id;
    fixture.daemon.daemon_id = format!("node-daemon:{}", fixture.daemon.node_id);
    let daemon = &fixture.daemon;
    if !fixture
        .store
        .latest_execution_nodes()
        .unwrap()
        .iter()
        .any(|node| node.id == daemon.node_id)
    {
        fixture
            .store
            .insert_execution_node(&harness_core::ExecutionNode {
                id: daemon.node_id.clone(),
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
                node_id: daemon.node_id.clone(),
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
        .latest_node_daemon_lease(&daemon.node_id)
        .unwrap()
        .unwrap();
    let supervisor = store
        .acquire_team_supervisor_under_node_lease(
            &fixture.run_id,
            &daemon.node_id,
            &daemon.daemon_id,
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
        let node_heartbeat = scope.spawn(|| daemon.refresh_held_node_authorities());
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
        assert!(!daemon.authority_lost.load(Ordering::Acquire));
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
        .latest_node_daemon_lease(&daemon.node_id)
        .unwrap()
        .unwrap();
    assert_eq!(node.expires_unix_ms - node.renewed_unix_ms, 60_000);
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
    let mut run = crate::latest_team_run(&fixture.store, &fixture.run_id).unwrap();
    let expected = run.clone();
    run.status = harness_core::TeamRunStatus::Completed;
    fixture
        .store
        .compare_and_append_team_run_lifecycle(&expected, &run)
        .unwrap();
    fixture
        .daemon
        .contexts
        .lock()
        .unwrap()
        .push(MultiTeamContext {
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
        });
    while !fixture.daemon.contexts.lock().unwrap()[0]
        .thread
        .as_ref()
        .unwrap()
        .is_finished()
    {
        std::thread::yield_now();
    }
    fixture.daemon.reap_finished().unwrap();
    assert!(!fixture.daemon.authority_lost.load(Ordering::Acquire));
    assert!(!fixture.daemon.stop_requested.load(Ordering::Acquire));
    fixture.daemon.refresh_held_node_authorities().unwrap();
    assert!(fixture.daemon.contexts.lock().unwrap().is_empty());
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
