use super::*;

#[test]
fn work_delegation_rolls_up_target_condition_and_resolution_without_mutating_source() {
    let (root, store, run_a, member_a, run_b, member_b) =
        delegation_test_fixture("delegation-rollup");
    let source = insert_assigned_delegation_work(
        &store,
        &run_a,
        &member_a,
        "source-rollup",
        "work-source-rollup",
        "create-source-rollup",
        "unix-ms:2",
    );
    let (delegation, target) = store
        .create_work_delegation_with_target_work(
            delegation_request("delegation-rollup", &source, &run_b.agent_team_id),
            delegation_work(&run_b, "target-rollup"),
            run_host_work_context(
                &run_a,
                "delegation-create-rollup",
                "delegate-source-rollup",
                "unix-ms:3",
            ),
        )
        .expect("create Delegation");
    let assigned = store
        .assign_work_to_membership(
            &target.id,
            target.version,
            &format!(
                "membership:{}:{}",
                run_b.agent_team_id, member_b.agent_member_id
            ),
            "delegation-test-space",
            run_host_work_context(&run_b, "target-assign", "target-assign", "unix-ms:3.5"),
        )
        .expect("assign delegated target responsibility");
    let started = start_claimed_work_for_test(
        &store,
        &assigned,
        &member_b,
        "target-start",
        "target-start-command",
        "unix-ms:4",
    );
    let blocked = store
        .block_work(
            &target.id,
            started.version,
            &member_b.id,
            "waiting for an external contract",
            member_work_context(
                &member_b.id,
                "target-block",
                "target-block-command",
                "unix-ms:5",
            ),
        )
        .expect("block target");
    let blocked_rollup = store
        .latest_work_delegations()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == delegation.id)
        .expect("atomic blocked rollup");
    assert_eq!(blocked_rollup.state, WorkDelegationState::Blocked);
    assert_eq!(
        blocked_rollup.blocker_reason.as_deref(),
        Some("waiting for an external contract")
    );

    let resumed = store
        .resume_work(
            &target.id,
            blocked.version,
            &member_b.id,
            "contract arrived",
            member_work_context(
                &member_b.id,
                "target-resume",
                "target-resume-command",
                "unix-ms:7",
            ),
        )
        .expect("resume target");
    assert_eq!(
        store
            .latest_work_delegations()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == delegation.id)
            .expect("atomic resumed rollup")
            .state,
        WorkDelegationState::Active
    );

    let submitted = submit_started_work_for_test(
        &store,
        &resumed,
        &member_b,
        "target-result",
        "target result ready",
        vec!["artifact://target".into()],
        vec!["check://target".into()],
        "unix-ms:9",
    );
    let accepted = accept_result_for_test(
        &store,
        &submitted,
        "target-result",
        "target-accept-command",
        "unix-ms:10",
    );
    let completed = store
        .latest_work_delegations()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == delegation.id)
        .expect("atomic completed rollup");
    assert_eq!(completed.state, WorkDelegationState::Completed);
    assert_eq!(
        completed.resolution_summary.as_deref(),
        accepted.result_summary.as_deref()
    );
    let source_after = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == source.id)
        .expect("source remains visible");
    assert_eq!(
        source_after, source,
        "target result never mutates source Work"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
