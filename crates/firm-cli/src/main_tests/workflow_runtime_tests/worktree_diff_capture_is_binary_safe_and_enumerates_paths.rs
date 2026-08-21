use super::*;

    #[test]
    fn worktree_diff_capture_is_binary_safe_and_enumerates_paths() {
        // A real worktree: create a text file + a binary file, capture with the
        // isolation-path helpers, and assert (a) the binary is captured as a
        // GIT-binary-patch block (D5, not a "Binary files differ" stub) that
        // `git apply` accepts, and (b) name-status enumerates both paths (D4a).
        let seed_repo = |dir: &Path| {
            std::fs::create_dir_all(dir).unwrap();
            let git = |args: &[&str]| {
                Command::new("git")
                    .arg("-C")
                    .arg(dir)
                    .args(args)
                    .output()
                    .expect("git")
            };
            assert!(git(&["init"]).status.success());
            let _ = git(&["config", "user.email", "t@t"]);
            let _ = git(&["config", "user.name", "t"]);
            std::fs::write(dir.join("README"), "seed").unwrap();
            let _ = git(&["add", "-A"]);
            assert!(git(&["commit", "-m", "seed"]).status.success());
        };
        let repo = std::env::temp_dir().join(format!("harness-bincap-{}", generated_id("bin")));
        seed_repo(&repo);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/text.txt"), "hello\n").unwrap();
        std::fs::write(repo.join("src/blob.bin"), [0u8, 1, 2, 3, 255, 128, 7]).unwrap();

        let diff = ephemeral_worktree_diff(&repo).expect("diff");
        assert!(
            diff.contains("GIT binary patch"),
            "binary change captured as a git binary patch (D5), not a stub: {diff}"
        );
        assert!(
            !diff.contains("Binary files"),
            "no lossy 'Binary files differ' stub in a --binary capture"
        );
        let paths = ephemeral_worktree_changed_paths(&repo).expect("paths");
        assert!(paths.contains(&"src/text.txt".to_string()));
        assert!(paths.contains(&"src/blob.bin".to_string()));

        // The captured diff applies cleanly onto a fresh clean checkout of the seed.
        let fresh = std::env::temp_dir().join(format!("harness-bincap2-{}", generated_id("bin")));
        seed_repo(&fresh);
        std::fs::create_dir_all(fresh.join("src")).unwrap();
        apply_patch_bytes(&fresh, diff.as_bytes(), true).expect("binary diff applies --check");
        apply_patch_bytes(&fresh, diff.as_bytes(), false).expect("binary diff applies");
        assert_eq!(
            std::fs::read(fresh.join("src/blob.bin")).unwrap(),
            vec![0u8, 1, 2, 3, 255, 128, 7],
            "binary content round-trips through the captured patch"
        );

        std::fs::remove_dir_all(&repo).ok();
        std::fs::remove_dir_all(&fresh).ok();
    }

