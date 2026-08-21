use super::*;

#[test]
fn unavailable_members_and_idempotency_key_reuse_are_rejected() {
    let (root, store, run, member, _) = work_test_fixture("work-command-guards");
    let first = store
        .insert_work(
            unassigned_test_work(&run.id, "work-idempotent-a"),
            host_work_context("we-guard-1", "shared-key", "unix-ms:2"),
        )
        .expect("first command");
    let other_work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-idempotent-b"),
            host_work_context("ignored", "shared-key", "unix-ms:3"),
        )
        .expect_err("same key cannot identify a different Work");
    assert!(other_work.to_string().contains("IDEMPOTENCY_CONFLICT"));
    let other_command = store
        .assign_work(
            &first.id,
            first.version,
            &member.id,
            host_work_context("ignored", "shared-key", "unix-ms:4"),
        )
        .expect_err("same key cannot identify a different command");
    assert!(other_command.to_string().contains("IDEMPOTENCY_CONFLICT"));

    let mut failed_member = member.clone();
    failed_member.status = MemberRunStatus::Failed;
    failed_member.finished_at = Some("unix-ms:5".into());
    store
        .compare_and_append_member_run(&member, &failed_member)
        .expect("record failed member");
    let mut assigned_to_failed = unassigned_test_work(&run.id, "work-failed-member");
    assigned_to_failed.claim_mode = WorkClaimMode::HostAssign;
    assigned_to_failed.active_member_run_id = Some(failed_member.id.clone());
    let failed = store
        .insert_work(
            assigned_to_failed,
            host_work_context("we-guard-2", "create-failed", "unix-ms:6"),
        )
        .expect_err("failed member cannot receive owned Work");
    assert!(failed.to_string().contains("MEMBER_UNAVAILABLE"));

    let mut stopped_member = failed_member.clone();
    stopped_member.status = MemberRunStatus::Stopped;
    store
        .compare_and_append_member_run(&failed_member, &stopped_member)
        .expect("record stopped member");
    let stopped_work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-assign-stopped"),
            host_work_context("we-guard-3", "create-for-stopped", "unix-ms:7"),
        )
        .expect("create unassigned Work");
    let stopped = store
        .assign_work(
            &stopped_work.id,
            stopped_work.version,
            &stopped_member.id,
            host_work_context("we-guard-4", "assign-stopped", "unix-ms:8"),
        )
        .expect_err("stopped member cannot be assigned");
    assert!(stopped.to_string().contains("MEMBER_UNAVAILABLE"));

    let mut closed_member = stopped_member.clone();
    closed_member.status = MemberRunStatus::Idle;
    closed_member.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    store
        .compare_and_append_member_run(&stopped_member, &closed_member)
        .expect("record closed coordination");
    let unassigned = store
        .insert_work(
            unassigned_test_work(&run.id, "work-assign-closed"),
            host_work_context("we-guard-5", "create-unassigned", "unix-ms:9"),
        )
        .expect("create unassigned Work");
    let closed = store
        .assign_work(
            &unassigned.id,
            unassigned.version,
            &closed_member.id,
            host_work_context("we-guard-6", "assign-closed", "unix-ms:10"),
        )
        .expect_err("closed member cannot be assigned");
    assert!(closed.to_string().contains("MEMBER_UNAVAILABLE"));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
