use super::*;

#[test]
fn legacy_assignment_message_is_ignored_by_current_work_store() {
    let (root, store, run, _, _) = work_test_fixture("legacy-work-store");
    append_sparse_row(
        &root,
        "team_messages.jsonl",
        &format!(
            r#"{{"id":"legacy-assignment","team_run_id":"{}","sender_runtime_id":"host","kind":"assignment","body":"legacy","correlation_id":"legacy","created_at":"unix-ms:1"}}"#,
            run.id
        ),
    );
    let legacy_path = root.join("team_messages.jsonl");
    let legacy_before = std::fs::read(&legacy_path).expect("read legacy message history");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-current"),
            host_work_context("we-current", "create-current", "unix-ms:2"),
        )
        .expect("retired TeamMessage history must not gate current Work");
    assert_eq!(created.id, "work-current");
    assert_eq!(
        std::fs::read(&legacy_path).expect("re-read legacy message history"),
        legacy_before,
        "current Work mutation must not reinterpret or rewrite legacy TeamMessage rows"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
