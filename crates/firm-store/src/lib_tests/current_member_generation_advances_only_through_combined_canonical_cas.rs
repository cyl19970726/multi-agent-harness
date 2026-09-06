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

#[test]
fn member_dual_ledger_failure_names_both_records_and_restart_fails_closed() {
    let (root, store, run, member, _) = work_test_fixture("member-dual-ledger-crash");
    let canonical_before = std::fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap();
    let mut next = member.clone();
    next.runtime_generation += 1;
    next.status = MemberRunStatus::Queued;
    next.started_at = "unix-ms:2".into();
    next.last_event_at = Some("unix-ms:2".into());
    // Fail the second physical write after the MemberRun projection append.
    std::fs::create_dir(root.join("agentfirm_trust_operations.jsonl.next")).unwrap();
    let error = store
        .compare_and_advance_member_run_generation(&member, &next)
        .expect_err("canonical write fails")
        .to_string();
    assert!(
        error.contains("MEMBER_RUN_DUAL_LEDGER_COMMIT_INCOMPLETE"),
        "{error}"
    );
    assert!(
        error.contains(&member.id) && error.contains("runtime_generation=2"),
        "{error}"
    );
    assert!(error.contains("canonical settlement unknown"), "{error}");
    assert!(
        error.contains("member_runs.jsonl") && error.contains("agentfirm_trust_operations.jsonl")
    );
    assert_eq!(
        std::fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap(),
        canonical_before
    );
    let restarted = HarnessStore::new(&root);
    assert_eq!(
        restarted
            .member_runs()
            .unwrap()
            .last()
            .unwrap()
            .runtime_generation,
        2
    );
    let refusal = restarted
        .current_team_run_execution_space(&run)
        .expect_err("restarted admission detects divergence")
        .to_string();
    assert!(
        refusal.contains("MEMBER_RUN_MATERIALIZATION_MISMATCH"),
        "{refusal}"
    );
    assert_eq!(
        std::fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap(),
        canonical_before,
        "detection never silently repairs authoritative history"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn current_member_scope_scan_stays_decode_free_as_real_history_grows() {
    let (root, store, run, mut member, _) = work_test_fixture("scope-scan-growth");
    let mut revisions = 0;
    for target in [10, 100, 500] {
        while revisions < target {
            let mut next = member.clone();
            next.last_event_at = Some(format!("unix-ms:scan-{}", revisions + 2));
            store.compare_and_append_member_run(&member, &next).unwrap();
            member = next;
            revisions += 1;
        }
        // First post-write read honestly pays for trust's atomic replacement.
        let rebuild_started = std::time::Instant::now();
        store.current_team_run_execution_space(&run).unwrap();
        let rebuild_elapsed = rebuild_started.elapsed();
        let before = store.read_scan_metrics();
        let started = std::time::Instant::now();
        for _ in 0..20 {
            assert_eq!(
                store.current_team_run_execution_space(&run).unwrap(),
                "unit-test-space"
            );
        }
        let elapsed = started.elapsed();
        let after = store.read_scan_metrics();
        for metric in &after {
            if let Some(previous) = before.iter().find(|m| m.ledger == metric.ledger) {
                assert_eq!(
                    metric.total_decoded_rows, previous.total_decoded_rows,
                    "{} still decodes idle history",
                    metric.ledger
                );
                assert_eq!(
                    metric.total_bytes_read, previous.total_bytes_read,
                    "{} still reads idle history",
                    metric.ledger
                );
            }
        }
        println!(
            "{}",
            serde_json::json!({
                "real_member_revisions": target, "idle_passes": 20,
                "idle_elapsed_us": elapsed.as_micros(), "first_post_write_elapsed_us": rebuild_elapsed.as_micros(),
                "metrics": after,
                "limitation": "first post-write observation rebuilds atomic trust replacement; append prefix bytes scale with history"
            })
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
