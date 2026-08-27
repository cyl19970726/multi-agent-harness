use super::*;

#[test]
fn legacy_runtime_work_writers_are_typed_zero_delta_rejections() {
    let (root, store, run, member, _) = work_test_fixture("legacy-runtime-work-writers");
    let before = store.work_operations().expect("operations before").len();

    let mut legacy = unassigned_test_work(&run.id, "legacy-runtime-create");
    legacy.owner_member_id = Some(member.agent_member_id.clone());
    legacy.active_member_run_id = Some(member.id.clone());
    let create_error = store
        .insert_work(
            legacy,
            host_work_context("legacy-create-event", "legacy-create", "unix-ms:2"),
        )
        .expect_err("legacy runtime-owned Work creation is retired");
    assert!(create_error
        .to_string()
        .contains("LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED"));
    assert_eq!(store.work_operations().unwrap().len(), before);

    let canonical = store
        .insert_work(
            unassigned_test_work(&run.id, "legacy-runtime-row"),
            host_work_context("canonical-create-event", "canonical-create", "unix-ms:3"),
        )
        .expect("create canonical Work before simulating historical storage");
    let ledger = root.join("work_operations.jsonl");
    let rewritten = std::fs::read_to_string(&ledger)
        .expect("read Work ledger")
        .lines()
        .map(|line| {
            let mut row: serde_json::Value = serde_json::from_str(line).expect("Work row");
            if row["work"]["id"] == canonical.id {
                row["work"]["owner_member_id"] =
                    serde_json::Value::String(member.agent_member_id.clone());
                row["work"]["active_member_run_id"] = serde_json::Value::String(member.id.clone());
                row["work"]["assignee_membership_id"] = serde_json::Value::Null;
            }
            serde_json::to_string(&row).expect("serialize historical Work row")
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&ledger, format!("{rewritten}\n")).expect("write historical Work row");
    let legacy_before = store.work_operations().unwrap().len();
    for error in [
        store
            .start_work(
                &canonical.id,
                canonical.version,
                &member.id,
                member_work_context(&member.id, "legacy-start", "legacy-start", "unix-ms:4"),
            )
            .expect_err("historical runtime owner cannot Start"),
        store
            .release_work(
                &canonical.id,
                canonical.version,
                &member.id,
                member_work_context(&member.id, "legacy-release", "legacy-release", "unix-ms:6"),
            )
            .expect_err("historical runtime owner cannot Release"),
        store
            .retarget_work_execution(
                &canonical.id,
                canonical.version,
                "successor-run-does-not-matter",
                host_work_context("legacy-retarget", "legacy-retarget", "unix-ms:7"),
            )
            .expect_err("historical runtime owner cannot retarget"),
    ] {
        assert!(
            error
                .to_string()
                .contains("LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED")
                || error
                    .to_string()
                    .contains("does not hold active Work responsibility")
                || error.to_string().contains("does not hold responsibility"),
            "unexpected legacy rejection: {error}"
        );
        assert_eq!(store.work_operations().unwrap().len(), legacy_before);
    }

    std::fs::remove_dir_all(root).expect("remove temp store");
}
