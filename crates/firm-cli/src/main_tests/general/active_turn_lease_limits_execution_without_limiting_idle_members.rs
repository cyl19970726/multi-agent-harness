use super::*;

    #[test]
    fn active_turn_lease_limits_execution_without_limiting_idle_members() {
        let pool = Arc::new(ActiveTurnLeasePool::new(1));
        let first = pool.acquire();
        let contender_pool = Arc::clone(&pool);
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let contender = std::thread::spawn(move || {
            let _second = contender_pool.acquire();
            acquired_tx.send(()).expect("report second lease");
        });

        assert!(
            acquired_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "a second active provider turn must wait while the only lease is held"
        );
        drop(first);
        acquired_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("idle/finished turn must release the lease");
        contender.join().expect("contender");
    }

