use super::*;

    #[test]
    fn workflow_run_script_resume_rejects_changed_script() {
        let store = temp_store("run-script-resume-changed");
        let dir =
            std::env::temp_dir().join(format!("harness-wf-resume-chg-{}", generated_id("src")));
        fs::create_dir_all(&dir).expect("mkdir script dir");
        let path = dir.join("triage.star");
        let original = r#"
workflow("triage", "a stable design intent that explains the shape")
agent("scan the code")
"#;
        fs::write(&path, original).expect("write script");
        let first = workflow_run_script_value(
            &store,
            None,
            &[path.display().to_string(), "--dry-run".to_string()],
        )
        .expect("first run");
        let prior_run_id = first
            .get("run")
            .and_then(|r| r.get("id"))
            .and_then(|v| v.as_str())
            .expect("prior id")
            .to_string();

        // Edit the script, then attempt to resume — the guard must reject it.
        let changed = r#"
workflow("triage", "a stable design intent that explains the shape")
agent("scan the code")
agent("a NEW second leaf that changes the ordinal alignment")
"#;
        fs::write(&path, changed).expect("rewrite script");
        let err = workflow_run_script_value(
            &store,
            None,
            &[
                path.display().to_string(),
                "--dry-run".to_string(),
                "--resume".to_string(),
                prior_run_id,
            ],
        )
        .expect_err("changed script rejected");
        match err {
            CliError::Usage(msg) => assert!(
                msg.contains("the script changed"),
                "unexpected message: {msg}"
            ),
            other => panic!("expected Usage error, got {other:?}"),
        }
    }

