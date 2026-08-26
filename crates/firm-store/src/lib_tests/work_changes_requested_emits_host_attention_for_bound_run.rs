use super::*;

#[test]
fn host_authored_changes_requested_does_not_wake_the_same_host() {
    let (root, store, run, member, _) = work_test_fixture("work-cr-ha");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-cr-ha-1"),
            host_work_context("we-cr-1", "create-cr-ha", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &work.id,
            work.version,
            &member.id,
            member_work_context(&member.id, "we-cr-2", "claim-cr-ha", "unix-ms:3"),
        )
        .expect("claim Work");
    let claimed = start_claimed_work_for_test(
        &store,
        &claimed,
        &member,
        "we-cr-start",
        "start-cr-ha",
        "unix-ms:3.5",
    );
    let submitted = store
        .submit_work(
            &claimed.id,
            claimed.version,
            &member.id,
            "done",
            vec!["artifact://x".into()],
            vec![],
            member_work_context(&member.id, "we-cr-3", "submit-cr-ha", "unix-ms:4"),
        )
        .expect("submit Work");
    let _changes = store
        .request_work_changes(
            &submitted.id,
            submitted.version,
            "needs more tests",
            host_work_context("we-cr-4", "request-changes-cr-ha", "unix-ms:5"),
        )
        .expect("request changes");
    let attentions = store.host_attentions().expect("host attentions");
    let cr = attentions
        .iter()
        .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkChangesRequested);
    assert!(
        cr.is_none(),
        "Host-authored changes requested must not recursively wake the same Host"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
