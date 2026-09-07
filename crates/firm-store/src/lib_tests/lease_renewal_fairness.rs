use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

const NODE: &str = "00000000-0000-4000-8000-000000000001";

fn queue_count(store: &HarnessStore) -> u64 {
    store.process_write_lock.state.lock().unwrap().next_ticket
}

fn wait_queued(store: &HarnessStore, expected: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while queue_count(store) < expected {
        assert!(Instant::now() < deadline, "writer must enter FIFO");
        std::thread::yield_now();
    }
}

fn fixture(label: &str) -> (HarnessStore, NodeDaemonLease, TeamSupervisorLease) {
    let store = lock_policy_test_store(label);
    seed_lease_run(&store, "run-renewal");
    let supervisor = store
        .acquire_test_supervisor_lease(
            "run-renewal",
            "supervisor-test",
            1,
            "tcp://127.0.0.1:1",
            1_000,
            10_000,
        )
        .unwrap();
    let parent = store.latest_node_daemon_lease(NODE).unwrap().unwrap();
    let node = store
        .renew_node_daemon_lease(
            NODE,
            &parent.daemon_id,
            parent.generation,
            &parent.instance_id,
            1_001,
            10_000,
        )
        .unwrap();
    (store, node, supervisor)
}

fn queued_renewal_precedes_later_writers(supervisor_mode: bool) {
    let (store, node, supervisor) = fixture("renewal-fifo");
    let first = store.acquire_write_lock().unwrap();
    let baseline = queue_count(&store);
    std::thread::scope(|scope| {
        let renewal = scope.spawn(|| {
            if supervisor_mode {
                store
                    .renew_team_supervisor_lease(
                        &supervisor.team_run_id,
                        &supervisor.supervisor_id,
                        supervisor.generation,
                        1_002,
                        10_000,
                    )
                    .map(|_| ())
            } else {
                store
                    .renew_node_daemon_lease(
                        NODE,
                        &node.daemon_id,
                        node.generation,
                        &node.instance_id,
                        1_002,
                        10_000,
                    )
                    .map(|_| ())
            }
        });
        wait_queued(&store, baseline + 1);
        let mut writers = Vec::new();
        for index in 0..3 {
            let store = &store;
            writers.push(scope.spawn(move || {
                let _guard = store.acquire_write_lock().unwrap();
                let renewed = if supervisor_mode {
                    store
                        .latest_lease_for_run_unlocked("run-renewal")
                        .unwrap()
                        .unwrap()
                        .heartbeat_unix_ms
                } else {
                    store
                        .latest_node_daemon_lease(NODE)
                        .unwrap()
                        .unwrap()
                        .renewed_unix_ms
                };
                store
                    .append_jsonl_unlocked("test_fifo_writers.jsonl", &index)
                    .unwrap();
                std::thread::sleep(Duration::from_millis(300));
                renewed
            }));
            wait_queued(store, baseline + 2 + index);
        }
        // Renewal is definitely queued before this interval begins. Later
        // writers stay queued throughout; a cancelled 250ms ticket loses.
        std::thread::sleep(Duration::from_millis(400));
        drop(first);
        let renewed = renewal.join().unwrap();
        let observations = writers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>();
        assert!(
            renewed.is_ok(),
            "renewal must retain its original FIFO position: {renewed:?}"
        );
        assert!(observations.iter().all(|observed| *observed > 1_002));
    });
    let writes: Vec<u64> = store.read_jsonl("test_fifo_writers.jsonl").unwrap();
    assert_eq!(writes, vec![0, 1, 2]);
}

#[test]
fn node_renewal_keeps_fifo_position_ahead_of_continuous_writers() {
    queued_renewal_precedes_later_writers(false);
}

#[test]
fn supervisor_renewal_keeps_fifo_position_ahead_of_continuous_writers() {
    queued_renewal_precedes_later_writers(true);
}

#[test]
fn cancelled_renewal_removes_its_ticket_and_later_writers_progress() {
    let (store, node, _) = fixture("renewal-cancel");
    let first = store.acquire_write_lock().unwrap();
    let baseline = queue_count(&store);
    let cancelled = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let renewal = scope.spawn(|| {
            store.renew_node_daemon_lease_cancellable(
                NODE,
                &node.daemon_id,
                node.generation,
                &node.instance_id,
                1_002,
                10_000,
                &|| cancelled.load(Ordering::Acquire),
            )
        });
        wait_queued(&store, baseline + 1);
        cancelled.store(true, Ordering::Release);
        assert!(renewal
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("STORE_LOCK_CANCELLED"));
        drop(first);
        let _next = store.acquire_write_lock().unwrap();
    });
    assert_eq!(store.latest_node_daemon_lease(NODE).unwrap().unwrap(), node);
}

#[test]
fn renewal_flock_wait_uses_confirmed_expiry_without_extending_authority() {
    let (store, node, _) = fixture("renewal-flock-deadline");
    let short = store
        .renew_node_daemon_lease(
            NODE,
            &node.daemon_id,
            node.generation,
            &node.instance_id,
            1_002,
            120,
        )
        .unwrap();
    let lock = hold_store_lock(&store); // Separate fd holds the OS flock, not FIFO.
    let started = Instant::now();
    let error = store
        .renew_node_daemon_lease(
            NODE,
            &node.daemon_id,
            node.generation,
            &node.instance_id,
            1_002,
            10_000,
        )
        .unwrap_err();
    drop(lock);
    assert!(matches!(error, StoreError::LockTimeout(_)));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        store.latest_node_daemon_lease(NODE).unwrap().unwrap(),
        short
    );
    assert!(store
        .renew_node_daemon_lease(
            NODE,
            &node.daemon_id,
            node.generation,
            &node.instance_id,
            short.expires_unix_ms,
            10_000,
        )
        .is_err());
}
