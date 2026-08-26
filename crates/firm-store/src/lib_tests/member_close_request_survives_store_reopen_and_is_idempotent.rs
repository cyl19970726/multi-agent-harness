use super::*;

#[test]
fn member_close_request_survives_store_reopen_and_is_idempotent() {
    let root = team_test_root("durable-member-close");
    let store = HarnessStore::new(&root);
    let run = AgentTeamRun {
        id: "tr-close".into(),
        agent_team_id: "team-close".into(),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "codex-app".into(),
        host_thread_id: Some("thread-close".into()),
        host_actor: None,
        host_control_mode: Default::default(),
        objective: "close once".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: vec!["mr-close".into()],
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    let member = ProviderRuntimeProjection {
        id: "mr-close".into(),
        team_run_id: run.id.clone(),
        slot_id: None,
        agent_member_id: "agent-mr-close".into(),
        name: "Builder".into(),
        role: "builder".into(),
        provider: "codex".into(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: Default::default(),
        runtime_generation: 1,
        status: MemberRunStatus::Running,
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
    seed_current_team_run_fixture(&store, &run, std::slice::from_ref(&member));

    let request = TeamMemberCloseRequest {
        id: "close-1".into(),
        team_run_id: run.id.clone(),
        member_run_id: member.id.clone(),
        requested_by: "host".into(),
        reason: "accepted".into(),
        status: TeamMemberCloseStatus::Pending,
        requested_at: "unix-ms:2".into(),
        applied_at: None,
        detached_recovery_fence: None,
    };
    let latched = store
        .latch_team_member_close(&request)
        .expect("latch Close");
    let repeated = store
        .latch_team_member_close(&TeamMemberCloseRequest {
            id: "close-duplicate".into(),
            ..request.clone()
        })
        .expect("repeat Close");
    assert_eq!(latched.id, repeated.id);

    let mut forged_recovery = request.clone();
    forged_recovery.id = "close-forged-recovery".into();
    forged_recovery.detached_recovery_fence =
        Some(Box::new(firm_core::DetachedRecoveryCloseFence {
            execution_space_id: "space-forged".into(),
            member_run_generation: 1,
            agent_session_id: "session-forged".into(),
            agent_session_generation: 1,
            agent_session_version: 1,
            agent_session_driver_generation: 1,
            native_session_id: "native-forged".into(),
            node_daemon_id: "daemon-forged".into(),
            node_daemon_generation: 1,
            authorizing_supervisor_id: "supervisor-forged".into(),
            authorizing_supervisor_generation: 1,
        }));
    let forged_error = store
        .latch_team_member_close(&forged_recovery)
        .expect_err("generic Close writer must reject a recovery authority fence");
    assert!(forged_error
        .to_string()
        .contains("DETACHED_RECOVERY_CLOSE_REQUIRES_SUPERVISOR_AUTHORITY"));
    let no_supervisor_error = store
        .latch_team_member_close_without_current_supervisor(&forged_recovery, 1)
        .expect_err("no-Supervisor Close writer must reject a recovery authority fence");
    assert!(no_supervisor_error
        .to_string()
        .contains("DETACHED_RECOVERY_CLOSE_REQUIRES_SUPERVISOR_AUTHORITY"));
    assert_eq!(
        store
            .team_member_close_requests()
            .expect("Close rows after rejected generic recovery write"),
        vec![latched.clone()],
        "rejected generic recovery fence must append no row"
    );

    let reopened = HarnessStore::new(&root);
    let pending = reopened
        .latest_team_member_close_request(&member.id)
        .expect("read Close after reopen")
        .expect("durable Close");
    assert_eq!(pending.status, TeamMemberCloseStatus::Pending);
    let applied = reopened
        .complete_team_member_close(&run.id, &member.id, &pending.id, "unix-ms:3")
        .expect("apply Close");
    assert_eq!(applied.status, TeamMemberCloseStatus::Applied);
    assert_eq!(applied.applied_at.as_deref(), Some("unix-ms:3"));
    let applied_again = reopened
        .complete_team_member_close(&run.id, &member.id, &pending.id, "unix-ms:4")
        .expect("Close apply is idempotent");
    assert_eq!(applied_again.applied_at.as_deref(), Some("unix-ms:3"));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
