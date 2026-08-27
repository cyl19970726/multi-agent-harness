use super::*;

#[test]
fn duplicate_title_guard_allows_when_existing_is_done() {
    let (root, store, run, member, _) = work_test_fixture("dup-title-done");
    let created = store
        .insert_work(
            work_with_title(&run.id, "work-audit-done-1", "Audit Company Docs"),
            host_work_context("dup-done-create", "dup-done-create", "unix-ms:2"),
        )
        .expect("create first Work");
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "dup-done-assign",
        "dup-done-assign",
        "unix-ms:3",
    );
    let active = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "dup-done-start",
        "dup-done-start",
        "unix-ms:4",
    );
    let submitted = submit_started_work_for_test(
        &store,
        &active,
        &member,
        "dup-done-result",
        "All tests pass.",
        (Vec::new(), Vec::new()),
        "unix-ms:5",
    );
    let accepted = accept_result_for_test(
        &store,
        &submitted,
        "dup-done-result",
        "dup-done-accept",
        "unix-ms:6",
    );
    assert_eq!(accepted.phase, WorkPhase::Closed);

    store
        .insert_work(
            work_with_title(&run.id, "work-audit-done-2", "Audit Company Docs"),
            host_work_context("dup-after-done", "dup-after-done", "unix-ms:7"),
        )
        .expect("terminal existing Work must not block a new same-title Work");
    std::fs::remove_dir_all(root).expect("remove temp store");
}
