use super::*;

    #[test]
    fn worktree_create_in_non_git_dir_gives_actionable_error() {
        // A writable / isolated step in a non-git cwd must fail with guidance, not
        // the cryptic raw `git worktree add` error (issue #89 item 5).
        let dir = std::env::temp_dir().join(format!("harness-nongit-{}", generated_id("ng")));
        std::fs::create_dir_all(&dir).expect("mk non-git dir");
        // (WorktreeGuard isn't Debug — match instead of expect_err.)
        let msg = match WorktreeGuard::create(&dir, "wfrun-x", "writer", "session-x-0") {
            Ok(_) => panic!("a non-git dir must fail clearly, not attempt git worktree add"),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("not a git repository"),
            "names the cause: {msg}"
        );
        assert!(
            msg.contains("git init") && msg.contains("get-output"),
            "offers both fixes (git init / read-only + get-output): {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

