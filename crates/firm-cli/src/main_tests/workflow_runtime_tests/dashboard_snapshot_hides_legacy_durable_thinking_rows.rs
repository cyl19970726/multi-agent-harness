use super::*;

#[test]
fn dashboard_snapshot_hides_legacy_durable_thinking_rows() {
    let store = temp_store("snapshot-no-thinking");
    let legacy = MemberAction {
        id: generated_id("mact"),
        seq: 1,
        team_run_id: "team-run-legacy".to_string(),
        member_run_id: "member-run-legacy".to_string(),
        task_id: None,
        provider_call_id: None,
        action_type: "thinking".to_string(),
        status: MemberActionStatus::Succeeded,
        provider_status: None,
        semantic_status: None,
        title: "old reasoning".to_string(),
        summary: "must remain only in the legacy ledger".to_string(),
        evidence_refs: Vec::new(),
        started_at: now_string(),
        completed_at: Some(now_string()),
    };
    append_jsonl_value(
        &store.root().join("member_actions.jsonl"),
        &serde_json::to_value(&legacy).expect("serialize Legacy member action fixture"),
    )
    .expect("append legacy row");

    assert_eq!(store.member_actions().expect("raw ledger").len(), 1);
    let snapshot = dashboard_snapshot(&store).expect("snapshot");
    assert_eq!(
        snapshot["member_actions"].as_array().map(Vec::len),
        Some(0),
        "legacy thinking must not be projected as product state"
    );
}
