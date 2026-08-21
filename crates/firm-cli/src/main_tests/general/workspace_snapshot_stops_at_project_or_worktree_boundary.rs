use super::*;

#[test]
fn workspace_snapshot_stops_at_project_or_worktree_boundary() {
    let root = std::env::temp_dir().join(format!(
        "harness-workspace-boundary-{}-{}",
        std::process::id(),
        generated_id("test")
    ));
    let project_root = root.join("project");
    let cwd = project_root.join("src").join("nested");
    std::fs::create_dir_all(cwd.join(".agents/skills/local")).expect("local skills");
    std::fs::write(root.join("AGENTS.md"), "outside").expect("outside instructions");
    std::fs::create_dir_all(root.join("skills/outside")).expect("outside skills");
    std::fs::write(project_root.join("AGENTS.md"), "inside").expect("inside instructions");

    let snapshot = snapshot_member_workspace(
        &cwd,
        Some("binding-1"),
        Some(&project_root),
        "project_binding_root",
    );
    let outside = project::canonicalize_best_effort(&root)
        .display()
        .to_string();
    let inside = project::canonicalize_best_effort(&project_root)
        .display()
        .to_string();
    assert!(snapshot.instruction_roots.contains(&inside));
    assert!(!snapshot.instruction_roots.contains(&outside));
    assert!(snapshot
        .skill_roots
        .iter()
        .all(|path| !path.starts_with(&format!("{outside}/skills"))));
    assert_eq!(snapshot.project_binding_id.as_deref(), Some("binding-1"));
    let _ = std::fs::remove_dir_all(root);
}
