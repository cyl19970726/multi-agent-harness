use super::*;

#[test]
fn duplicate_title_guard_refuses_non_terminal_match() {
    let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-guard");
    let ctx1 = host_work_context("dup-ctx-1", "create-first", "unix-ms:3");
    store
        .insert_work(
            work_with_title(&run.id, "work-audit-1", "Audit Company Docs"),
            ctx1,
        )
        .expect("create first Work");

    let ctx2 = host_work_context("dup-ctx-2", "create-dup", "unix-ms:4");
    let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
    let error = store
        .insert_work(dup, ctx2)
        .expect_err("duplicate title must fail");
    assert!(
        error.to_string().contains("DUPLICATE_TITLE"),
        "error: {error}"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
