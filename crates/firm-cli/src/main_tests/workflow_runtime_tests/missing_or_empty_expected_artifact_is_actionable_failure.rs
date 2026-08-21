use super::*;

    #[test]
    fn missing_or_empty_expected_artifact_is_actionable_failure() {
        let root =
            std::env::temp_dir().join(format!("harness-artifact-{}", generated_id("missing")));
        let worker = root.join("worktree");
        let repo = root.join("repo");
        std::fs::create_dir_all(worker.join("out")).expect("mk worker out");
        std::fs::create_dir_all(&repo).expect("mk repo");
        std::fs::write(worker.join("out/empty.txt"), b"").expect("write empty artifact");

        let outcome = collect_expected_artifacts(
            &worker,
            &repo,
            &["out/missing.txt".to_string(), "out/empty.txt".to_string()],
        );

        assert!(outcome.copied.is_empty());
        assert_eq!(outcome.failures.len(), 2);
        assert!(
            outcome.failures[0].contains("missing or empty")
                && outcome.failures[0].contains("write a non-empty file"),
            "failure should be actionable: {:?}",
            outcome.failures
        );
        assert!(
            outcome.failures[1].contains("missing or empty"),
            "empty artifact should fail: {:?}",
            outcome.failures
        );
        assert!(
            !step_ok_after_gates(true, false, &outcome),
            "a missing declared artifact must fail the step even when the provider succeeded"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

