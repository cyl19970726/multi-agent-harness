use super::*;

    #[test]
    fn expected_artifact_is_copied_from_worker_cwd_to_live_repo() {
        let root = std::env::temp_dir().join(format!("harness-artifact-{}", generated_id("copy")));
        let worker = root.join("worktree");
        let repo = root.join("repo");
        std::fs::create_dir_all(worker.join("out")).expect("mk worker out");
        std::fs::create_dir_all(&repo).expect("mk repo");
        std::fs::write(worker.join("out/image.png"), b"image-bytes").expect("write artifact");

        let outcome = collect_expected_artifacts(&worker, &repo, &["out/image.png".to_string()]);

        assert_eq!(outcome.failures, Vec::<String>::new());
        assert_eq!(outcome.copied, vec!["out/image.png".to_string()]);
        assert_eq!(
            std::fs::read(repo.join("out/image.png")).expect("read copied artifact"),
            b"image-bytes"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

