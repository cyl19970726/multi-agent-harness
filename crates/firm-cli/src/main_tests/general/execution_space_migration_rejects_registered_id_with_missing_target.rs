use super::*;

#[test]
fn execution_space_migration_rejects_registered_id_with_missing_target() {
    let (root, firm_home, project_context) = migration_test_project("registered-id");
    fs::write(
        project_context.store_root.join("missions.jsonl"),
        b"{\"id\":\"source\"}\n",
    )
    .expect("source ledger");
    let registered = execution_space::register_and_activate(
        &firm_home,
        "registered-space",
        "Registered Space",
        None,
        None,
        "unix-ms:2",
    )
    .expect("register stale id");
    fs::remove_dir_all(&registered.store_root).expect("remove stale registered target");
    let registry_before = fs::read(execution_space::registry_path(&firm_home)).unwrap();
    let active_before = fs::read(execution_space::active_space_path(&firm_home)).unwrap();

    let error = execution_space_migrate_from_project(
        &firm_home,
        &migration_args(&project_context.id, "registered-space", false),
    )
    .expect_err("a registered id cannot be reused as a migration target");
    assert!(error.to_string().contains("already registered"));
    assert!(error.to_string().contains("choose a new --id"));
    assert!(!registered.store_root.exists());
    assert_eq!(
        fs::read(execution_space::registry_path(&firm_home)).unwrap(),
        registry_before
    );
    assert_eq!(
        fs::read(execution_space::active_space_path(&firm_home)).unwrap(),
        active_before
    );
    assert!(hidden_migration_paths(&firm_home, "registered-space").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}
