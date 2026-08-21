use super::*;

#[test]
fn legacy_raw_work_operation_is_rejected_without_a_read_fallback() {
    let root = team_test_root("legacy-raw-work-operation");
    let store = HarnessStore::new(&root);
    store.init().expect("initialize legacy replay store");
    let raw_operation = serde_json::json!({
        "event": {
            "id": "work-event-legacy-raw-1",
            "team_run_id": "team-run-legacy-raw-1",
            "work_id": "work-legacy-raw-1",
            "sequence": 1,
            "kind": "created",
            "expected_version": 0,
            "resulting_version": 1,
            "performed_by_actor": { "kind": "host", "id": "host" },
            "idempotency_key": "create-work-legacy-raw-1",
            "created_at": "unix-ms:1"
        },
        "work": {
            "id": "work-legacy-raw-1",
            "team_run_id": "team-run-legacy-raw-1",
            "title": "Replay a historical WorkOperation",
            "context_markdown": "Raw JSONL compatibility row",
            "completion_criteria_markdown": "Both Store projections remain readable",
            "status": "open",
            "claim_mode": "team_claim",
            "priority": "normal",
            "created_by_actor": { "kind": "host", "id": "host" },
            "version": 1,
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }
    });
    std::fs::write(
        root.join("work_operations.jsonl"),
        format!("{raw_operation}\n"),
    )
    .expect("write historical WorkOperation bytes");

    let error = store
        .work_operations()
        .expect_err("legacy Work status must not gain a read fallback");
    assert!(
        error.to_string().contains("unknown field `status`"),
        "legacy Work rows must fail with an actionable schema error: {error}"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
