use super::*;

#[test]
fn partial_registration_failure_recovers_registry_and_active_pointer_via_switch() {
    let (root, firm_home, project_context) = migration_test_project("partial-registration");
    fs::write(
        project_context.store_root.join("missions.jsonl"),
        b"{\"id\":\"source\"}\n",
    )
    .expect("source ledger");
    let active_path = execution_space::active_space_path(&firm_home);
    fs::create_dir_all(&active_path).expect("block ACTIVE_SPACE file publication");
    let target = execution_space::space_store_root(&firm_home, "partial-space");

    let error = execution_space_migrate_from_project(
        &firm_home,
        &migration_args(&project_context.id, "partial-space", false),
    )
    .expect_err("ACTIVE_SPACE failure occurs after registry publication");
    assert!(error.to_string().contains("published and verified"));
    assert!(target.join("execution_space_migration.json").exists());
    let partial_registry = execution_space::ExecutionSpaceRegistry::load(&firm_home)
        .expect("partially written registry remains readable");
    assert!(partial_registry.find("partial-space").is_some());
    assert_eq!(
        partial_registry.current_space_id.as_deref(),
        Some("partial-space")
    );
    let pending_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(target.join("execution_space_migration.json")).expect("pending manifest"),
    )
    .expect("valid pending manifest");
    assert_eq!(pending_manifest["registration"]["status"], "pending");

    fs::remove_dir(&active_path).expect("clear injected ACTIVE_SPACE blocker");
    let recovered = execution_space::switch_current_space(&firm_home, "partial-space", "unix-ms:3")
        .expect("public switch converges partial registration");
    assert_eq!(recovered.store_root, target);
    assert_eq!(fs::read_to_string(&active_path).unwrap(), "partial-space\n");
    let recovered_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(target.join("execution_space_migration.json")).expect("recovered manifest"),
    )
    .expect("valid recovered manifest");
    assert_eq!(recovered_manifest["registration"]["status"], "complete");
    assert!(hidden_migration_paths(&firm_home, "partial-space").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}
