use super::*;

#[test]
fn node_local_journal_verifies_committed_wire_digest_across_defaulted_field_upgrade() {
    let root = TestRoot::new("node-local-wire-digest-upgrade");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a")
        .expect("create node-local store");
    let session = FabricSessionFence {
        company_id: COMPANY.into(),
        node_id: "node-a".into(),
        node_daemon_id: "node-daemon:node-a".into(),
        node_daemon_generation: 1,
        gateway_generation: 1,
        control_plane_generation: 1,
    };
    store.bind_gateway_session(&session).expect("write frame");
    drop(store);

    let journal = root.path().join("node-fabric-transactions.jsonl");
    let mut frame: serde_json::Value =
        serde_json::from_str(std::fs::read_to_string(&journal).unwrap().trim()).unwrap();
    frame["state"]
        .as_object_mut()
        .unwrap()
        .remove("ordering_tombstones");
    let mut core = frame.clone();
    core.as_object_mut().unwrap().remove("frame_digest");
    frame["frame_digest"] = json!(json_digest(&core).unwrap());
    std::fs::write(
        &journal,
        format!("{}\n", serde_json::to_string(&frame).unwrap()),
    )
    .unwrap();

    let reopened = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a")
        .expect("valid historical wire digest must survive a defaulted-field upgrade");
    assert_eq!(reopened.snapshot().unwrap().active_session, Some(session));
}
