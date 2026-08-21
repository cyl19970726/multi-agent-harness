use super::*;

    #[test]
    fn heartbeat_latches_immediately_on_terminal_parent_fence() {
        // A real TEAM_SUPERVISOR_PARENT_FENCED renewal error (exact production
        // message) must latch lease-loss on the FIRST failed renewal: no
        // retry, no backoff, and the thread exits.
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_thread = Arc::clone(&attempts);
        let policy = SupervisorHeartbeatPolicy {
            team_run_id: "team-run-terminal".to_string(),
            supervisor_id: "supervisor-terminal".to_string(),
            generation: 7,
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
        let thread = std::thread::spawn(move || {
            run_supervisor_heartbeat_loop(
                &policy,
                &stop_thread,
                &valid_thread,
                &gate_thread,
                move || {
                    attempts_thread.fetch_add(1, Ordering::SeqCst);
                    Err(StoreError::Conflict(
                        "TEAM_SUPERVISOR_PARENT_FENCED: parent NodeDaemon generation is no longer active for TeamRun team-run-terminal".to_string(),
                    ))
                },
                || None,
            );
        });
        thread
            .join()
            .expect("heartbeat thread exits after a terminal fence");
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "a terminal fence must latch on the first failed renewal without retries"
        );
        assert!(
            !valid.load(Ordering::Acquire),
            "heartbeat did not latch lease-loss on a terminal parent fence"
        );
    }

