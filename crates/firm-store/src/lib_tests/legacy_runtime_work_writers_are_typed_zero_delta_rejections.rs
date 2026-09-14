use super::*;

#[test]
fn legacy_runtime_work_writers_are_typed_zero_delta_rejections() {
    let (root, store, run, member, _) = work_test_fixture("legacy-runtime-work-writers");
    let before = store
        .work_record_operations_unlocked()
        .expect("operations before")
        .len();

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
    assert_eq!(
        store.work_record_operations_unlocked().unwrap().len(),
        before
    );

    // Stage the historical shape the way a pre-cutover binary stored it: a
    // `work_operations.jsonl` row whose Work carries runtime ownership. Since
    // W4 no current writer can produce that row, so the fixture writes it.
    let mut historical = unassigned_test_work(&run.id, "legacy-runtime-row");
    historical.version = 1;
    historical.created_at = "unix-ms:3".into();
    historical.updated_at = "unix-ms:3".into();
    historical.accountable_team_id = Some(run.agent_team_id.clone());
    historical.owner_member_id = Some(member.agent_member_id.clone());
    historical.active_member_run_id = Some(member.id.clone());
    historical.assignee_membership_id = None;
    let canonical = historical.clone();
    let historical_context =
        host_work_context("canonical-create-event", "canonical-create", "unix-ms:3");
    let operation = WorkOperation {
        event: WorkEvent {
            id: historical_context.event_id.clone(),
            team_run_id: run.id.clone(),
            work_id: canonical.id.clone(),
            sequence: 1,
            kind: WorkEventKind::Created,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: historical_context.performed_by_actor.clone(),
            authority_actor: historical_context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: historical_context.idempotency_key.clone(),
            payload: serde_json::Value::Null,
            created_at: historical_context.created_at.clone(),
            executed_by_member_run_id: None,
        },
        work: historical,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
    };
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .append_legacy_work_operation_unlocked(&operation)
            .expect("stage historical runtime-owned Work row");
    }
    let legacy_before = store.work_record_operations_unlocked().unwrap().len();
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
        assert_eq!(
            store.work_record_operations_unlocked().unwrap().len(),
            legacy_before
        );
    }

    std::fs::remove_dir_all(root).expect("remove temp store");
}
