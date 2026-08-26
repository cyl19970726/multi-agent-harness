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
    let ctx = member_work_context(&member.id, "we-dedup-3", "submit-dedup-ha", "unix-ms:4");
    let _submitted = store
        .submit_work(
            &claimed.id,
            claimed.version,
            &member.id,
            "done",
            vec!["artifact://x".into()],
            vec![],
            ctx.clone(),
        )
        .expect("first submit");
    // Second submit with same idempotency key should be a no-op (dedup).
    let _again = store
        .submit_work(
            &claimed.id,
            claimed.version,
            &member.id,
            "done",
            vec!["artifact://x".into()],
            vec![],
            ctx,
        )
        .expect("idempotent second submit");
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
