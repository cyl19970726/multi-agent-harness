use super::*;

#[test]
fn node_project_registration_is_fenced_to_selected_execution_space() {
    let (root, store) = temp_store("node-project-space-fence");
    let node_id = "00000000-0000-4000-8000-000000000001";
    store
        .insert_execution_node(&ExecutionNode {
            id: node_id.into(),
            display_name: "space-fenced-node".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert Node");
    let registration = NodeProjectRegistration {
        node_id: node_id.into(),
        execution_space_id: "other-space".into(),
        project_binding_id: "project-test".into(),
        status: NodeProjectRegistrationStatus::Active,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    let mismatch = store
        .register_node_project(&registration, "selected-space")
        .expect_err("cross-space registration must be rejected");
    assert!(mismatch
        .to_string()
        .contains("EXECUTION_SPACE_SCOPE_MISMATCH"));
    assert!(store
        .latest_node_project_registrations()
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(root).expect("remove temp store");
}
