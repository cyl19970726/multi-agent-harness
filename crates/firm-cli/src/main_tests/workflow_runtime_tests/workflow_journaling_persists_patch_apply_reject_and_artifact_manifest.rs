use super::*;

#[test]
fn workflow_journaling_persists_patch_apply_reject_and_artifact_manifest() {
    let store = temp_store("patch-artifact");
    let project_root = init_gc_git_project("patch-artifact", &store);
    std::fs::create_dir_all(project_root.join("out")).expect("mk out");
    std::fs::write(project_root.join("out/summary.md"), "artifact").expect("artifact");
    assert!(Command::new("git")
        .arg("-C")
        .arg(&project_root)
        .args(["add", "-A"])
        .status()
        .expect("git add artifact")
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(&project_root)
        .args(["commit", "-m", "artifact seed"])
        .status()
        .expect("git commit artifact")
        .success());

    let new_file_diff = |path: &str, content: &str| {
        format!(
                "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{content}\n"
            )
    };
    let mk_step = |label: &str, path: &str, content: &str| workflow::StepResult {
        phase: "develop".into(),
        label: label.into(),
        provider: "codex".into(),
        isolation: Some("worktree".into()),
        ok: true,
        output_summary: format!("{label} wrote {path}"),
        step_id: None,
        started_at: None,
        details: Some(serde_json::json!({
            "worktree_diff": new_file_diff(path, content),
            "persist_changes": "patch",
            "owned_paths": ["src"],
            "writable": true,
        })),
        structured: None,
        ordinal: None,
    };
    let run = WorkflowRun {
        id: generated_id("wfrun"),
        workflow_name: "patch-artifact-test".into(),
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
        design_intent: Some("test writable patch and artifact manifest path".into()),
        spec: None,
        host_pid: None,
        dry_run: false,
        terminal_reason: None,
        partial_output_available: false,
    };
    let outcome = workflow::WorkflowOutcome {
        steps: vec![
            mk_step("writer", "src/generated.txt", "hello"),
            mk_step("reject-me", "src/rejected.txt", "bad"),
        ],
        status: WorkflowRunStatus::Completed,
        summary: "patch artifact completed".into(),
        agents_spawned: 2,
        final_output: Some(serde_json::json!({
            "result": null,
            "steps": [],
            "logs": [],
            "patch_actions": [
                { "action": "reject", "label": "reject-me", "reason": "review failed" }
            ],
            "artifact_manifests": [
                { "paths": ["summary.md"], "artifact_root": "out", "write_roots": ["out"] }
            ],
            "verdict": { "ok": true, "reason": "test" },
        })),
    };

    let value = journal_workflow_outcome(&store, run, &outcome).expect("journal");
    assert_eq!(value["patches"].as_array().expect("patches").len(), 2);
    let patches = latest_workflow_patches_in_append_order(&store).expect("patches");
    let writer = patches
        .iter()
        .find(|patch| patch.label == "writer")
        .expect("writer patch")
        .clone();
    assert_eq!(writer.status, WorkflowPatchStatus::PendingApply);
    let rejected = patches
        .iter()
        .find(|patch| patch.label == "reject-me")
        .expect("reject patch");
    assert_eq!(rejected.status, WorkflowPatchStatus::Rejected);

    let applied = apply_workflow_patch_record(
        &store,
        None,
        &writer,
        Some("test".into()),
        Some("manual accept".into()),
        false,
    )
    .expect("apply patch");
    assert_eq!(applied.status, WorkflowPatchStatus::Applied);
    assert_eq!(
        std::fs::read_to_string(project_root.join("src/generated.txt")).expect("applied file"),
        "hello\n"
    );

    let manifests = latest_workflow_artifact_manifests_in_append_order(&store).expect("manifests");
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].status, WorkflowArtifactManifestStatus::Current);
    assert_eq!(manifests[0].files[0].path, "out/summary.md");

    let snapshot = dashboard_snapshot(&store).expect("snapshot");
    assert_eq!(snapshot["workflow_patches"].as_array().unwrap().len(), 2);
    assert_eq!(
        snapshot["workflow_artifact_manifests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let _ = std::fs::remove_dir_all(&project_root);
    let _ = std::fs::remove_dir_all(store.root());
}
