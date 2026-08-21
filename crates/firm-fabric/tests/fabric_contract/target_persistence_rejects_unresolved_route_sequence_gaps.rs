use super::*;

#[test]
fn target_persistence_rejects_unresolved_route_sequence_gaps() {
    let root = TestRoot::new("route-ordering");
    let target_root = TestRoot::new("route-ordering-target");
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
    let first = operation(source.gateway_generation, lease.control_plane_generation);
    let (_, first_attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        first.clone(),
        100,
    )
    .expect("first route");
    let mut second = first.clone();
    second.id = "operation-2".into();
    second.idempotency_key = "idempotency-2".into();
    let (_, second_attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        second.clone(),
        101,
    )
    .expect("second route");
    assert_eq!(second_attempt.route_seq, first_attempt.route_seq + 1);
    let before = target_local.snapshot().expect("snapshot");
    let gap = target_local
        .persist_inbox(&target_session, &second, &second_attempt, 102)
        .expect_err("seq=2 cannot pass unresolved seq=1");
    assert_eq!(gap.code, FabricErrorCode::ExpectedRevisionConflict);
    assert_eq!(target_local.snapshot().expect("snapshot"), before);
    target_local
        .persist_inbox(&target_session, &first, &first_attempt, 102)
        .expect("persist seq=1");
    target_local
        .persist_inbox(&target_session, &second, &second_attempt, 102)
        .expect("persist seq=2 after seq=1");
    assert_eq!(
        target_local
            .snapshot()
            .expect("snapshot")
            .persisted_ordering_sequences[&second.ordering_key],
        second_attempt.ordering_seq
    );
}
