use super::*;

#[test]
#[ignore = "legacy Work acceptance route is retired; exact replacement: member_execution_trust::canonical_acceptance_rolls_up_delegation_in_the_same_operation"]
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
            host_work_context(
                "delegation-create-rollup",
                "delegate-source-rollup",
                "unix-ms:3",
            ),
        )
        .expect("create Delegation");
    let started = store
        .start_work(
            &target.id,
            target.version,
            &member_b.id,
            member_work_context(
                &member_b.id,
                "target-start",
                "target-start-command",
                "unix-ms:4",
            ),
        )
        .expect("start target");
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
    assert!(store
        .transition_work_and_roll_up_delegation(
            &target.id,
            host_work_context("rollup-blocked", "rollup-blocked-command", "unix-ms:6"),
        )
        .expect("already-atomic blocker reconciliation is a no-op")
        .is_empty());

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
    let resumed_rollup = store
        .latest_work_delegations()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == delegation.id)
        .expect("atomic resumed rollup");
    assert_eq!(resumed_rollup.state, WorkDelegationState::Active);

    let submitted = store
        .submit_work(
            &target.id,
            resumed.version,
            &member_b.id,
            "target result ready",
            vec!["artifact://target".into()],
            vec!["check://target".into()],
            member_work_context(
                &member_b.id,
                "target-submit",
                "target-submit-command",
                "unix-ms:9",
            ),
        )
        .expect("submit target");
    let accepted = store
        .accept_work(
            &target.id,
            submitted.version,
            host_work_context("target-accept", "target-accept-command", "unix-ms:10"),
        )
        .expect("accept target");
    let completed = store
        .latest_work_delegations()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == delegation.id)
        .expect("atomic completed rollup");
    assert_eq!(completed.state, WorkDelegationState::Completed);
    assert_eq!(completed.version, delegation.version + 3);
    assert_eq!(
        completed.resolution_summary.as_deref(),
        accepted.result_summary.as_deref()
    );
    assert!(store
        .transition_work_and_roll_up_delegation(
            &target.id,
            host_work_context(
                "rollup-completed-retry",
                "rollup-completed-retry-command",
                "unix-ms:12",
            ),
        )
        .expect("terminal rollup retry is a no-op")
        .is_empty());
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
