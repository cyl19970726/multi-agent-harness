use super::*;

#[test]
fn ensure_team_run_event_is_idempotent_and_rejects_semantic_mismatch() {
    let root = team_test_root("ensure-team-run-event");
    let store = HarnessStore::new(&root);
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
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "event idempotency".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: Vec::new(),
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        },
        &[],
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
