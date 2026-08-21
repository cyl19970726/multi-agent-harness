use super::*;

    #[test]
    fn run_ndjson_child_warns_and_keeps_valid_events_after_junk_stdout() {
        let root = std::env::temp_dir().join(format!("mah-junk-{}", generated_id("t")));
        let session_dir = root.join("runtimes/test-workers").join("s");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("printf 'not-json\\n'; printf '{\"type\":\"item\",\"n\":1}\\n'");

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

        assert!(run.process_success);
        assert!(!run.timed_out);
        assert_eq!(
            run.events,
            vec![serde_json::json!({"type": "item", "n": 1})]
        );
        assert!(run
            .warnings
            .iter()
            .any(|warning| warning == "1 stdout line(s) were not valid JSON and were dropped"));
        let _ = fs::remove_dir_all(&root);
    }

