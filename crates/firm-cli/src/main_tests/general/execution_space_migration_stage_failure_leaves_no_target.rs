use super::*;

    #[test]
    fn execution_space_migration_stage_failure_leaves_no_target() {
        let (root, firm_home, project_context) = migration_test_project("stage-failure");
        fs::write(
            project_context.store_root.join("missions.jsonl"),
            b"{\"id\":\"source\"}\n",
        )
        .expect("source ledger");

        let error = execution_space_migrate_from_project_with_hooks(
            &firm_home,
            &migration_args(&project_context.id, "failed-stage-space", false),
            || {
                Err(CliError::Usage(
                    "injected stage verification failure".into(),
                ))
            },
            |_home, _lock, _id, _name, _binding, _now| panic!("activation must not run"),
        )
        .expect_err("stage failure");
        assert!(error
            .to_string()
            .contains("injected stage verification failure"));
        assert!(!execution_space::space_store_root(&firm_home, "failed-stage-space").exists());
        assert!(hidden_migration_paths(&firm_home, "failed-stage-space").is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

