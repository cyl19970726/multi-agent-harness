use super::*;

    #[test]
    fn gc_worktrees_keeps_registered_worktree_for_running_run() {
        let store = temp_store("gc-wt-running");
        let project_root = init_gc_git_project("running", &store);
        seed_gc_workflow_run(&store, "wfrun-running", WorkflowRunStatus::Running);
        let running = add_registered_gc_worktree(
            &project_root,
            "wfrun-running",
            "writer",
            "session-running-0",
        );

        let out = workflow_gc_worktrees(&store, None).expect("gc worktrees");
        assert!(
            out["removed"].as_array().expect("removed array").is_empty(),
            "running owner should not be removed: {out}"
        );
        assert!(running.is_dir(), "running run worktree preserved");
        let _ = Command::new("git")
            .arg("-C")
            .arg(&project_root)
            .args(["worktree", "remove", "--force"])
            .arg(&running)
            .output();
        let _ = std::fs::remove_dir_all(&project_root);
    }

