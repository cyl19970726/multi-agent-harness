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
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "we-closed-2",
        "assign-owned",
        "unix-ms:3",
    );
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
    let closed_report = result_report_for_test(
        &started,
        &member,
        "we-closed-5",
        "result from a closed runtime",
        Vec::new(),
        Vec::new(),
        "unix-ms:6",
    );
    let submitted = store
        .create_trust_work_report(
            &firm_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: closed_report.authored_by.clone(),
                authority_actor: None,
                command_name: "test.work_report.create".into(),
                idempotency_key: closed_report.id.clone(),
                expected_version: 0,
                request_fingerprint: None,
            },
            started.accountable_team_id.as_deref().expect("team id"),
            closed_report,
        )
        .expect_err("closed member cannot submit owned Work");
    assert!(
        submitted
            .to_string()
            .contains("MEMBER_RUN_GENERATION_FENCED"),
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

    // Reopen restores stable participation without replaying the provider
    // effect. The exact same MemberRun and AgentSession lineage may settle the
    // already ProviderReceived Work, but no other Work mutation receives this
    // narrow settlement authority.
    let mut reopened_member = closed_member.clone();
    reopened_member.coordination_status = firm_core::MemberCoordinationStatus::Active;
    reopened_member.status = MemberRunStatus::Idle;
    reopened_member.runtime_generation += 1;
    store
        .compare_and_advance_member_run_generation(&closed_member, &reopened_member)
        .expect("record reopened member");
    let reopened_report = result_report_for_test(
        &started,
        &reopened_member,
        "we-closed-6",
        "result after reopen",
        Vec::new(),
        Vec::new(),
        "unix-ms:7",
    );
    let submitted = store
        .create_trust_work_report(
            &firm_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: reopened_report.authored_by.clone(),
                authority_actor: None,
                command_name: "test.work_report.create".into(),
                idempotency_key: reopened_report.id.clone(),
                expected_version: 0,
                request_fingerprint: None,
            },
            started.accountable_team_id.as_deref().expect("team id"),
            reopened_report,
        )
        .expect("reopened exact session lineage may settle ProviderReceived Work");
    assert_eq!(submitted.projection.work_revision, started.version + 1);
    let current = store
        .latest_works()
        .expect("latest works")
        .into_iter()
        .find(|work| work.id == started.id)
        .expect("submitted Work");
    assert_eq!(current.phase, WorkPhase::Review);
    let binding = store
        .fabric_work_execution_bindings("unit-test-space")
        .expect("bindings")
        .into_iter()
        .find(|binding| binding.work_id == started.id)
        .expect("result binding");
    assert_eq!(
        binding.status,
        firm_core::agentfirm_api::WorkExecutionBindingStatus::Released
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
