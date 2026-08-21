use super::*;

#[cfg(unix)]
#[test]
fn execution_space_migration_rejects_source_evidence_symlink_before_staging() {
    use std::os::unix::fs::symlink;
    let (root, firm_home, project_context) = migration_test_project("source-symlink");
    let outside = root.join("outside-checks");
    fs::create_dir_all(&outside).expect("outside checks");
    fs::write(outside.join("result.json"), b"external").expect("outside evidence");
    symlink(&outside, project_context.store_root.join("checks")).expect("source symlink");

    let error = execution_space_migrate_from_project(
        &firm_home,
        &migration_args(&project_context.id, "source-linked-space", false),
    )
    .expect_err("source symlink fails closed");
    assert!(error.to_string().contains("refuses symbolic links"));
    assert!(!execution_space::space_store_root(&firm_home, "source-linked-space").exists());
    assert!(hidden_migration_paths(&firm_home, "source-linked-space").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}
