use super::*;

#[test]
#[ignore = "legacy Work acceptance route is retired; canonical acceptance side effects are covered by member_execution_trust"]
fn work_accept_emits_host_attention_for_bound_run() {
    let (root, store, run, member, _) = work_test_fixture("work-accept-ha");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-accept-ha-1"),
            host_work_context("we-accept-1", "create-accept-ha", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &work.id,
            work.version,
            &member.id,
            member_work_context(&member.id, "we-accept-2", "claim-accept-ha", "unix-ms:3"),
        )
        .expect("claim Work");
    let submitted = store
        .submit_work(
            &claimed.id,
            claimed.version,
            &member.id,
            "done",
            vec!["artifact://z".into()],
            vec![],
            member_work_context(&member.id, "we-accept-3", "submit-accept-ha", "unix-ms:4"),
        )
        .expect("submit Work");
    let _accepted = store
        .accept_work_with_summary(
            &submitted.id,
            submitted.version,
            Some("Host accepted"),
            host_work_context("we-accept-4", "accept-accept-ha", "unix-ms:5"),
        )
        .expect("accept Work");
    let attentions = store.host_attentions().expect("host attentions");
    let accepted = attentions
        .iter()
        .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkAccepted);
    assert!(
        accepted.is_some(),
        "bound run must emit WorkAccepted on accept"
    );
    assert_eq!(accepted.unwrap().team_run_id, run.id);
    // WorkReviewRequested should still be present from the earlier submit
    let review = attentions
        .iter()
        .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
    assert!(
        review.is_some(),
        "WorkReviewRequested must persist after accept"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
