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

/// ADR 0075 retarget of `default_leases_survive_ten_second_writer_contention_without_extending_ttl`.
///
/// The contention is kept exactly as it was — ten real seconds of a held Space
/// `.store.lock` with a bulk reader hammering the same Space — because that is
/// the gen-4 incident in miniature. What changes is what it has to prove.
/// "Survived without extending the TTL" was the strongest claim available while
/// the heartbeat queued on that lock: the renewal could only be shown not to
/// have died. The machine lease now lives under its own leaf lock, so the
/// stronger claim is available and is asserted here: the contention does not
/// slow the machine heartbeat down at all, measured against the renewal
/// diagnostics the daemon already records.
#[test]
fn a_ten_second_space_lock_does_not_slow_the_machine_heartbeat() {
    let fixture = enrolled_fixture("renewal-ten-second-lock");
    let daemon = &fixture.daemon;
    let store = &fixture.store;
    let node = store
        .current_authorized_machine_lease(daemon.node_id())
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
        // Returns how long the machine heartbeat needed to land its first
        // renewal, measured from inside the ten-second Space-lock hold. On the
        // pre-cutover writer this could only be `>= the hold`, because the
        // renewal queued behind the very lock this scope is holding; that is
        // what "did not expire" was hiding.
        let node_heartbeat = scope.spawn(|| -> CliResult<Duration> {
            let started = Instant::now();
            loop {
                daemon.refresh_held_node_authorities()?;
                if store
                    .current_authorized_machine_lease(daemon.node_id())?
                    .unwrap()
                    .renewed_unix_ms
                    > node.renewed_unix_ms
                {
                    return Ok(started.elapsed());
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
        // Measured while the Space lock is still held, before the holder
        // releases it: the renewal must already have landed.
        let renewal_wait = node_heartbeat.join().unwrap().unwrap();
        assert!(
            renewal_wait < Duration::from_secs(2),
            "a contended Space data lock must not delay the machine heartbeat;              the renewal took {renewal_wait:?} while the lock was held"
        );
        drop(lock);
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
        .current_authorized_machine_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    assert_eq!(node.expires_unix_ms - node.renewed_unix_ms, 20_000);
    let diagnostics = crate::lease_renewal_diagnostics::snapshot();
    let supervisor_diagnostic = diagnostics
        .iter()
        .find(|v| v["id"] == fixture.run_id)
        .unwrap();
    // The same admitted FIFO wait succeeds after the holder releases. It
    // must no longer manufacture repeated timeout failures during that wait.
    assert_eq!(supervisor_diagnostic["failure_count"].as_u64(), Some(0));
    assert!(
        supervisor_diagnostic["attempt_elapsed_ms"]
            .as_u64()
            .unwrap()
            >= 9_000
    );
    assert!(supervisor_diagnostic["last_error"].is_null());
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

/// ADR 0075 retarget of `contended_space_does_not_delay_healthy_space_to_expiry`.
///
/// "Healthy space" is the phrase that stopped meaning anything. The old test
/// proved one Space's lock contention could not starve a *different* Space's
/// lease into expiry, because each Space carried its own lease and its own
/// renewal worker. There is one lease now, so there is no healthy Space to
/// protect — and the successor property is the one the ADR is actually for: a
/// contended Space data lock does not delay the machine heartbeat at all. The
/// contention is kept exactly as it was and the assertion is raised: the
/// renewal must land *while* the lock is held, not merely before the TTL.
#[test]
fn a_contended_space_does_not_delay_the_machine_heartbeat_at_all() {
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
    // The short TTL has to belong to the generation from the start. `expires` is
    // monotonic within a status (ADR 0075), so a generation acquired under the
    // 20 s default can never renew *down* to 1.2 s — every such renewal is
    // refused as a backwards expiry until the lease simply runs out. Production
    // never meets that because a generation's TTL is fixed for its life; the
    // fixture has to take a fresh generation rather than shrink a live one.
    let held = fixture
        .store
        .current_authorized_machine_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    fixture
        .store
        .release_machine_authority_for_test(
            daemon.node_id(),
            daemon.daemon_id(),
            held.generation,
            daemon.instance_id(),
            current_unix_ms_u64(),
        )
        .unwrap();
    let contended_space = daemon
        .registered_spaces()
        .unwrap()
        .into_iter()
        .find(|(candidate, _)| candidate.id == fixture.execution_space_id)
        .unwrap()
        .0;
    daemon
        .ensure_node_authority(&contended_space, &fixture.store)
        .unwrap();
    daemon.ensure_node_authority(&space, &healthy).unwrap();
    let initial = healthy
        .current_authorized_machine_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    assert!(initial.generation > held.generation);
    assert!(initial.expires_unix_ms - initial.renewed_unix_ms <= 1_200);
    // The contention is unchanged: the first Space's data lock is held for two
    // full seconds, well past the 1.2 s lease TTL this fixture runs on.
    let lock = fixture.store.acquire_exclusive_migration_guard().unwrap();
    let (midpoint, result) = std::thread::scope(|scope| {
        let heartbeat = scope.spawn(|| daemon.run_held_node_authorities());
        let holder = scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(2_000));
            drop(lock);
        });
        // Sampled while the Space lock is still held and already past the TTL
        // the lease started with. Before the cutover the renewal for *this*
        // Space would have been queued behind that lock for the whole window.
        std::thread::sleep(Duration::from_millis(1_400));
        let midpoint = healthy
            .current_authorized_machine_lease(daemon.node_id())
            .unwrap()
            .unwrap();
        holder.join().unwrap();
        daemon
            .authority_shutdown_flag()
            .store(true, Ordering::SeqCst);
        (midpoint, heartbeat.join().unwrap())
    });
    result.unwrap();
    // Renewed repeatedly *during* the contention, not rescued after it.
    assert!(
        midpoint.renewed_unix_ms >= initial.renewed_unix_ms + 600,
        "the machine heartbeat must renew while a Space data lock is held:          {} vs {}",
        midpoint.renewed_unix_ms,
        initial.renewed_unix_ms
    );
    assert!(midpoint.expires_unix_ms > initial.expires_unix_ms);
    assert!(midpoint.expires_unix_ms > initial.expires_unix_ms + 600);
    assert!(!daemon.authority_lost());
    // One machine, one lease: both Stores answer with the same live document.
    let from_contended = fixture
        .store
        .current_authorized_machine_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    let from_other = healthy
        .current_authorized_machine_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    assert_eq!(from_contended, from_other);
    assert!(from_contended.expires_unix_ms > current_unix_ms_u64());
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

/// ADR 0075 retarget of `expired_owned_space_still_latches_global_authority_loss`.
///
/// The old name claimed a per-Space expiry escalated to a *global* loss, which
/// was true only because `ensure_node_authority_bundle` treated any member's
/// failure as total. There is no per-Space expiry to escalate now: the machine
/// lease is one document, and an expired document is a machine-wide loss by
/// construction rather than by a rule written down in prose. The successor
/// property is the same latch, proven against the record that now decides it —
/// and the expiry is produced the way elapsed time produces one, because
/// `expires` is monotonic within a status and nothing may shorten a live lease.
#[test]
fn an_expired_machine_lease_latches_authority_loss() {
    let fixture = enrolled_fixture("renewal-real-expiry");
    let daemon = &fixture.daemon;
    let live = fixture
        .store
        .current_authorized_machine_lease(daemon.node_id())
        .unwrap()
        .unwrap();
    assert!(live.expires_unix_ms > current_unix_ms_u64());
    let expired = fixture
        .store
        .expire_machine_lease_for_test(daemon.node_id())
        .unwrap();
    assert_eq!(expired.generation, live.generation);
    // The durable document has really expired; retry scheduling cannot turn
    // that into a transient success even when an older cached expiry was
    // longer.
    std::thread::sleep(Duration::from_millis(10));
    assert!(daemon.refresh_held_node_authorities().is_err());
    assert!(daemon.authority_lost());
    assert!(daemon.stop_requested_flag().load(Ordering::Acquire));
}
