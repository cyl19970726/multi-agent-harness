use super::*;

    #[test]
    fn concurrent_migrations_cannot_both_publish_or_register_the_same_id() {
        let (root, firm_home, first_project) = migration_test_project("same-id-race");
        let second_root = root.join("second-project");
        fs::create_dir_all(&second_root).expect("second project root");
        let second_project = project::register_and_activate(&firm_home, &second_root, "unix-ms:2")
            .expect("second project registration");
        HarnessStore::new(second_project.store_root.clone())
            .init()
            .expect("second source store");
        let barrier = Arc::new(std::sync::Barrier::new(2));

        let spawn_migration = |project_context: ProjectContext| {
            let home = firm_home.clone();
            let rendezvous = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let project_id = project_context.id.clone();
                let result = execution_space_migrate_from_project_with_hooks(
                    &home,
                    &migration_args(&project_id, "contended-space", false),
                    move || {
                        rendezvous.wait();
                        Ok(())
                    },
                    |home, lock, id, name, binding, now| {
                        execution_space::register_and_activate_locked(
                            home,
                            lock,
                            id,
                            name,
                            Some(binding.to_string()),
                            None,
                            now,
                        )
                    },
                );
                (project_id, result)
            })
        };
        let first = spawn_migration(first_project);
        let second = spawn_migration(second_project);
        let outcomes = [
            first.join().expect("first migration"),
            second.join().expect("second migration"),
        ];
        assert_eq!(
            outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
            1,
            "exactly one same-id migration may succeed: {outcomes:?}"
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|(_, result)| result.is_err())
                .count(),
            1
        );

        let winner = outcomes
            .iter()
            .find(|(_, result)| result.is_ok())
            .map(|(project_id, _)| project_id.as_str())
            .expect("winning source");
        let registry = execution_space::ExecutionSpaceRegistry::load(&firm_home)
            .expect("parseable registry after contention");
        let entries = registry
            .spaces
            .iter()
            .filter(|entry| entry.id == "contended-space")
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].default_project_binding_id.as_deref(),
            Some(winner)
        );
        let metadata = execution_space::read_metadata(&execution_space::space_store_root(
            &firm_home,
            "contended-space",
        ))
        .expect("read target metadata")
        .expect("target metadata");
        assert_eq!(metadata.default_project_binding_id.as_deref(), Some(winner));
        assert!(hidden_migration_paths(&firm_home, "contended-space").is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

