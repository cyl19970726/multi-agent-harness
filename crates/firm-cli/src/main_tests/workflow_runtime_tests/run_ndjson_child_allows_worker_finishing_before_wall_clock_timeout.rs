use super::*;

    #[test]
    fn run_ndjson_child_allows_worker_finishing_before_wall_clock_timeout() {
        let root = std::env::temp_dir().join(format!("mah-wall-ok-{}", generated_id("t")));
        let session_dir = root.join("runtimes/test-workers").join("s");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("for i in 1 2 3; do printf '{\"type\":\"item\"}\\n'; sleep 0.05; done");

        let run = run_ndjson_child(
            cmd,
            &session_dir,
            "s",
            "out.ndjson",
            1_000,
            Some(2_000),
            None,
            "ephemeral worker",
        )
        .expect("run");

        assert!(!run.wall_timed_out);
        assert!(!run.timed_out);
        assert!(run.process_success);
        assert_eq!(run.events.len(), 3);
        assert!(run
            .warnings
            .iter()
            .all(|warning| { !warning.contains("per-leaf wall-clock timeout") }));
        let _ = fs::remove_dir_all(&root);
    }

