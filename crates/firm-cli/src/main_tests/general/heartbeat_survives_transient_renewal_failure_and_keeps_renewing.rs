use super::*;

#[test]
fn heartbeat_survives_transient_renewal_failure_and_keeps_renewing() {
    let (store, root) = temp_store("heartbeat-survives-transient");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-heartbeat-survives",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("acquire Supervisor lease");
    let initial = latest_heartbeat_ms(&store, &created.team_run.id);
    let marker = std::env::temp_dir().join(format!(
        "firm-heartbeat-transient-marker-{}",
        generated_id("test")
    ));

    let policy = SupervisorHeartbeatPolicy {
        team_run_id: created.team_run.id.clone(),
        supervisor_id: lease.supervisor_id.clone(),
        generation: lease.generation,
        ttl_ms: 600_000,
        heartbeat_interval_ms: 5,
        max_transient_failures: 10,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let valid = Arc::new(AtomicBool::new(true));
    let gate = Arc::new(Mutex::new(()));
    let stop_thread = Arc::clone(&stop);
    let valid_thread = Arc::clone(&valid);
    let gate_thread = Arc::clone(&gate);
    let store_thread = store.clone();
    let marker_thread = marker.clone();
    let thread = std::thread::spawn(move || {
        let store_thread = store_thread;
        let policy_thread = policy;
        let renew_policy = policy_thread.clone();
        run_supervisor_heartbeat_loop(
            &policy_thread,
            &stop_thread,
            &valid_thread,
            &gate_thread,
            || {
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
            move || {
                if marker_thread.exists() {
                    Some(marker_thread.clone())
                } else {
                    None
                }
            },
        );
    });

    // The loop is renewing normally before the injected failure.
    let first_renewal =
        wait_until_heartbeat_advances(&store, &created.team_run.id, initial, "renew once");
    assert!(
        valid.load(Ordering::Acquire),
        "heartbeat latched lease-loss while renewing normally"
    );

    // Inject a transient failure window. While the marker exists the loop
    // must keep retrying (never latching); the durable heartbeat freezes
    // because no real renewal is issued.
    fs::write(&marker, b"transient").expect("write transient failure marker");
    std::thread::sleep(Duration::from_millis(30));
    let frozen_at = latest_heartbeat_ms(&store, &created.team_run.id);
    std::thread::sleep(Duration::from_millis(60));
    let still_frozen = latest_heartbeat_ms(&store, &created.team_run.id);
    assert!(
        frozen_at >= first_renewal,
        "heartbeat moved backwards during the failure window"
    );
    assert_eq!(
        frozen_at, still_frozen,
        "heartbeat advanced while the injected transient failure was active; \
             the loop may have latched or died"
    );
    assert!(
        valid.load(Ordering::Acquire),
        "heartbeat latched lease-loss on a transient failure"
    );
    assert!(
        !thread.is_finished(),
        "heartbeat thread died on a transient failure"
    );

    // Remove the failure: the same thread must resume renewing.
    fs::remove_file(&marker).expect("remove transient failure marker");
    let recovered_at =
        wait_until_heartbeat_advances(&store, &created.team_run.id, still_frozen, "resume");
    assert!(
        recovered_at > still_frozen,
        "heartbeat did not resume after the transient failure window"
    );
    assert!(
        valid.load(Ordering::Acquire),
        "heartbeat latched lease-loss after the transient failure cleared"
    );
    assert!(
        !thread.is_finished(),
        "heartbeat thread died after the transient failure cleared"
    );

    stop.store(true, Ordering::Release);
    thread.join().expect("heartbeat thread stops cleanly");
    std::fs::remove_dir_all(root).expect("cleanup");
    let _ = fs::remove_file(&marker);
}
