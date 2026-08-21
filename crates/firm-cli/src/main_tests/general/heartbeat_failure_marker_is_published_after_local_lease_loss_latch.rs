use super::*;

    #[test]
    fn heartbeat_failure_marker_is_published_after_local_lease_loss_latch() {
        let marker = std::env::temp_dir().join(format!(
            "firm-heartbeat-failure-latched-{}",
            generated_id("test")
        ));
        let supervisor_valid = Arc::new(AtomicBool::new(true));
        let observed_valid = Arc::clone(&supervisor_valid);
        let observed_marker = marker.clone();
        let observer = std::thread::spawn(move || {
            while !observed_marker.exists() {
                std::thread::yield_now();
            }
            observed_valid.load(Ordering::Acquire)
        });

        let error = latch_supervisor_lease_lost_and_mark(
            &supervisor_valid,
            "team-run-test",
            "supervisor-test",
            1,
            "injected renewal failure",
            Some(&marker),
        );

        assert!(error.is_supervisor_lease_lost());
        assert!(
            !observer.join().expect("marker observer"),
            "heartbeat_valid remained true after the failure marker became observable"
        );
        assert_eq!(
            fs::read(&marker).expect("heartbeat failure marker"),
            b"heartbeat failure latched"
        );
        fs::remove_file(&marker).expect("remove heartbeat failure marker");
    }

