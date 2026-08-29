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

    let host = store
        .exact_team_run_host_actor(&run.id)
        .expect("exact Host actor");
    let operations_before_false_reopen = store.canonical_operations().unwrap().len();
    let false_reopen = store
        .compare_and_reopen_member_run_generation(&host, &member, &next)
        .expect_err("active generic recovery cannot impersonate formal Reopen");
    assert!(false_reopen
        .to_string()
        .contains("INVALID_STATE_TRANSITION"));
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        operations_before_false_reopen,
        "rejected active-to-active Reopen must be zero-write"
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

#[test]
fn formal_reopen_writer_is_exact_host_and_closed_generation_fenced() {
    let (root, store, run, member, _) = work_test_fixture("formal-reopen-authority");
    let host = store
        .exact_team_run_host_actor(&run.id)
        .expect("exact Host actor");
    let mut closed = member.clone();
    closed.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    closed.status = MemberRunStatus::Stopped;
    closed.finished_at = Some("unix-ms:2".into());
    store
        .compare_and_append_member_run(&member, &closed)
        .expect("close exact predecessor");

    let mut reopened = closed.clone();
    reopened.runtime_generation += 1;
    reopened.coordination_status = firm_core::MemberCoordinationStatus::Active;
    reopened.status = MemberRunStatus::Queued;
    reopened.started_at = "unix-ms:3".into();
    reopened.last_event_at = Some("unix-ms:3".into());
    reopened.finished_at = None;
    let foreign = firm_core::TeamActorRef {
        kind: firm_core::TeamActorKind::Host,
        id: "foreign-host".into(),
        display_name: None,
        authn_source: Some("test".into()),
    };
    let operations_before_foreign = store.canonical_operations().unwrap().len();
    let foreign_error = store
        .compare_and_reopen_member_run_generation(&foreign, &closed, &reopened)
        .expect_err("foreign Host cannot write formal Reopen evidence");
    assert!(foreign_error
        .to_string()
        .contains("TEAM_RUN_HOST_AUTHORITY_MISMATCH"));
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        operations_before_foreign,
        "foreign formal Reopen must be zero-write"
    );

    store
        .compare_and_reopen_member_run_generation(&host, &closed, &reopened)
        .expect("exact Host may formally Reopen exact closed generation");
    let latest = store
        .trust_member_runs("unit-test-space")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == member.id)
        .unwrap();
    assert_eq!(latest.runtime_generation, reopened.runtime_generation);
    assert_eq!(
        latest.runtime_status,
        firm_core::agentfirm_api::MemberRuntimeStatus::Queued
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
