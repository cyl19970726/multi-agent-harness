use super::*;

    #[test]
    fn run_ndjson_child_kills_a_hung_worker_via_timeout() {
        // A worker that emits one line then HANGS (stdout open, never exits) goes
        // SILENT, so the IDLE timeout fires and kills it — not block forever.
        let root = std::env::temp_dir().join(format!("mah-hang-{}", generated_id("t")));
        let session_dir = root.join("runtimes/test-workers").join("s");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("printf '{\"type\":\"item\"}\\n'; sleep 600");

        let start = Instant::now();
        // 500ms IDLE limit: after the one event, silence > 500ms → killed.
        let run = run_ndjson_child(
            cmd,
            &session_dir,
            "s",
            "out.ndjson",
            500,
            None,
            None,
            "ephemeral worker",
        )
        .expect("run");
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(8),
            "must not block on the hung child; took {elapsed:?}"
        );
        assert!(run.timed_out, "the idle timeout must have fired");
        assert!(!run.process_success);
        assert!(run
            .warnings
            .iter()
            .any(|warning| warning == "ephemeral worker timed out"));
        // The single event emitted before the hang was still captured live.
        assert_eq!(run.events.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

