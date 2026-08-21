use super::*;

#[test]
fn append_and_read_team_run_jsonl_rejects_legacy_sparse_rows() {
    let root = team_test_root("team-run");
    let store = HarnessStore::new(&root);
    let run = AgentTeamRun {
        id: "tr-1".into(),
        agent_team_id: "td-1".into(),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-example".into(),
        previous_run_id: Some("tr-0".into()),
        host_surface: "codex-app".into(),
        host_thread_id: Some("thread-1".into()),
        host_actor: None,
        host_control_mode: Default::default(),
        objective: "Ship the feature".into(),
        execution_root: Some("/projects/example/worktrees/feature".into()),
        status: TeamRunStatus::Running,
        member_run_ids: vec!["mr-1".into()],
        budget_limit_usd: Some(12.5),
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:2".into(),
        completed_at: None,
    };

    store
        .legacy_import_append_team_run_projection(&run)
        .expect("append team run");
    // Required Team/Node/Project authority makes legacy sparse rows unreadable
    // after the clean cutover.
    append_sparse_row(
        &root,
        "team_runs.jsonl",
        r#"{"id":"tr-sparse","host_surface":"kimi-cli","objective":"obj","status":"planning","created_at":"unix-ms:3","updated_at":"unix-ms:3"}"#,
    );

    let error = store
        .team_runs()
        .expect_err("legacy sparse TeamRun must not compatibility-read");
    assert!(matches!(error, StoreError::Json(_)));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
