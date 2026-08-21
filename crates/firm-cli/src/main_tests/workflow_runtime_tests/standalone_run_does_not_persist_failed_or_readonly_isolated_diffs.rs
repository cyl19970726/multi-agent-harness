use super::*;

    #[test]
    fn standalone_run_does_not_persist_failed_or_readonly_isolated_diffs() {
        let store = temp_store("standalone-d3a");
        let project_root = init_gc_git_project("standalone-d3a", &store);
        let run = WorkflowRun {
            id: generated_id("wfrun"),
            workflow_name: "standalone".into(),
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
            design_intent: Some("standalone D3a persistence gate".into()),
            spec: None, // NOT orchestrated
            host_pid: None,
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        };
        let failed_writable = workflow::StepResult {
            phase: "p".into(),
            label: "failed-writer".into(),
            provider: "codex".into(),
            isolation: Some("worktree".into()),
            ok: false, // step FAILED
            output_summary: "boom".into(),
            step_id: Some("wfstep-failed".into()),
            started_at: None,
            details: Some(serde_json::json!({
                "worktree_diff": new_file_diff_str("src/partial.txt", "partial"),
                "worktree_changed_paths": ["src/partial.txt"],
                "persist_changes": "patch",
                "writable": true,
            })),
            structured: None,
            ordinal: Some(0),
        };
        let readonly_isolated = workflow::StepResult {
            phase: "p".into(),
            label: "kimi-reader".into(),
            provider: "kimi".into(),
            isolation: Some("worktree".into()),
            ok: true,
            output_summary: "read but wrote anyway".into(),
            step_id: Some("wfstep-kimi".into()),
            started_at: None,
            details: Some(serde_json::json!({
                // A read-only leaf that produced a stray diff (unauthorized write).
                // writable=false → the persistence gate discards it regardless of
                // whether the leaf isolated (post #190 it would not).
                "worktree_diff": new_file_diff_str("src/sneaky.txt", "sneaky"),
                "worktree_changed_paths": ["src/sneaky.txt"],
                "writable": false,
            })),
            structured: None,
            ordinal: Some(1),
        };
        let outcome = workflow::WorkflowOutcome {
            steps: vec![failed_writable, readonly_isolated],
            status: WorkflowRunStatus::Completed,
            summary: "standalone".into(),
            agents_spawned: 2,
            final_output: Some(serde_json::json!({
                "steps": [
                    { "label": "failed-writer", "ok": false, "writable": true },
                    { "label": "kimi-reader", "ok": true, "writable": false }
                ],
                "patch_actions": [],
                "verdict": { "ok": true, "reason": "test" },
            })),
        };

        journal_workflow_outcome(&store, run, &outcome).expect("journal");
        assert!(
            latest_workflow_patches_in_append_order(&store)
                .expect("patches")
                .is_empty(),
            "neither a failed writable step nor a read-only isolated leaf persists a patch (D3a)"
        );

        let _ = std::fs::remove_dir_all(&project_root);
        let _ = std::fs::remove_dir_all(store.root());
    }

