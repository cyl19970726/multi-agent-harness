use super::*;

#[test]
fn draining_rejects_new_target_work_but_preserves_inflight_completion() {
    let root = TestRoot::new("draining");
    let target_root = TestRoot::new("draining-target");
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
    let first_digest = json_digest(&first).expect("request digest");
    let (_, attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        first.clone(),
        100,
    )
    .expect("accept inflight operation before drain");
    let host = actor("host", &["company_host"]);
    let drained = control
        .set_node_administrative_status(
            &host,
            lease.control_plane_generation,
            "node-b",
            1,
            NodeAdministrativeStatus::Draining,
            101,
        )
        .expect("drain target Node");
    assert_eq!(
        drained.administrative_status,
        NodeAdministrativeStatus::Draining
    );
    let before_new = store.snapshot().expect("snapshot");
    let mut second = first.clone();
    second.id = "operation-after-drain".into();
    second.idempotency_key = "idempotency-after-drain".into();
    let rejected = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        second,
        102,
    )
    .expect_err("draining target rejects new operation");
    assert_eq!(rejected.code, FabricErrorCode::TargetNotPlaced);
    assert_eq!(store.snapshot().expect("snapshot"), before_new);
    target_local
        .persist_inbox(&target_session, &first, &attempt, 103)
        .expect("drain preserves local persistence");
    control
        .record_target_persisted(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &first.id,
            &first_digest,
            attempt.route_seq,
            103,
        )
        .expect("drain does not corrupt inflight operation");
    target_local
        .claim_inbox(&target_session, &first, 104)
        .expect("claim inflight operation before native effect");
    let (_, local_result, _) = target_local
        .record_application_result(
            &target_session,
            &first.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            104,
        )
        .expect("inflight operation reaches terminal state during drain");
    control
        .record_application_receipt(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &first.id,
            &local_result.result_schema,
            local_result.result,
            local_result.effect,
            104,
        )
        .expect("Control Plane records terminal result during drain");
}
