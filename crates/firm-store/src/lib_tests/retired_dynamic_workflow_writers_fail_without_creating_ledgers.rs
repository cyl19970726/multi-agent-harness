use super::*;

#[test]
fn retired_dynamic_workflow_writers_fail_without_creating_ledgers() {
    let root = team_test_root("retired-dynamic-workflow-writers");
    let store = HarnessStore::new(&root);
    let run: WorkflowRun = serde_json::from_value(serde_json::json!({
        "id": "workflow-run-retired",
        "workflow_name": "retired",
        "status": "running",
        "created_at": "unix-ms:1"
    }))
    .expect("historical WorkflowRun shape");
    let error = store
        .append_workflow_run(&run)
        .expect_err("retired workflow writer must reject");
    assert!(error
        .to_string()
        .contains("RETIRED_DYNAMIC_WORKFLOW_WRITER"));

    let step: WorkflowStep = serde_json::from_value(serde_json::json!({
        "id": "workflow-step-retired",
        "run_id": "workflow-run-retired",
        "phase": "retired",
        "label": "retired",
        "status": "running",
        "started_at": "unix-ms:1"
    }))
    .expect("historical WorkflowStep shape");
    let patch: WorkflowPatch = serde_json::from_value(serde_json::json!({
        "id": "workflow-patch-retired",
        "run_id": "workflow-run-retired",
        "step_id": "workflow-step-retired",
        "label": "retired",
        "phase": "retired",
        "provider": "codex",
        "status": "pending_apply",
        "patch_ref": "workflow-patches/run/step.patch",
        "created_at": "unix-ms:1"
    }))
    .expect("historical WorkflowPatch shape");
    let manifest: WorkflowArtifactManifest = serde_json::from_value(serde_json::json!({
        "id": "workflow-artifacts-retired",
        "run_id": "workflow-run-retired",
        "status": "current",
        "created_at": "unix-ms:1"
    }))
    .expect("historical WorkflowArtifactManifest shape");
    for error in [
        store.append_workflow_step(&step).unwrap_err(),
        store.append_workflow_patch(&patch).unwrap_err(),
        store
            .append_workflow_artifact_manifest(&manifest)
            .unwrap_err(),
    ] {
        assert!(error
            .to_string()
            .contains("RETIRED_DYNAMIC_WORKFLOW_WRITER"));
    }

    let delegation = DelegationRun {
        id: "delegation-retired-workflow".into(),
        team_run_id: "tr-1".into(),
        parent_member_run_id: "mr-1".into(),
        parent_task_id: None,
        mode: DelegationMode::DynamicWorkflow,
        provider: "claude".into(),
        provider_child_thread_id: None,
        workflow_run_id: Some("workflow-run-retired".into()),
        objective: "must reject".into(),
        status: DelegationStatus::Planned,
        evidence_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    assert!(store
        .append_delegation_run(&delegation)
        .expect_err("workflow delegation writer must reject")
        .to_string()
        .contains("RETIRED_DYNAMIC_WORKFLOW_WRITER"));
    for ledger in [
        "workflow_runs.jsonl",
        "workflow_steps.jsonl",
        "workflow_patches.jsonl",
        "workflow_artifact_manifests.jsonl",
        "delegation_runs.jsonl",
    ] {
        assert!(!root.join(ledger).exists(), "unexpected ledger: {ledger}");
    }
    if root.exists() {
        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}
