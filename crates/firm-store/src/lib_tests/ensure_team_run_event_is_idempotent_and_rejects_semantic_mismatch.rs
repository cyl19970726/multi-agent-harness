use super::*;

#[test]
fn ensure_team_run_event_is_idempotent_and_rejects_semantic_mismatch() {
    let root = team_test_root("ensure-team-run-event");
    let store = HarnessStore::new(&root);
    let host = ProviderRuntimeProjection {
        id: "mr-event-host".into(),
        team_run_id: "tr-1".into(),
        slot_id: Some("slot-event-host".into()),
        agent_member_id: "agent-event-host".into(),
        name: "Event Host".into(),
        role: "host".into(),
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
        zero_output_streak: 0,
        last_consumed_work_version: None,
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
    };
    seed_current_team_run_fixture(
        &store,
        &AgentTeamRun {
            id: "tr-1".into(),
            agent_team_id: "team-event-idempotency".into(),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "test".into(),
            host_thread_id: None,
            host_actor: Some(TeamActorRef {
                kind: TeamActorKind::Host,
                id: host.agent_member_id.clone(),
                display_name: None,
                authn_source: Some("test_team_membership:host".into()),
            }),
            host_control_mode: firm_core::HostControlMode::Managed,
            objective: "event idempotency".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![host.id.clone()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        },
        std::slice::from_ref(&host),
    );
    let event = TeamRunEvent {
        id: "caller-id-is-ignored".into(),
        seq: 0,
        team_run_id: "tr-1".into(),
        source_kind: TeamRunEventSourceKind::Host,
        member_run_id: None,
        delegation_run_id: None,
        entity_type: "host_attention".into(),
        entity_id: "attention-1".into(),
        operation: "dispatch_ready".into(),
        summary: "attention-1 actionable attempt 0".into(),
        occurred_at: "unix-ms:1".into(),
    };
    let first = store
        .ensure_team_run_event_next("tr-1:attention-1:actionable:0", event.clone())
        .expect("first event");
    let mut retry = event.clone();
    retry.occurred_at = "unix-ms:2".into();
    let second = store
        .ensure_team_run_event_next("tr-1:attention-1:actionable:0", retry)
        .expect("same causal transition");
    assert_eq!(first, second);
    assert_eq!(store.legacy_team_run_events().unwrap().len(), 1);

    let mut mismatch = event;
    mismatch.summary = "different causal meaning".into();
    assert!(matches!(
        store.ensure_team_run_event_next("tr-1:attention-1:actionable:0", mismatch),
        Err(StoreError::Conflict(_))
    ));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
