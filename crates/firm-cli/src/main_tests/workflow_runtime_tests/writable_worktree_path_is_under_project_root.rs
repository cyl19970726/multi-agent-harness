use super::*;

    #[test]
    fn writable_worktree_path_is_under_project_root() {
        // worktree-root-split: a writable leaf's git worktree lives under
        // <project_root>/.harness/worktrees/... — pinned to the repo, NOT the
        // centralized store and NOT the harness process cwd. We init a real git repo
        // as the project root, create the worktree directly, and assert its path.
        let project_root =
            std::env::temp_dir().join(format!("harness-gitproj-{}", generated_id("gp")));
        std::fs::create_dir_all(&project_root).expect("mk git project root");
        // Minimal git repo with one commit so `worktree add HEAD` works.
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&project_root)
                .args(args)
                .output()
                .expect("git")
        };
        assert!(git(&["init"]).status.success(), "git init");
        let _ = git(&["config", "user.email", "t@t"]);
        let _ = git(&["config", "user.name", "t"]);
        std::fs::write(project_root.join("README"), "x").expect("seed file");
        let _ = git(&["add", "-A"]);
        assert!(
            git(&["commit", "-m", "init"]).status.success(),
            "git commit"
        );

        let guard = WorktreeGuard::create(&project_root, "wfrun-gp", "writer", "session-gp-0")
            .expect("worktree create in a git project");
        assert!(
            guard.path.starts_with(&project_root),
            "worktree must live under the project root: {:?}",
            guard.path
        );
        assert!(
            guard.path.to_string_lossy().contains(".harness/worktrees/"),
            "worktree path must be the gitignored .harness/worktrees/ dir: {:?}",
            guard.path
        );
        assert!(guard.path.is_dir(), "worktree dir was actually created");
        drop(guard);
        let _ = std::fs::remove_dir_all(&project_root);
    }

