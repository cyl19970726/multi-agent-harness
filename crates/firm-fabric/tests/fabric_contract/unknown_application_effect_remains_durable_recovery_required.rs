use super::*;

#[test]
fn unknown_application_effect_remains_durable_recovery_required() {
    let root = TestRoot::new("unknown-recovery-control");
    let source_root = TestRoot::new("unknown-recovery-source");
    let target_root = TestRoot::new("unknown-recovery-target");
    let store = FabricStore::open(root.path()).expect("open store");
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
    let source_session = fabric_session(
        "node-a",
        source.gateway_generation,
        lease.control_plane_generation,
    );
    let target_session = fabric_session(
        "node-b",
        target.gateway_generation,
        lease.control_plane_generation,
    );
    let source_local =
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a").expect("source local");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target local");
    source_local
        .bind_gateway_session(&source_session)
        .expect("bind source");
    target_local
        .bind_gateway_session(&target_session)
        .expect("bind target");
    let request = operation(source.gateway_generation, lease.control_plane_generation);
    source_local
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            99,
        )
        .expect("prepare outbox");
    let (_, attempt, accepted, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        request.clone(),
        100,
    )
    .expect("accept route");
    source_local
        .mark_outbox_receipt(&source_session, &accepted)
        .expect("record accepted");
    target_local
        .persist_inbox(&target_session, &request, &attempt, 101)
        .expect("persist target");
    control
        .record_target_persisted(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &request.id,
            &json_digest(&request).expect("request digest"),
            attempt.route_seq,
            101,
        )
        .expect("record target persistence");
    target_local
        .claim_inbox(&target_session, &request, 102)
        .expect("claim target");
    let (local_inbox, local_result, _) = target_local
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"error":"transport outcome cannot be proven"}),
            EffectCertainty::Unknown,
            103,
        )
        .expect("persist unknown result");
    assert_eq!(local_inbox.state, LocalInboxState::RecoveryRequired);
    let (attempt, receipt, replayed) = control
        .record_application_receipt(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &request.id,
            &local_result.result_schema,
            local_result.result,
            local_result.effect,
            104,
        )
        .expect("Control Plane preserves unknown result");
    assert!(!replayed);
    assert_eq!(attempt.state, RouteAttemptState::TargetPersisted);
    assert_eq!(attempt.ended_at_unix_ms, None);
    assert_eq!(receipt.kind, ReceiptKind::RecoveryRequired);
    assert_eq!(receipt.application_effect, Some(EffectCertainty::Unknown));
    let outbox = source_local
        .mark_outbox_receipt(&source_session, &receipt)
        .expect("source converges to reconciliation required");
    assert_eq!(outbox.local_state, LocalOutboxState::ReconcileRequired);
    let diagnostics = inspect_fabric(&store, COMPANY, 105).expect("diagnostics");
    assert!(diagnostics.nodes.iter().any(|node| {
        node.node_id == "node-b" && node.recovery_required_operations.contains(&request.id)
    }));
}
