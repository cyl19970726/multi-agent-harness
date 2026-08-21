use super::*;

#[test]
fn workflow_run_rejects_a_different_explicit_project_binding() {
    let store = temp_store("pinned-binding-conflict");
    store
        .append_workflow_run(&WorkflowRun {
            id: "wfrun-pinned".into(),
            workflow_name: "demo".into(),
            project_binding_id: Some("binding-a".into()),
            status: WorkflowRunStatus::Running,
            step_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            ended_at: None,
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: None,
            design_intent: None,
            spec: None,
            host_pid: None,
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        })
        .expect("append workflow run");
    let conflicting = ProjectContext {
        id: "binding-b".into(),
        project_root: std::env::temp_dir(),
        store_root: store.root().to_path_buf(),
        kind: ProjectKind::Repo,
        is_git_repo: false,
    };

    let error = workflow_project_context_for_run(&store, "wfrun-pinned", Some(&conflicting))
        .expect_err("a selected binding must not override durable run identity");
    assert!(error.to_string().contains(
        "workflow run wfrun-pinned is pinned to Project Binding binding-a, not binding-b"
    ));
}
