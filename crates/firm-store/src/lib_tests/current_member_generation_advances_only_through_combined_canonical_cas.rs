use super::*;

#[test]
fn current_member_generation_advances_only_through_combined_canonical_cas() {
    let (root, store, run, member, _) = work_test_fixture("member-generation-combined");
    let mut next = member.clone();
    next.runtime_generation = 2;
    next.status = MemberRunStatus::Queued;
    next.started_at = "unix-ms:2".into();
    next.last_event_at = Some("unix-ms:2".into());

    let legacy_before = std::fs::read(root.join("member_runs.jsonl")).unwrap();
    let canonical_before = std::fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap();
    let rejected = store
        .compare_and_append_member_run(&member, &next)
        .expect_err("generic projection CAS cannot advance generation");
    assert!(rejected
        .to_string()
        .contains("MEMBER_GENERATION_TRANSITION_AUTHORITY_REQUIRED"));
    assert_eq!(
        std::fs::read(root.join("member_runs.jsonl")).unwrap(),
        legacy_before
    );
    assert_eq!(
        std::fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap(),
        canonical_before
    );

    store
        .compare_and_advance_member_run_generation(&member, &next)
        .expect("combined Store authority advances both projections");
    assert_eq!(
        store
            .trust_member_runs("unit-test-space")
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == member.id)
            .unwrap()
            .runtime_generation,
        2
    );
    assert_eq!(
        store
            .member_runs()
            .unwrap()
            .into_iter()
            .rev()
            .find(|candidate| candidate.id == member.id)
            .unwrap()
            .runtime_generation,
        2
    );
    assert_eq!(
        store.current_team_run_execution_space(&run).unwrap(),
        "unit-test-space"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
