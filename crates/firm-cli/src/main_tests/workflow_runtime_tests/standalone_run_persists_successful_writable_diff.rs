use super::*;

    #[test]
    fn standalone_run_persists_successful_writable_diff() {
        let store = temp_store("standalone-ok");
        let project_root = init_gc_git_project("standalone-ok", &store);
        let run = WorkflowRun {
            id: generated_id("wfrun"),
            workflow_name: "standalone-ok".into(),
            project_binding_id: None,
            status: WorkflowRunStatus::Running,
            step_ids: Vec::new(),
            created_at: now_string(),
            ended_at: None,
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: Some("test".into()),
            design_intent: Some("standalone D3a positive control".into()),
            spec: None,
            host_pid: None,
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        };
        let outcome = workflow::WorkflowOutcome {
            steps: vec![workflow::StepResult {
                phase: "p".into(),
                label: "ok-writer".into(),
                provider: "codex".into(),
                isolation: Some("worktree".into()),
                ok: true,
                output_summary: "ok".into(),
                step_id: Some("wfstep-ok".into()),
                started_at: None,
                details: Some(serde_json::json!({
                    "worktree_diff": new_file_diff_str("src/ok.txt", "ok"),
                    "worktree_changed_paths": ["src/ok.txt"],
                    "persist_changes": "patch",
                    "writable": true,
                })),
                structured: None,
                ordinal: Some(0),
            }],
            status: WorkflowRunStatus::Completed,
            summary: "standalone ok".into(),
            agents_spawned: 1,
            final_output: Some(serde_json::json!({
                "steps": [{ "label": "ok-writer", "ok": true, "writable": true }],
                "patch_actions": [],
                "verdict": { "ok": true, "reason": "test" },
            })),
        };
        journal_workflow_outcome(&store, run, &outcome).expect("journal");
        let patches = latest_workflow_patches_in_append_order(&store).expect("patches");
        assert_eq!(
            patches.len(),
            1,
            "a successful writable step persists a patch"
        );
        assert_eq!(patches[0].label, "ok-writer");
        assert_eq!(patches[0].changed_paths, vec!["src/ok.txt".to_string()]);

        let _ = std::fs::remove_dir_all(&project_root);
        let _ = std::fs::remove_dir_all(store.root());
    }

