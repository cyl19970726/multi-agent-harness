use super::*;

#[test]
fn host_authored_work_cancel_does_not_wake_the_same_host() {
    let (root, store, run, member, _) = work_test_fixture("work-cancel-ha");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-cancel-ha-1"),
            host_work_context("we-cancel-1", "create-cancel-ha", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &work.id,
            work.version,
            &member.id,
            member_work_context(&member.id, "we-cancel-2", "claim-cancel-ha", "unix-ms:3"),
        )
        .expect("claim Work");
    let _cancelled = store
        .cancel_work(
            &claimed.id,
            claimed.version,
            "no longer needed",
            host_work_context("we-cancel-3", "cancel-cancel-ha", "unix-ms:4"),
        )
        .expect("cancel Work");
    let attentions = store.host_attentions().expect("host attentions");
    let cancelled = attentions
        .iter()
        .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkCancelled);
    assert!(
        cancelled.is_none(),
        "a Host-authored cancellation must not recursively wake the same Host"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
