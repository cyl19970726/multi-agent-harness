use super::*;

    #[test]
    fn execution_space_migration_rejects_existing_target_without_modification() {
        let (root, firm_home, project_context) = migration_test_project("existing-target");
        fs::write(
            project_context.store_root.join("missions.jsonl"),
            b"{\"id\":\"source\"}\n",
        )
        .expect("source ledger");
        let target = execution_space::space_store_root(&firm_home, "existing-space");
        fs::create_dir_all(&target).expect("target");
        fs::write(target.join("keep.txt"), b"untouched").expect("sentinel");
        let registry_before = fs::read(execution_space::registry_path(&firm_home)).ok();
        let active_before = fs::read(execution_space::active_space_path(&firm_home)).ok();

        let error = execution_space_migrate_from_project(
            &firm_home,
            &migration_args(&project_context.id, "existing-space", false),
        )
        .expect_err("existing target must fail");
        assert!(error.to_string().contains("choose a new --id"));
        assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"untouched");
        assert!(!target.join("missions.jsonl").exists());
        assert_eq!(
            fs::read(execution_space::registry_path(&firm_home)).ok(),
            registry_before
        );
        assert_eq!(
            fs::read(execution_space::active_space_path(&firm_home)).ok(),
            active_before
        );
        assert!(hidden_migration_paths(&firm_home, "existing-space").is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

