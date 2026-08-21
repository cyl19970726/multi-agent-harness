use super::*;

#[test]
fn direct_write_mode_requires_writable_clean_git_project() {
    let store = temp_store("direct-guards");
    let project_root = init_gc_git_project("direct-guards", &store);
    let project = workflow_project_context(&store);
    let mut spec = cwd_test_spec("direct", true, None);
    spec.write_mode = Some(workflow::WRITE_MODE_DIRECT.into());
    ensure_direct_write_ready(&project, &project_root, &spec).expect("clean git repo allowed");

    let mut read_only = spec.clone();
    read_only.writable = false;
    let err = ensure_direct_write_ready(&project, &project_root, &read_only)
        .expect_err("direct mode requires writable");
    assert!(err.to_string().contains("require writable=True"));

    let non_git_root =
        std::env::temp_dir().join(format!("harness-direct-nongit-{}", generated_id("ng")));
    std::fs::create_dir_all(&non_git_root).expect("mk non git");
    let non_git_project = ProjectContext {
        id: "nongit".into(),
        project_root: non_git_root.clone(),
        store_root: store.root().to_path_buf(),
        kind: ProjectKind::Repo,
        is_git_repo: false,
    };
    let err = ensure_direct_write_ready(&non_git_project, &non_git_root, &spec)
        .expect_err("non-git direct write rejected");
    assert!(err.to_string().contains("not a git repository"));

    std::fs::write(project_root.join("scratch.txt"), "dirty").expect("dirty file");
    let err =
        ensure_direct_write_ready(&project, &project_root, &spec).expect_err("dirty repo rejected");
    assert!(err.to_string().contains("uncommitted changes"));

    let _ = std::fs::remove_dir_all(&project_root);
    let _ = std::fs::remove_dir_all(&non_git_root);
    let _ = std::fs::remove_dir_all(store.root());
}
