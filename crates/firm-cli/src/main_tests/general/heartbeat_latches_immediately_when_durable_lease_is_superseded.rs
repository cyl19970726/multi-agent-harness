use super::*;

#[test]
fn heartbeat_latches_immediately_when_durable_lease_is_superseded() {
    // Real store: releasing the durable lease behind the running heartbeat
    // makes the next renewal hit the genuine "is no longer owned by"
    // conflict, which must latch immediately without retries.
    let (store, root) = temp_store("heartbeat-terminal-supersede");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-heartbeat-superseded",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("acquire Supervisor lease");
    store
        .release_team_supervisor_lease(
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms_u64(),
        )
        .expect("release durable lease behind the heartbeat");

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_thread = Arc::clone(&attempts);
    let policy = SupervisorHeartbeatPolicy {
        team_run_id: created.team_run.id.clone(),
        supervisor_id: lease.supervisor_id.clone(),
        generation: lease.generation,
        ttl_ms: 600_000,
        heartbeat_interval_ms: 5,
        max_transient_failures: 3,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let valid = Arc::new(AtomicBool::new(true));
    let gate = Arc::new(Mutex::new(()));
    let stop_thread = Arc::clone(&stop);
    let valid_thread = Arc::clone(&valid);
    let gate_thread = Arc::clone(&gate);
    let store_thread = store.clone();
    let thread = std::thread::spawn(move || {
        let store_thread = store_thread;
        let policy_thread = policy;
        let renew_policy = policy_thread.clone();
        run_supervisor_heartbeat_loop(
            &policy_thread,
            &stop_thread,
            &valid_thread,
            &gate_thread,
            move || {
                attempts_thread.fetch_add(1, Ordering::SeqCst);
                store_thread
                    .renew_team_supervisor_lease(
                        &renew_policy.team_run_id,
                        &renew_policy.supervisor_id,
                        renew_policy.generation,
                        current_unix_ms_u64(),
                        renew_policy.ttl_ms,
                    )
                    .map(|_lease| ())
            },
            || None,
        );
    });
    thread
        .join()
        .expect("heartbeat thread exits after supersession");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a superseded durable lease must latch on the first failed renewal without retries"
    );
    assert!(
        !valid.load(Ordering::Acquire),
        "heartbeat did not latch lease-loss on a superseded durable lease"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
