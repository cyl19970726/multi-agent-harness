use super::*;

#[test]
fn workflow_repo_root_is_project_root_not_process_cwd() {
    // worktree-root-split: the worker's shared cwd + worktree base is the
    // PROJECT ROOT (a long-running `serve` never `cd`s), NOT `env::current_dir`.
    let project_root =
        std::env::temp_dir().join(format!("harness-projroot-{}", generated_id("pr")));
    std::fs::create_dir_all(&project_root).expect("mk project root");
    let ctx = ProjectContext {
        id: "demo".into(),
        project_root: project_root.clone(),
        store_root: std::env::temp_dir().join("some-central-store"),
        kind: ProjectKind::Repo,
        is_git_repo: true,
    };
    let resolved = workflow_repo_root(&ctx);
    assert_eq!(resolved, project_root, "repo root must be project_root");
    assert_ne!(
        resolved,
        env::current_dir().unwrap(),
        "must NOT fall back to the harness process cwd"
    );
    let _ = std::fs::remove_dir_all(&project_root);
}
