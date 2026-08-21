use super::*;

#[test]
fn supervisor_lease_rejects_caller_selected_foreign_execution_space_without_write() {
    let root = team_test_root("lease-space-fence");
    let store = HarnessStore::new(&root);
    seed_lease_run(&store, "run-a");
    let parent = store
        .acquire_node_daemon_lease(
            "00000000-0000-4000-8000-000000000001",
            "daemon-test",
            "instance-test",
            1_000,
            60_000,
        )
        .expect("parent lease");
    let before = store
        .team_supervisor_leases()
        .expect("read Supervisor leases");
    let error = store
        .acquire_team_supervisor_under_node_lease(
            "run-a",
            "00000000-0000-4000-8000-000000000001",
            &parent.daemon_id,
            parent.generation,
            "foreign-space",
            "project-test",
            "supervisor-hostile",
            1,
            "tcp://127.0.0.1:1",
            1_001,
            60_000,
        )
        .expect_err("caller-selected foreign Execution Space must fail closed");
    assert!(error.to_string().contains("EXECUTION_SPACE_SCOPE_MISMATCH"));
    assert_eq!(
        store
            .team_supervisor_leases()
            .expect("read Supervisor leases after rejection"),
        before
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
