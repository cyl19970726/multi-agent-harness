use super::*;

#[test]
fn work_submit_emits_host_attention_for_bound_run() {
    let (root, store, run, member, _) = work_test_fixture("work-submit-ha");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-submit-ha-1"),
            host_work_context("we-submit-1", "create-submit-ha", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &work.id,
            work.version,
            &member.id,
            member_work_context(&member.id, "we-submit-2", "claim-submit-ha", "unix-ms:3"),
        )
        .expect("claim Work");
    let claimed = start_claimed_work_for_test(
        &store,
        &claimed,
        &member,
        "we-submit-start",
        "start-submit-ha",
        "unix-ms:3.5",
    );
    let _submitted = submit_started_work_for_test(
        &store,
        &claimed,
        &member,
        "we-submit-3",
        "done",
        vec!["artifact://x".into()],
        vec!["check://y".into()],
        "unix-ms:4",
    );
    let attentions = store.host_attentions().expect("host attentions");
    let review = attentions
        .iter()
        .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
    assert!(
        review.is_some(),
        "bound run must emit WorkReviewRequested on submit"
    );
    assert_eq!(review.unwrap().team_run_id, run.id);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
