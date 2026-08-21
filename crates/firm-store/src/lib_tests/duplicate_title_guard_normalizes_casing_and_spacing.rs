use super::*;

#[test]
fn duplicate_title_guard_normalizes_casing_and_spacing() {
    let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-normalize");
    let ctx1 = host_work_context("dup-norm-1", "create-first", "unix-ms:3");
    store
        .insert_work(
            work_with_title(&run.id, "work-norm-1", "audit company docs"),
            ctx1,
        )
        .expect("create first Work");

    let ctx2 = host_work_context("dup-norm-2", "create-dup-norm", "unix-ms:4");
    let dup = work_with_title(&run.id, "work-norm-2", "AUDIT   COMPANY   DOCS");
    let error = store
        .insert_work(dup, ctx2)
        .expect_err("different casing/spacing must still be detected");
    assert!(
        error.to_string().contains("DUPLICATE_TITLE"),
        "error: {error}"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
