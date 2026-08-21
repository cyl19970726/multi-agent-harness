use super::*;

    #[test]
    fn workflow_journaling_skips_discard_and_auto_applies_on_verdict() {
        let store = temp_store("patch-auto");
        let project_root = init_gc_git_project("patch-auto", &store);
        let new_file_diff = |path: &str, content: &str| {
            format!(
                "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{content}\n"
            )
        };
        let mk_step =
            |label: &str, path: &str, persist_changes: &str, auto: bool| workflow::StepResult {
                phase: "develop".into(),
                label: label.into(),
                provider: "codex".into(),
                isolation: Some("worktree".into()),
                ok: true,
                output_summary: format!("{label} wrote {path}"),
                step_id: None,
                started_at: None,
                details: Some(serde_json::json!({
                    "worktree_diff": new_file_diff(path, label),
                    "persist_changes": persist_changes,
                    "owned_paths": ["src"],
                    "auto_apply_on_verdict": auto,
                    "writable": true,
                })),
                structured: None,
                ordinal: None,
            };
        let run = WorkflowRun {
            id: generated_id("wfrun"),
            workflow_name: "patch-auto-test".into(),
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
            design_intent: Some("test discard and auto-apply patch behavior".into()),
            spec: None,
            host_pid: None,
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        };
        let outcome = workflow::WorkflowOutcome {
            steps: vec![
                mk_step("discard", "src/discard.txt", "discard", false),
                mk_step("auto", "src/auto.txt", "patch", true),
            ],
            status: WorkflowRunStatus::Completed,
            summary: "patch auto completed".into(),
            agents_spawned: 2,
            final_output: Some(serde_json::json!({
                "result": null,
                "steps": [
                    { "label": "discard", "auto_apply_on_verdict": false },
                    { "label": "auto", "auto_apply_on_verdict": true }
                ],
                "logs": [],
                "patch_actions": [],
                "artifact_manifests": [],
                "verdict": { "ok": true, "reason": "test" },
            })),
        };

        journal_workflow_outcome(&store, run, &outcome).expect("journal");
        let patches = latest_workflow_patches_in_append_order(&store).expect("patches");
        assert_eq!(patches.len(), 1, "discarded diffs do not create patches");
        assert_eq!(patches[0].label, "auto");
        assert_eq!(patches[0].status, WorkflowPatchStatus::Applied);
        assert_eq!(
            std::fs::read_to_string(project_root.join("src/auto.txt")).expect("auto file"),
            "auto\n"
        );
        assert!(
            !project_root.join("src/discard.txt").exists(),
            "discarded patch never lands"
        );

        let _ = std::fs::remove_dir_all(&project_root);
        let _ = std::fs::remove_dir_all(store.root());
    }

