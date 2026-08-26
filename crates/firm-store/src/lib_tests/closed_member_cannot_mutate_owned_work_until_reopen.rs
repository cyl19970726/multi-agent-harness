use super::*;

#[test]
fn closed_member_cannot_mutate_owned_work_until_reopen() {
    let (root, store, run, member, _) = work_test_fixture("closed-member-owned-work");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-owned-closed"),
            host_work_context("we-closed-1", "create-owned", "unix-ms:2"),
        )
        .expect("create Work");
    let assigned = store
        .assign_work(
            &created.id,
            created.version,
            &member.id,
            host_work_context("we-closed-2", "assign-owned", "unix-ms:3"),
        )
        .expect("assign Work");
    let started = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "we-closed-3",
        "start-owned",
        "unix-ms:4",
    );

    // Close lands mid-execution: coordination flips Closed while the Work
    // stays owned and InProgress.
    let mut closed_member = member.clone();
    closed_member.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    closed_member.status = MemberRunStatus::Stopped;
    store
        .compare_and_append_member_run(&member, &closed_member)
        .expect("record closed coordination");

    let blocked = store
        .block_work(
            &started.id,
            started.version,
            &member.id,
            "still blocked",
            member_work_context(&member.id, "we-closed-4", "block-owned", "unix-ms:5"),
        )
        .expect_err("closed member cannot block owned Work");
    assert!(
        blocked
            .to_string()
            .contains("does not hold active Work responsibility"),
        "unexpected error: {blocked}"
    );
    let submitted = store
        .submit_work(
            &started.id,
            started.version,
            &member.id,
            "result from a closed runtime",
            Vec::new(),
            Vec::new(),
            member_work_context(&member.id, "we-closed-5", "submit-owned", "unix-ms:6"),
        )
        .expect_err("closed member cannot submit owned Work");
    assert!(
        submitted
            .to_string()
            .contains("does not hold active Work responsibility"),
        "unexpected error: {submitted}"
    );
    // The Work projection is untouched by both rejections.
    let current = store
        .latest_works()
        .expect("latest works")
        .into_iter()
        .find(|work| work.id == started.id)
        .expect("owned Work");
    assert_eq!(current.phase, WorkPhase::Active);
    assert_eq!(current.version, started.version);

    // Reopen (coordination Active, next runtime generation) restores the
    // member-side transition path for the same durable Work.
    let mut reopened_member = closed_member.clone();
    reopened_member.coordination_status = firm_core::MemberCoordinationStatus::Active;
    reopened_member.status = MemberRunStatus::Idle;
    reopened_member.runtime_generation += 1;
    store
        .compare_and_advance_member_run_generation(&closed_member, &reopened_member)
        .expect("record reopened member");
    let submitted = store
        .submit_work(
            &started.id,
            started.version,
            &member.id,
            "result after reopen",
            Vec::new(),
            Vec::new(),
            member_work_context(&member.id, "we-closed-6", "submit-reopened", "unix-ms:7"),
        )
        .expect("reopened member submits owned Work");
    assert_eq!(submitted.phase, WorkPhase::Review);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
