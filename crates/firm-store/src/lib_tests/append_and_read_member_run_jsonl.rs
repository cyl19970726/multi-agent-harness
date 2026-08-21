use super::*;

#[test]
fn append_and_read_member_run_jsonl() {
    let root = team_test_root("member-run");
    let store = HarnessStore::new(&root);
    let member_run = ProviderRuntimeProjection {
        id: "mr-1".into(),
        team_run_id: "tr-1".into(),
        slot_id: Some("slot-1".into()),
        agent_member_id: "agent-worker-1".into(),
        name: "worker-1".into(),
        role: "worker".into(),
        provider: "kimi".into(),
        model: Some("kimi-k2".into()),
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: Default::default(),
        runtime_generation: 1,
        status: MemberRunStatus::Running,
        native_session: None,
        provider_cwd_hint: Some("/projects/example/worktrees/worker-1".into()),
        provider_environment_observation: Some(MemberWorkspaceSnapshot {
            cwd: "/projects/example/worktrees/worker-1".into(),
            project_binding_id: Some("project-example".into()),
            resolution_source: Some("member_worktree".into()),
            git_head: Some("0123456789abcdef".into()),
            git_branch: Some("feature/worker-1".into()),
            instruction_roots: vec!["/projects/example".into()],
            skill_roots: vec!["/projects/example/.agents/skills".into()],
        }),
        owned_paths: vec!["src/".into()],
        started_at: "unix-ms:1".into(),
        last_event_at: Some("unix-ms:2".into()),
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    };

    store
        .legacy_import_append_team_run_projection(&AgentTeamRun {
            id: "tr-1".into(),
            agent_team_id: "team-tr-1".into(),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "test".into(),
            host_thread_id: None,
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "member JSONL test".into(),
            execution_root: None,
            status: TeamRunStatus::Planning,
            member_run_ids: vec![member_run.id.clone(), "mr-sparse".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        })
        .expect("declare initial TeamRun membership");

    store
        .legacy_import_append_member_run_projection(&member_run)
        .expect("append member run");
    append_sparse_row(
        &root,
        "member_runs.jsonl",
        r#"{"id":"mr-sparse","team_run_id":"tr-1","name":"w","role":"worker","provider":"codex","status":"idle","started_at":"unix-ms:3"}"#,
    );

    let error = store.member_runs().expect_err(
        "ProviderRuntimeProjection without agent_member_id must not compatibility-read",
    );
    assert!(matches!(error, StoreError::Json(_)));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
