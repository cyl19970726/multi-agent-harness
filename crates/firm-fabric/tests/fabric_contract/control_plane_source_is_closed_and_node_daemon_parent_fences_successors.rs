use super::*;

#[test]
fn control_plane_source_is_closed_and_node_daemon_parent_fences_successors() {
    let root = TestRoot::new("source-authority-and-daemon-parent");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let target_hello = hello("node-b", "gateway-b", "cert-b", &fingerprint("node-b"));
    connect_node(
        &control,
        lease.control_plane_generation,
        &target_hello,
        &signing_key("node-b"),
        30,
    )
    .expect("connect target");

    let mut control_operation = operation(1, lease.control_plane_generation);
    control_operation.id = "operation-control-plane".into();
    control_operation.idempotency_key = "idempotency-control-plane".into();
    control_operation.source_authority = OperationSourceAuthority::ControlPlane;
    control_operation.source_node_id = None;
    control_operation.source_gateway_generation = None;
    control_operation.source_node_daemon_id = None;
    control_operation.source_node_daemon_generation = None;
    control_operation.source_execution_space_id = None;
    let (_, _, _, replayed) = control
        .accept_control_plane_operation(
            lease.control_plane_generation,
            &actor("control-service", &["fabric_submit"]),
            control_operation.clone(),
            100,
        )
        .expect("Control Plane source routes without a fabricated Node");
    assert!(!replayed);

    let before_forged = store.snapshot().expect("snapshot");
    let mut forged = control_operation;
    forged.id = "operation-forged-control-source".into();
    forged.idempotency_key = "idempotency-forged-control-source".into();
    forged.source_node_daemon_id = Some("node-daemon:forged".into());
    assert_eq!(
        control
            .accept_control_plane_operation(
                lease.control_plane_generation,
                &actor("control-service", &["fabric_submit"]),
                forged,
                101,
            )
            .expect_err("Control Plane source cannot claim Node authority")
            .code,
        FabricErrorCode::SourceMismatch
    );
    assert_eq!(store.snapshot().expect("snapshot"), before_forged);

    let source_hello = hello("node-a", "gateway-a", "cert-a", &fingerprint("node-a"));
    let source = connect_node(
        &control,
        lease.control_plane_generation,
        &source_hello,
        &signing_key("node-a"),
        102,
    )
    .expect("connect source");
    let mut stale_daemon = operation(source.gateway_generation, lease.control_plane_generation);
    stale_daemon.id = "operation-stale-daemon".into();
    stale_daemon.idempotency_key = "idempotency-stale-daemon".into();
    stale_daemon.source_node_daemon_generation = Some(source.node_daemon_generation + 1);
    let before_stale = store.snapshot().expect("snapshot");
    assert_eq!(
        control
            .accept_operation(
                lease.control_plane_generation,
                &fabric_session(
                    "node-a",
                    source.gateway_generation,
                    lease.control_plane_generation,
                ),
                &actor("fabric-client", &["fabric_submit"]),
                stale_daemon,
                103,
            )
            .expect_err("gateway lease is a child of exact NodeDaemon generation")
            .code,
        FabricErrorCode::NodeStaleGeneration
    );
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
}
