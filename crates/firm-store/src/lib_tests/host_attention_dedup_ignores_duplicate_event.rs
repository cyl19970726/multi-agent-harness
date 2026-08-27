use super::*;

#[test]
fn host_attention_dedup_ignores_duplicate_event() {
    let (root, store, run, member, _) = work_test_fixture("work-dedup-ha");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-dedup-ha-1"),
            host_work_context("we-dedup-1", "create-dedup-ha", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &work.id,
            work.version,
            &member.id,
            member_work_context(&member.id, "we-dedup-2", "claim-dedup-ha", "unix-ms:3"),
        )
        .expect("claim Work");
    let claimed = start_claimed_work_for_test(
        &store,
        &claimed,
        &member,
        "we-dedup-start",
        "start-dedup-ha",
        "unix-ms:3.5",
    );
    let _submitted = submit_started_work_for_test(
        &store,
        &claimed,
        &member,
        "we-dedup-3",
        "done",
        vec!["artifact://x".into()],
        Vec::new(),
        "unix-ms:4",
    );
    let report = result_report_for_test(
        &claimed,
        &member,
        "we-dedup-3",
        "done",
        vec!["artifact://x".into()],
        Vec::new(),
        "unix-ms:4",
    );
    store
        .create_trust_work_report(
            &firm_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: report.authored_by.clone(),
                authority_actor: None,
                command_name: "test.work_report.create".into(),
                idempotency_key: report.id.clone(),
                expected_version: 0,
                request_fingerprint: None,
            },
            claimed.accountable_team_id.as_deref().expect("team id"),
            report,
        )
        .expect("idempotent canonical Result replay");
    let attentions = store.host_attentions().expect("host attentions");
    let review_count = attentions
        .iter()
        .filter(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested)
        .count();
    assert_eq!(
        review_count, 1,
        "dedup must emit exactly one WorkReviewRequested"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
