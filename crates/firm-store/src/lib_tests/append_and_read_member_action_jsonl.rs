use super::*;

#[test]
fn append_and_read_member_action_jsonl() {
    let root = team_test_root("member-action");
    let store = HarnessStore::new(&root);
    let action = MemberAction {
        id: "ma-1".into(),
        seq: 7,
        team_run_id: "tr-1".into(),
        member_run_id: "mr-1".into(),
        task_id: Some("task-1".into()),
        provider_call_id: Some("tool-1".into()),
        action_type: "tool_completed".into(),
        status: MemberActionStatus::Succeeded,
        provider_status: Some("completed".into()),
        semantic_status: Some("succeeded".into()),
        title: "cargo test".into(),
        summary: "all green".into(),
        evidence_refs: vec!["ev-1".into()],
        started_at: "unix-ms:1".into(),
        completed_at: Some("unix-ms:2".into()),
    };

    // Raw serialization compatibility is a Legacy diagnostic fixture, not
    // a current MemberAction authorization path.
    store
        .append_jsonl("member_actions.jsonl", &action)
        .expect("append legacy member action fixture");
    append_sparse_row(
        &root,
        "member_actions.jsonl",
        r#"{"id":"ma-sparse","seq":8,"team_run_id":"tr-1","member_run_id":"mr-1","action_type":"blocked","status":"started","title":"t","summary":"s","started_at":"unix-ms:3"}"#,
    );

    let actions = store.member_actions().expect("read member actions");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0], action);
    let sparse = &actions[1];
    assert_eq!(sparse.id, "ma-sparse");
    assert_eq!(sparse.seq, 8);
    assert!(sparse.task_id.is_none());
    assert!(sparse.evidence_refs.is_empty());
    assert!(sparse.completed_at.is_none());

    std::fs::remove_dir_all(root).expect("remove temp store");
}
