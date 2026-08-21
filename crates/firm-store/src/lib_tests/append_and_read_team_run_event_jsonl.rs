use super::*;

#[test]
fn append_and_read_team_run_event_jsonl() {
    let root = team_test_root("team-run-event");
    let store = HarnessStore::new(&root);
    let event = TeamRunEvent {
        id: "tre-1".into(),
        seq: 3,
        team_run_id: "tr-1".into(),
        source_kind: TeamRunEventSourceKind::Member,
        member_run_id: Some("mr-1".into()),
        delegation_run_id: None,
        entity_type: "action".into(),
        entity_id: "ma-1".into(),
        operation: "completed".into(),
        summary: "tool completed".into(),
        occurred_at: "unix-ms:1".into(),
    };

    store
        .legacy_import_append_team_run_event(&event)
        .expect("append team run event");
    append_sparse_row(
        &root,
        "team_run_events.jsonl",
        r#"{"id":"tre-sparse","seq":4,"team_run_id":"tr-1","source_kind":"host","entity_type":"team_run","entity_id":"tr-1","operation":"created","summary":"run started","occurred_at":"unix-ms:3"}"#,
    );

    let events = store
        .legacy_team_run_events()
        .expect("read team run events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], event);
    let sparse = &events[1];
    assert_eq!(sparse.id, "tre-sparse");
    assert_eq!(sparse.source_kind, TeamRunEventSourceKind::Host);
    assert!(sparse.member_run_id.is_none());
    assert!(sparse.delegation_run_id.is_none());

    std::fs::remove_dir_all(root).expect("remove temp store");
}
