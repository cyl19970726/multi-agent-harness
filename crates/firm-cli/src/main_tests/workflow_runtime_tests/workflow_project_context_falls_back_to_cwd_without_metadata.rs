use super::*;

#[test]
fn workflow_project_context_falls_back_to_cwd_without_metadata() {
    // BACK-COMPAT: a store with no metadata.json (a raw --store / FIRM_ROOT /
    // walk-up store) has no pinned identity, so the project_root degrades to the
    // harness process cwd exactly as before, and store_root is the store root.
    let store = temp_store("nometa");
    let ctx = workflow_project_context(&store);
    assert_eq!(
        ctx.project_root,
        env::current_dir().unwrap(),
        "no metadata → cwd is the project root (today's behavior)"
    );
    assert_eq!(ctx.store_root, store.root());
}
