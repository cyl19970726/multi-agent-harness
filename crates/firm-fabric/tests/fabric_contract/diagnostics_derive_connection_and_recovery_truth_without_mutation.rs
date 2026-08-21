use super::*;

#[test]
fn diagnostics_derive_connection_and_recovery_truth_without_mutation() {
    let root = TestRoot::new("diagnostics");
    let target_root = TestRoot::new("diagnostics-target");
    let store = FabricStore::open(root.path()).expect("open store");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let source_hello = hello("node-a", "gateway-a", "cert-a", &fingerprint("node-a"));
    let source = connect_node(
        &control,
        lease.control_plane_generation,
        &source_hello,
        &signing_key("node-a"),
        30,
    )
    .expect("source connect");
    let target_hello = hello("node-b", "gateway-b", "cert-b", &fingerprint("node-b"));
    let target = connect_node(
        &control,
        lease.control_plane_generation,
        &target_hello,
        &signing_key("node-b"),
        30,
    )
    .expect("target connect");
    let target_session = fabric_session(
        "node-b",
        target.gateway_generation,
        lease.control_plane_generation,
    );
    target_local
        .bind_gateway_session(&target_session)
        .expect("bind target session");
    let request = operation(source.gateway_generation, lease.control_plane_generation);
    let request_digest = json_digest(&request).expect("request digest");
    let (_, attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        request.clone(),
        100,
    )
    .expect("route operation");
    target_local
        .persist_inbox(&target_session, &request, &attempt, 101)
        .expect("persist local target inbox");
    control
        .record_target_persisted(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &request.id,
            &request_digest,
            attempt.route_seq,
            101,
        )
        .expect("persist target inbox");
    control
        .mark_unknown(lease.control_plane_generation, &request.id, 102)
        .expect("mark unknown after transport loss");
    let before = store.snapshot().expect("snapshot");
    let report = inspect_fabric(&store, COMPANY, 103).expect("read diagnostics");
    assert!(report.control_plane_online);
    assert_eq!(report.recovery_required_count, 1);
    assert_eq!(report.nodes.len(), 2);
    let target_report = report
        .nodes
        .iter()
        .find(|node| node.node_id == "node-b")
        .expect("target diagnostics");
    assert_eq!(
        target_report.connection_status,
        NodeConnectionStatus::Online
    );
    assert_eq!(target_report.recovery_required_operations, vec![request.id]);
    assert_eq!(target_report.last_persisted_route_seq, attempt.route_seq);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}
