use super::*;

#[test]
fn control_plane_successor_immediately_fences_prior_live_gateway_generation() {
    let root = TestRoot::new("control-plane-takeover");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let old = ControlPlane::new(COMPANY, "control-old", &store, &keys, [9; 32]);
    let first = old.acquire_lease("cp-lease-1", 0, 1).expect("first lease");
    enroll_nodes(&old, first.control_plane_generation);
    let hello_one = hello("node-a", "gateway-a-1", "cert-a", &fingerprint("node-a"));
    let gateway_one = connect_node(
        &old,
        first.control_plane_generation,
        &hello_one,
        &signing_key("node-a"),
        30,
    )
    .expect("first gateway");
    old.heartbeat_gateway(
        first.control_plane_generation,
        "node-a",
        gateway_one.gateway_generation,
        &gateway_one.node_daemon_id,
        gateway_one.node_daemon_generation,
        1,
        29_000,
    )
    .expect("old gateway lease remains live past Control Plane expiry");
    let successor = ControlPlane::new(COMPANY, "control-new", &store, &keys, [9; 32]);
    let second = successor
        .acquire_lease("cp-lease-2", first.revision, 30_001)
        .expect("Control Plane successor");
    let hello_two = hello("node-a", "gateway-a-2", "cert-a", &fingerprint("node-a"));
    let gateway_two = connect_node(
        &successor,
        second.control_plane_generation,
        &hello_two,
        &signing_key("node-a"),
        30_002,
    )
    .expect("new Control Plane generation fences old live gateway immediately");
    assert_eq!(
        gateway_two.gateway_generation,
        gateway_one.gateway_generation + 1
    );
    let before = store.snapshot().expect("snapshot");
    let stale = old
        .heartbeat_gateway(
            first.control_plane_generation,
            "node-a",
            gateway_one.gateway_generation,
            &gateway_one.node_daemon_id,
            gateway_one.node_daemon_generation,
            2,
            30_003,
        )
        .expect_err("old Control Plane and gateway generations are fenced");
    assert_eq!(stale.code, FabricErrorCode::ControlPlaneStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}
