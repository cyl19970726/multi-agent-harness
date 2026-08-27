use super::*;

#[test]
#[ignore = "legacy Work acceptance route is retired; canonical exact-candidate acceptance is covered by member_execution_trust"]
fn duplicate_title_guard_allows_when_existing_is_done() {
    let (root, store, run, member_a, _member_b) = work_test_fixture("dup-title-done");
    let ctx1 = host_work_context("dup-ctx-done-1", "create-first", "unix-ms:3");
    let mut work = work_with_title(&run.id, "work-audit-1", "Audit Company Docs");
    work.claim_mode = WorkClaimMode::HostAssign;
    let first = store.insert_work(work, ctx1).expect("create first Work");

    // Start → Submit → Accept to make the work Done.
    let first = store
        .start_work(
            &first.id,
            first.version,
            &member_a.id,
            member_work_context(&member_a.id, "start", "start-key", "unix-ms:4"),
        )
        .expect("start");
    let first = store
        .submit_work(
            &first.id,
            first.version,
            &member_a.id,
            "All tests pass.",
            Vec::new(),
            Vec::new(),
            member_work_context(&member_a.id, "submit", "submit-key", "unix-ms:5"),
        )
        .expect("submit");
    store
        .accept_work(
            &first.id,
            first.version,
            host_work_context("accept", "accept-key", "unix-ms:6"),
        )
        .expect("accept first Work");

    let ctx2 = host_work_context("dup-ctx-done-2", "create-after-done", "unix-ms:7");
    let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
    store
        .insert_work(dup, ctx2)
        .expect("terminal existing Work must not block new same-title");
    std::fs::remove_dir_all(root).expect("remove temp store");
}
