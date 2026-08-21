use super::*;

    #[test]
    fn spawn_writable_node_in_non_git_project_fails_loud() {
        // global-workflow-policy: a writable / isolation="worktree" node in a non-git
        // project (the reserved `_global` ~/ project) is rejected BEFORE any provider
        // spawn with an actionable message naming the project and offering the fix.
        let store = temp_store("nongit-writable");
        let project_root =
            std::env::temp_dir().join(format!("harness-global-{}", generated_id("g")));
        std::fs::create_dir_all(&project_root).expect("mk global root");
        let options = WorkflowDeliveryOptions {
            dry_run: false,
            start_runtime: false,
            timeout_ms: 1_000,
            default_model: None,
            default_effort: None,
            max_budget_usd: None,
            progress: false,
            project: ProjectContext {
                id: harness_core::GLOBAL_PROJECT_ID.into(),
                project_root: project_root.clone(),
                store_root: store.root().to_path_buf(),
                kind: ProjectKind::Global,
                is_git_repo: false,
            },
        };
        let spec = cwd_test_spec("writer", true, None);
        let err = spawn_ephemeral_worker(&store, &options, &spec, "wfrun-ng", "session-ng-0")
            .expect_err("writable node in a non-git project must fail loud");
        let msg = err.to_string();
        assert!(
            msg.contains("not a git repository"),
            "names the cause: {msg}"
        );
        assert!(
            msg.contains(harness_core::GLOBAL_PROJECT_ID),
            "names the offending project id: {msg}"
        );
        assert!(
            msg.contains("get-output") && msg.contains("isolation"),
            "offers the read-only fix: {msg}"
        );
        let _ = std::fs::remove_dir_all(&project_root);
    }

