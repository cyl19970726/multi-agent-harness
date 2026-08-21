use super::*;

#[test]
fn successor_reconnect_settles_expired_offline_operation_as_not_applied() {
    let root = TestRoot::new("expired-successor-control");
    let target_root = TestRoot::new("expired-successor-target");
    let store = FabricStore::open(root.path()).expect("open store");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let source = connect_node(
        &control,
        lease.control_plane_generation,
        &hello("node-a", "gateway-a", "cert-a", &fingerprint("node-a")),
        &signing_key("node-a"),
        30,
    )
    .expect("source connect");
    let target = connect_node(
        &control,
        lease.control_plane_generation,
        &hello("node-b", "gateway-b", "cert-b", &fingerprint("node-b")),
        &signing_key("node-b"),
        30,
    )
    .expect("target connect");
    let mut request = operation(source.gateway_generation, lease.control_plane_generation);
    request.expires_at_unix_ms = 200;
    let (_, first_attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        request.clone(),
        100,
    )
    .expect("queue while target is offline");
    let target_before = target_local.snapshot().expect("target before reconnect");
    control
        .heartbeat_lease(lease.control_plane_generation, lease.revision, 29_000)
        .expect("keep Control Plane alive");
    let successor_hello = rotate_gateway_certificate(
        &control,
        &store,
        lease.control_plane_generation,
        &target,
        "node-b",
        "cert-b",
        "cert-b-successor",
        29_500,
    );
    let successor = connect_node(
        &control,
        lease.control_plane_generation,
        &successor_hello,
        &signing_key("node-b"),
        30_031,
    )
    .expect("successor reconnect settles expiry");
    let state = store.snapshot().expect("settled state");
    let attempt = state
        .attempts
        .get(&first_attempt.id)
        .expect("first attempt");
    assert_eq!(attempt.state, RouteAttemptState::Ended);
    assert_eq!(attempt.error_code, Some(FabricErrorCode::OperationExpired));
    assert_eq!(attempt.effect, EffectCertainty::None);
    assert_eq!(attempt.ended_at_unix_ms, Some(30_031));
    let receipt = state
        .receipts
        .values()
        .find(|receipt| {
            receipt.operation_id == request.id && receipt.kind == ReceiptKind::OperationRejected
        })
        .expect("typed terminal expiry receipt");
    assert_eq!(
        receipt.target_gateway_generation,
        successor.gateway_generation
    );
    assert_eq!(
        receipt.application_effect,
        Some(EffectCertainty::NotApplied)
    );
    assert_eq!(
        receipt.error.as_ref().map(|error| error.code),
        Some(FabricErrorCode::OperationExpired)
    );
    assert_eq!(
        receipt.result_schema.as_deref(),
        Some("agentfirm.remote_fabric.expired.v1")
    );
    assert_eq!(
        target_local.snapshot().expect("target after reconnect"),
        target_before
    );
}
