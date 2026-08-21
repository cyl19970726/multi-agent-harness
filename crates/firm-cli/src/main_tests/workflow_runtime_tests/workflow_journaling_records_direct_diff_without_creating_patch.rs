use super::*;

    #[test]
    fn workflow_journaling_records_direct_diff_without_creating_patch() {
        let store = temp_store("direct-journal");
        let project_root = init_gc_git_project("direct-journal", &store);
        let run = WorkflowRun {
            id: generated_id("wfrun"),
            workflow_name: "direct-write-test".into(),
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
            design_intent: Some("test direct shared-repo write journaling".into()),
            spec: None,
            host_pid: None,
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        };
        let outcome = workflow::WorkflowOutcome {
            steps: vec![workflow::StepResult {
                phase: "develop".into(),
                label: "direct-writer".into(),
                provider: "codex".into(),
                isolation: None,
                ok: true,
                output_summary: "direct edit [direct diff: 6 lines]".into(),
                step_id: None,
                started_at: None,
                details: Some(serde_json::json!({
                    "write_mode": "direct",
                    "direct_diff": "diff --git a/README b/README\n--- a/README\n+++ b/README\n@@ -1 +1 @@\n-x\n+direct\n",
                    "persist_changes": "patch",
                })),
                structured: None,
                ordinal: Some(0),
            }],
            status: WorkflowRunStatus::Completed,
            summary: "direct completed".into(),
            agents_spawned: 1,
            final_output: Some(serde_json::json!({
                "result": null,
                "steps": [],
                "logs": [],
                "patch_actions": [],
                "artifact_manifests": [],
                "verdict": { "ok": true, "reason": "test" },
            })),
        };

        let value = journal_workflow_outcome(&store, run, &outcome).expect("journal");
        assert!(
            value["patches"].as_array().expect("patches").is_empty(),
            "direct shared-repo diffs are evidence, not pending WorkflowPatch rows"
        );
        assert!(latest_workflow_patches_in_append_order(&store)
            .expect("patches")
            .is_empty());
        let steps = store.workflow_steps().expect("steps");
        let result = steps
            .iter()
            .find(|step| step.label == "direct-writer")
            .and_then(|step| step.result.as_ref())
            .expect("step result");
        assert_eq!(result["write_mode"], serde_json::json!("direct"));
        assert!(result["direct_diff"]
            .as_str()
            .expect("direct diff")
            .contains("+direct"));

        let _ = std::fs::remove_dir_all(&project_root);
        let _ = std::fs::remove_dir_all(store.root());
    }

