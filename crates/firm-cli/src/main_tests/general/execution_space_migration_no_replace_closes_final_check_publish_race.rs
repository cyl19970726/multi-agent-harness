use super::*;

#[test]
fn execution_space_migration_no_replace_closes_final_check_publish_race() {
    let (root, firm_home, project_context) = migration_test_project("publish-window-race");
    let target = execution_space::space_store_root(&firm_home, "publish-raced-space");
    let hook_target = target.clone();

    let error = execution_space_migrate_from_project_with_publish_hook(
        &firm_home,
        &migration_args(&project_context.id, "publish-raced-space", false),
        || Ok(()),
        move || {
            // This runs after the final target absence check and immediately
            // before the no-replace publication primitive.
            fs::create_dir(&hook_target)?;
            Ok(())
        },
        |_home, _lock, _id, _name, _binding, _now| panic!("activation must not run"),
    )
    .expect_err("an empty target appearing in the publish window must be preserved");
    assert!(error.to_string().contains("appeared while publishing"));
    assert!(target.is_dir(), "foreign empty directory must remain");
    assert!(!target.join("metadata.json").exists());
    assert!(hidden_migration_paths(&firm_home, "publish-raced-space").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}
