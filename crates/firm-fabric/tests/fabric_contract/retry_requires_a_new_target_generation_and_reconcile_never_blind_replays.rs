use super::*;

#[test]
fn retry_requires_a_new_target_generation_and_reconcile_never_blind_replays() {
    let root = TestRoot::new("retry-reconcile");
    let target_root = TestRoot::new("retry-reconcile-target");
    let store = FabricStore::open(root.path()).expect("open store");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let source_hello = hello("node-a", "gateway-a-1", "cert-a", &fingerprint("node-a"));
    let source = connect_node(
        &control,
        lease.control_plane_generation,
        &source_hello,
        &signing_key("node-a"),
        30,
    )
    .expect("source connect");
    let target_hello = hello("node-b", "gateway-b-1", "cert-b", &fingerprint("node-b"));
    let target = connect_node(
        &control,
        lease.control_plane_generation,
        &target_hello,
        &signing_key("node-b"),
        30,
    )
    .expect("target connect");
    let request = operation(source.gateway_generation, lease.control_plane_generation);
    let request_digest = json_digest(&request).expect("request digest");
    let (_, first_attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        request.clone(),
        100,
    )
    .expect("accept operation");
    let same_generation = control
        .retry_operation(lease.control_plane_generation, &request.id, 101)
        .expect("same target generation is exact replay");
    assert!(same_generation.2);
    assert_eq!(same_generation.0, first_attempt);

    let cp_second = control
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
    .expect("target successor");
    let mut successor_session = fabric_session(
        "node-b",
        successor.gateway_generation,
        lease.control_plane_generation,
    );
    successor_session.node_daemon_generation = 2;
    target_local
        .bind_gateway_session(&successor_session)
        .expect("bind target successor session");
    let rebound_state = store.snapshot().expect("successor rebound snapshot");
    let auto_rebound = rebound_state
        .attempts
        .values()
        .find(|attempt| attempt.operation_id == request.id && attempt.attempt_no == 2)
        .expect("successor connect atomically rebinds effect-none attempt");
    assert_eq!(
        auto_rebound.target_gateway_generation,
        successor.gateway_generation
    );
    let (second_attempt, _, replayed) = control
        .retry_operation(lease.control_plane_generation, &request.id, 30_032)
        .expect("explicit retry replays automatic successor binding");
    assert!(replayed);
    assert_eq!(second_attempt.attempt_no, 2);
    assert_eq!(
        second_attempt.target_gateway_generation,
        successor.gateway_generation
    );
    let before_stale = store.snapshot().expect("snapshot");
    let stale = control
        .record_target_persisted(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &request.id,
            &request_digest,
            first_attempt.route_seq,
            30_033,
        )
        .expect_err("expired predecessor cannot persist after successor takeover");
    assert_eq!(stale.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
    target_local
        .persist_inbox(&successor_session, &request, &second_attempt, 30_034)
        .expect("successor persists local inbox");
    control
        .record_target_persisted(
            lease.control_plane_generation,
            "node-b",
            successor.gateway_generation,
            &successor.node_daemon_id,
            successor.node_daemon_generation,
            &request.id,
            &request_digest,
            second_attempt.route_seq,
            30_034,
        )
        .expect("successor persists inbox");
    target_local
        .claim_inbox(&successor_session, &request, 30_035)
        .expect("successor claims before native effect");
    let (_, local_result, _) = target_local
        .record_application_result(
            &successor_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            30_035,
        )
        .expect("successor records terminal result");
    let (_, terminal, _) = control
        .record_application_receipt(
            lease.control_plane_generation,
            "node-b",
            successor.gateway_generation,
            &successor.node_daemon_id,
            successor.node_daemon_generation,
            &request.id,
            &local_result.result_schema,
            local_result.result,
            local_result.effect,
            30_035,
        )
        .expect("Control Plane records terminal receipt");

    let cp_third = control
        .heartbeat_lease(lease.control_plane_generation, cp_second.revision, 58_000)
        .expect("keep Control Plane alive for reconciliation");
    assert_eq!(
        cp_third.control_plane_generation,
        lease.control_plane_generation
    );
    let third_hello = rotate_gateway_certificate(
        &control,
        &store,
        lease.control_plane_generation,
        &successor,
        "node-b",
        "cert-b-successor",
        "cert-b-third",
        59_000,
    );
    let third = connect_node(
        &control,
        lease.control_plane_generation,
        &third_hello,
        &signing_key("node-b"),
        60_032,
    )
    .expect("next target generation");
    let reconciled = control
        .reconcile(
            lease.control_plane_generation,
            "node-b",
            third.gateway_generation,
            &third.node_daemon_id,
            third.node_daemon_generation,
            &BTreeSet::from([request.id]),
            60_033,
        )
        .expect("successor reconciles durable prior-generation terminal receipt");
    assert_eq!(reconciled, vec![terminal]);
}
