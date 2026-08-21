use super::*;

#[test]
fn gc_worktrees_removes_registered_worktrees_for_terminal_or_absent_runs() {
    let store = temp_store("gc-wt-stale");
    let project_root = init_gc_git_project("stale", &store);
    seed_gc_workflow_run(&store, "wfrun-terminal", WorkflowRunStatus::Completed);
    let terminal = add_registered_gc_worktree(
        &project_root,
        "wfrun-terminal",
        "writer",
        "session-terminal-0",
    );
    let absent =
        add_registered_gc_worktree(&project_root, "wfrun-absent", "writer", "session-abs-0");

    let out = workflow_gc_worktrees(&store, None).expect("gc worktrees");
    let removed = out["removed"].as_array().expect("removed array");
    let terminal_display = terminal.display().to_string();
    let absent_display = absent.display().to_string();
    assert!(
        removed
            .iter()
            .any(|value| value.as_str() == Some(terminal_display.as_str())),
        "terminal owner's worktree should be reported removed: {out}"
    );
    assert!(
        removed
            .iter()
            .any(|value| value.as_str() == Some(absent_display.as_str())),
        "absent owner's worktree should be reported removed: {out}"
    );
    assert!(!terminal.exists(), "terminal run worktree removed");
    assert!(!absent.exists(), "absent run worktree removed");
    let _ = std::fs::remove_dir_all(&project_root);
}
