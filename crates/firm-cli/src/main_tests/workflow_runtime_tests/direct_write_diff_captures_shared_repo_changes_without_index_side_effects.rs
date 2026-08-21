use super::*;

#[test]
fn direct_write_diff_captures_shared_repo_changes_without_index_side_effects() {
    let store = temp_store("direct-diff");
    let project_root = init_gc_git_project("direct-diff", &store);
    std::fs::write(project_root.join("README"), "changed\n").expect("change tracked");
    std::fs::create_dir_all(project_root.join("src")).expect("mk src");
    std::fs::write(project_root.join("src/direct.txt"), "new direct\n").expect("new file");

    let diff = direct_write_diff(&project_root).expect("direct diff");
    assert!(diff.contains("diff --git a/README b/README"));
    assert!(diff.contains("diff --git a/src/direct.txt b/src/direct.txt"));
    assert!(diff.contains("+new direct"));
    let status = git_in(&project_root, &["status", "--porcelain"]).expect("status");
    assert!(
        status.contains(" M README") && status.contains("?? src/"),
        "direct diff must not stage intent-to-add entries: {status}"
    );

    let _ = std::fs::remove_dir_all(&project_root);
    let _ = std::fs::remove_dir_all(store.root());
}
