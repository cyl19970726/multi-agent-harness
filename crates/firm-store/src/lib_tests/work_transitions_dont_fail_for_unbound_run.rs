use super::*;

#[test]
fn legacy_unbound_run_cannot_authorize_current_host_work_write() {
    let root = team_test_root("work-unbound-ha");
    let store = HarnessStore::new(&root);
    let run = AgentTeamRun {
        id: "tr-work-unbound-ha".into(),
        agent_team_id: "team-work-unbound-ha".into(),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "codex-app".into(),
        host_thread_id: None,
        host_actor: None,
        host_control_mode: Default::default(),
        objective: "prove unbound graceful".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: vec!["mr-work-unbound-ha".into()],
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    store
        .legacy_import_append_team_run_projection(&run)
        .expect("append unbound run");
    let member = ProviderRuntimeProjection {
        id: "mr-work-unbound-ha".into(),
        team_run_id: run.id.clone(),
        slot_id: Some("slot-unbound".into()),
        agent_member_id: "agent-unbound".into(),
        name: "Member Unbound".into(),
        role: "builder".into(),
        provider: "codex".into(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: Default::default(),
        runtime_generation: 1,
        status: MemberRunStatus::Idle,
        native_session: None,
        provider_cwd_hint: None,
        provider_environment_observation: None,
        owned_paths: Vec::new(),
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    };
    store
        .legacy_import_append_member_run_projection(&member)
        .expect("append member");
    let error = store
        .insert_work(
            unassigned_test_work(&run.id, "work-unbound-ha-1"),
            host_work_context("we-ub-1", "create-ub-ha", "unix-ms:2"),
        )
        .expect_err("legacy unbound run must not authorize a current Work write");
    assert!(
        error
            .to_string()
            .contains("TEAM_RUN_HOST_AUTHORITY_REQUIRED"),
        "unexpected error: {error}"
    );
    assert!(store.latest_works().expect("works").is_empty());
    assert!(store.host_attentions().expect("host attentions").is_empty());
    std::fs::remove_dir_all(root).expect("remove temp store");
}
