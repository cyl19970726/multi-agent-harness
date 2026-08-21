use super::*;

    #[test]
    fn execution_space_migration_rejects_retired_force_without_modification() {
        let (root, firm_home, project_context) = migration_test_project("retired-force");
        let source = project_context.store_root.join("missions.jsonl");
        fs::write(&source, b"{\"id\":\"source\"}\n").expect("source ledger");
        let source_before = fs::read(&source).unwrap();
        let registry_before = fs::read(execution_space::registry_path(&firm_home)).ok();
        let active_before = fs::read(execution_space::active_space_path(&firm_home)).ok();

        let error = execution_space_migrate_from_project(
            &firm_home,
            &migration_args(&project_context.id, "force-space", true),
        )
        .expect_err("force is retired");
        assert!(error.to_string().contains("--force is retired"));
        assert!(!execution_space::space_store_root(&firm_home, "force-space").exists());
        assert_eq!(fs::read(&source).unwrap(), source_before);
        assert_eq!(
            fs::read(execution_space::registry_path(&firm_home)).ok(),
            registry_before
        );
        assert_eq!(
            fs::read(execution_space::active_space_path(&firm_home)).ok(),
            active_before
        );
        assert!(hidden_migration_paths(&firm_home, "force-space").is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

