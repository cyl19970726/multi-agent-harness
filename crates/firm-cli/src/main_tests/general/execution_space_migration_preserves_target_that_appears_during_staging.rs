use super::*;

#[test]
fn execution_space_migration_preserves_target_that_appears_during_staging() {
    let (root, firm_home, project_context) = migration_test_project("target-race");
    fs::write(
        project_context.store_root.join("missions.jsonl"),
        b"{\"id\":\"source\"}\n",
    )
    .expect("source ledger");
    let target = execution_space::space_store_root(&firm_home, "raced-space");
    let hook_target = target.clone();
    let registry_before = fs::read(execution_space::registry_path(&firm_home)).ok();
    let active_before = fs::read(execution_space::active_space_path(&firm_home)).ok();

    let error = execution_space_migrate_from_project_with_hooks(
        &firm_home,
        &migration_args(&project_context.id, "raced-space", false),
        move || {
            fs::create_dir_all(&hook_target)?;
            fs::write(hook_target.join("foreign.txt"), b"foreign target")?;
            Ok(())
        },
        |_home, _lock, _id, _name, _binding, _now| panic!("activation must not run"),
    )
    .expect_err("a target that appears before publish must be preserved");
    assert!(error.to_string().contains("appeared while staging"));
    assert_eq!(
        fs::read(target.join("foreign.txt")).unwrap(),
        b"foreign target"
    );
    assert!(!target.join("missions.jsonl").exists());
    assert_eq!(
        fs::read(execution_space::registry_path(&firm_home)).ok(),
        registry_before
    );
    assert_eq!(
        fs::read(execution_space::active_space_path(&firm_home)).ok(),
        active_before
    );
    assert!(hidden_migration_paths(&firm_home, "raced-space").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}
