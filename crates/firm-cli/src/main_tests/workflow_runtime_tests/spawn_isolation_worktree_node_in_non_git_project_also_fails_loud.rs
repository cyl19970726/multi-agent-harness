use super::*;

    #[test]
    fn spawn_isolation_worktree_node_in_non_git_project_also_fails_loud() {
        // The same gate must fire for an explicit isolation="worktree" node even when
        // it is not `writable` — both need a git worktree that a non-git project lacks.
        let store = temp_store("nongit-iso");
        let project_root =
            std::env::temp_dir().join(format!("harness-globaliso-{}", generated_id("gi")));
        std::fs::create_dir_all(&project_root).expect("mk global iso root");
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
        let spec = cwd_test_spec("iso", false, Some("worktree"));
        let err = spawn_ephemeral_worker(&store, &options, &spec, "wfrun-gi", "session-gi-0")
            .expect_err("isolation=worktree in a non-git project must fail loud");
        assert!(
            err.to_string().contains("not a git repository"),
            "names the cause: {err}"
        );
        let _ = std::fs::remove_dir_all(&project_root);
    }

