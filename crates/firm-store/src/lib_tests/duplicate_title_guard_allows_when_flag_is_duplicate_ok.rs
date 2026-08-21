use super::*;

#[test]
fn duplicate_title_guard_allows_when_flag_is_duplicate_ok() {
    let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-flag");
    let ctx1 = host_work_context("dup-ctx-flag-1", "create-first", "unix-ms:3");
    store
        .insert_work(
            work_with_title(&run.id, "work-audit-1", "Audit Company Docs"),
            ctx1,
        )
        .expect("create first Work");

    let mut ctx2 = host_work_context("dup-ctx-flag-2", "create-dup-ok", "unix-ms:4");
    ctx2.duplicate_ok = true;
    let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
    let created = store
        .insert_work(dup, ctx2)
        .expect("duplicate-ok must allow same-title Work");
    assert_eq!(created.title, "Audit Company Docs");
    std::fs::remove_dir_all(root).expect("remove temp store");
}
