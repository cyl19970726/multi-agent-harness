use super::*;

    #[test]
    fn registration_failure_retains_verified_target_for_switch_recovery() {
        let (root, firm_home, project_context) = migration_test_project("registration-failure");
        fs::write(
            project_context.store_root.join("missions.jsonl"),
            b"{\"id\":\"source\"}\n",
        )
        .expect("source ledger");
        let target = execution_space::space_store_root(&firm_home, "pending-space");

        let error = execution_space_migrate_from_project_with_activate(
            &firm_home,
            &migration_args(&project_context.id, "pending-space", false),
            |_home, _lock, _id, _name, _binding, _now| {
                Err(execution_space::ExecutionSpaceError::Io(
                    std::io::Error::other("injected registration failure"),
                ))
            },
        )
        .expect_err("registration failure");
        assert!(error.to_string().contains("published and verified"));
        assert!(error
            .to_string()
            .contains("harness space switch pending-space"));
        assert_eq!(
            fs::read(target.join("missions.jsonl")).unwrap(),
            b"{\"id\":\"source\"}\n"
        );
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(target.join("execution_space_migration.json")).expect("manifest"),
        )
        .expect("valid manifest");
        assert_eq!(manifest["registration"]["status"], "pending");
        assert_eq!(
            manifest["registration"]["recovery_command"],
            "harness space switch pending-space"
        );

        let recovered =
            execution_space::switch_current_space(&firm_home, "pending-space", "unix-ms:3")
                .expect("public switch path recovers registration and activation");
        assert_eq!(recovered.store_root, target);
        assert_eq!(
            execution_space::active_space_id(&firm_home).unwrap(),
            Some("pending-space".into())
        );
        let recovered_manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(target.join("execution_space_migration.json")).expect("recovered manifest"),
        )
        .expect("valid recovered manifest");
        assert_eq!(recovered_manifest["registration"]["status"], "complete");
        assert!(hidden_migration_paths(&firm_home, "pending-space").is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

