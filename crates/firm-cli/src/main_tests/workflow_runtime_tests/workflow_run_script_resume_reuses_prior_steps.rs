use super::*;

    #[test]
    fn workflow_run_script_resume_reuses_prior_steps() {
        let store = temp_store("run-script-resume");
        let script = r#"
workflow("triage", "scan first then fix, so the fix builds on the scan output")
a = agent("scan the code")
agent("fix per " + a, label = "fixer")
"#;
        let dir = std::env::temp_dir().join(format!("harness-wf-resume-{}", generated_id("src")));
        fs::create_dir_all(&dir).expect("mkdir script dir");
        let path = dir.join("triage.star");
        fs::write(&path, script).expect("write script");

        // First run (dry-run) to journal succeeded steps carrying ordinals.
        let args = vec![path.display().to_string(), "--dry-run".to_string()];
        let first = workflow_run_script_value(&store, None, &args).expect("first run");
        let prior_run_id = first
            .get("run")
            .and_then(|r| r.get("id"))
            .and_then(|v| v.as_str())
            .expect("prior run id")
            .to_string();

        // The prior steps carry an ordinal in their result JSON (the round-trip).
        let prior_steps: Vec<WorkflowStep> = latest_workflow_steps_in_append_order(&store)
            .expect("steps")
            .into_iter()
            .filter(|s| s.run_id == prior_run_id)
            .collect();
        assert!(prior_steps.iter().all(|s| s
            .result
            .as_ref()
            .and_then(|r| r.get("ordinal"))
            .is_some()));

        // Resume: re-run the SAME script with --resume <prior_run_id>.
        let resume_args = vec![
            path.display().to_string(),
            "--dry-run".to_string(),
            "--resume".to_string(),
            prior_run_id.clone(),
        ];
        let second = workflow_run_script_value(&store, None, &resume_args).expect("resume run");
        let run = second.get("run").expect("run key");
        assert_eq!(
            run.get("status").and_then(|s| s.as_str()),
            Some("completed")
        );
        let new_run_id = run.get("id").and_then(|v| v.as_str()).expect("new run id");
        assert_ne!(new_run_id, prior_run_id, "resume mints a NEW run id");
        let step_ids = run
            .get("step_ids")
            .and_then(|s| s.as_array())
            .expect("step_ids");
        assert_eq!(step_ids.len(), 2, "the resumed run references both leaves");

        // The new run records which prior run it resumed from.
        let runs = store.workflow_runs().expect("read runs");
        let final_run = runs
            .iter()
            .rev()
            .find(|r| r.id == new_run_id)
            .expect("new run row");
        assert_eq!(
            final_run
                .spec
                .as_ref()
                .and_then(|s| s.get("resumed_from"))
                .and_then(|v| v.as_str()),
            Some(prior_run_id.as_str())
        );

        // The new run's steps carry the [replayed] marker (driver not re-invoked).
        let new_steps: Vec<WorkflowStep> = latest_workflow_steps_in_append_order(&store)
            .expect("steps")
            .into_iter()
            .filter(|s| s.run_id == new_run_id)
            .collect();
        assert_eq!(new_steps.len(), 2);
        for step in &new_steps {
            assert!(
                step.output_summary
                    .as_deref()
                    .unwrap_or_default()
                    .starts_with("[replayed] "),
                "resumed step output: {:?}",
                step.output_summary
            );
            assert_eq!(
                step.result.as_ref().and_then(|r| r.get("replayed")),
                Some(&serde_json::json!(true))
            );
        }
    }

