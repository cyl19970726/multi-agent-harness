use super::*;

#[test]
fn workflow_project_context_reads_pinned_metadata() {
    // A central store self-describes its project via metadata.json; the workflow
    // recovers the real project_root from it (NOT the process cwd).
    let store = temp_store("withmeta");
    let project_root = std::env::temp_dir().join(format!("harness-pinned-{}", generated_id("pin")));
    std::fs::create_dir_all(&project_root).expect("mk pinned root");
    let pinned = ProjectContext {
        id: "pinned-proj".into(),
        project_root: project_root.clone(),
        store_root: store.root().to_path_buf(),
        kind: ProjectKind::Repo,
        is_git_repo: false,
    };
    project::write_metadata(&pinned, None).expect("write metadata");

    let ctx = workflow_project_context(&store);
    assert_eq!(ctx.id, "pinned-proj");
    assert_eq!(ctx.project_root, project_root);
    assert_eq!(ctx.store_root, store.root());
    assert!(!ctx.is_git_repo);
    let _ = std::fs::remove_dir_all(&project_root);
}
