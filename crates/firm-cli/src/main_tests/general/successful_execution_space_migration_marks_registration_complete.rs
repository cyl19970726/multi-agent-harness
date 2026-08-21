use super::*;

#[test]
fn successful_execution_space_migration_marks_registration_complete() {
    let (root, firm_home, project_context) = migration_test_project("registration-complete");
    fs::write(
        project_context.store_root.join("missions.jsonl"),
        b"{\"id\":\"source\"}\n",
    )
    .expect("source ledger");
    execution_space_migrate_from_project(
        &firm_home,
        &migration_args(&project_context.id, "complete-space", false),
    )
    .expect("migration");
    let target = execution_space::space_store_root(&firm_home, "complete-space");
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(target.join("execution_space_migration.json")).expect("manifest"),
    )
    .expect("valid manifest");
    assert_eq!(manifest["registration"]["status"], "complete");
    assert_eq!(
        execution_space::active_space_id(&firm_home).unwrap(),
        Some("complete-space".into())
    );
    assert!(hidden_migration_paths(&firm_home, "complete-space").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}
