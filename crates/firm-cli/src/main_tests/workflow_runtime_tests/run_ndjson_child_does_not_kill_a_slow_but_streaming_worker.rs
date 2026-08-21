use super::*;

    #[test]
    fn run_ndjson_child_does_not_kill_a_slow_but_streaming_worker() {
        // The point of the IDLE timeout: a worker that keeps emitting events runs to
        // completion even though its TOTAL runtime (~1.5s) exceeds the idle
        // limit (1s) — because it never goes silent that long. A fixed total-
        // wall-clock timeout would have wrongly killed it.
        let root = std::env::temp_dir().join(format!("mah-slow-{}", generated_id("t")));
        let session_dir = root.join("runtimes/test-workers").join("s");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let mut cmd = Command::new("sh");
        // 15 events, ~100ms apart → ~1.5s total, never silent for 1s. The
        // generous idle bound keeps this timing test stable under parallel CI.
        cmd.arg("-c")
            .arg("for i in $(seq 1 15); do printf '{\"type\":\"item\"}\\n'; sleep 0.1; done");

        let run = run_ndjson_child(
            cmd,
            &session_dir,
            "s",
            "out.ndjson",
            1_000,
            None,
            None,
            "ephemeral worker",
        )
        .expect("run");

        assert!(
            !run.timed_out,
            "a continuously-streaming worker must NOT be killed by the idle timeout"
        );
        assert!(run.process_success, "it should exit cleanly on its own");
        assert_eq!(run.events.len(), 15, "all streamed events captured");
        let _ = fs::remove_dir_all(&root);
    }

