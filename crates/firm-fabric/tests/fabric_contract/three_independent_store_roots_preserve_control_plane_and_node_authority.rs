use super::*;

#[test]
fn three_independent_store_roots_preserve_control_plane_and_node_authority() {
    let control_root = TestRoot::new("three-root-control");
    let source_root = TestRoot::new("three-root-source");
    let target_root = TestRoot::new("three-root-target");
    let store = FabricStore::open(control_root.path()).expect("open Control Plane store");
    let source_local =
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a").expect("source store");
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
    source_local
        .bind_gateway_session(&source_session)
        .expect("bind source session");
    target_local
        .bind_gateway_session(&target_session)
        .expect("bind target session");
    let request = operation(source.gateway_generation, lease.control_plane_generation);
    let request_digest = json_digest(&request).expect("request digest");
    let (source_outbox, replayed) = source_local
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            90,
        )
        .expect("source durably prepares before submission");
    assert!(!replayed);
    assert_eq!(
        source_outbox.local_state,
        LocalOutboxState::QueuedForControlPlane
    );
    assert_eq!(source_outbox.attempt_count, 0);
    source_local
        .mark_outbox_submitted(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            99,
        )
        .expect("live gateway begins exact submission");
    let (_, attempt, accepted, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        request.clone(),
        100,
    )
    .expect("Control Plane accepts and journals route");
    source_local
        .mark_outbox_receipt(&source_session, &accepted)
        .expect("source records Control Plane acceptance");
    let (target_inbox, replayed) = target_local
        .persist_inbox(&target_session, &request, &attempt, 101)
        .expect("target local Store persists before acknowledgement");
    assert!(!replayed);
    assert_eq!(target_inbox.state, LocalInboxState::Persisted);
    let (_, persisted, _) = control
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
        .expect("Control Plane records transport receipt only");
    source_local
        .mark_outbox_receipt(&source_session, &persisted)
        .expect("source records target persistence");
    target_local
        .claim_inbox(&target_session, &request, 102)
        .expect("target claims before native effect");
    let (_, local_result, _) = target_local
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            102,
        )
        .expect("target application result is local authority");
    let (_, terminal, _) = control
        .record_application_receipt(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &request.id,
            &local_result.result_schema,
            local_result.result.clone(),
            local_result.effect,
            103,
        )
        .expect("Control Plane records terminal receipt");
    let terminal_outbox = source_local
        .mark_outbox_receipt(&source_session, &terminal)
        .expect("source converges terminal outbox");
    assert_eq!(terminal_outbox.local_state, LocalOutboxState::Terminal);

    let control_state = store.snapshot().expect("Control Plane snapshot");
    assert_eq!(control_state.operations.len(), 1);
    assert_eq!(control_state.receipts.len(), 3);
    let mut source_state = source_local.snapshot().expect("source snapshot");
    assert_eq!(source_state.outboxes.len(), 1);
    assert!(source_state.inboxes.is_empty());
    let target_state = target_local.snapshot().expect("target snapshot");
    assert_eq!(target_state.inboxes.len(), 1);
    assert_eq!(target_state.results.len(), 1);
    assert!(target_state.outboxes.is_empty());

    let mut independent = request;
    independent.id = "operation-target-store-unavailable".into();
    independent.idempotency_key = "idempotency-target-store-unavailable".into();
    source_local
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &independent,
            104,
        )
        .expect("healthy source Node remains writable");
    let (_, independent_attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        independent.clone(),
        105,
    )
    .expect("Control Plane remains healthy when one Node Store fails");
    target_local.fail_next_commit_for_test();
    let unavailable = target_local
        .persist_inbox(&target_session, &independent, &independent_attempt, 105)
        .expect_err("target Store failure is isolated and explicit");
    assert_eq!(unavailable.code, FabricErrorCode::StoreUnavailable);
    assert_eq!(
        target_local.snapshot().expect("target snapshot"),
        target_state
    );
    source_state = source_local.snapshot().expect("healthy source snapshot");
    assert_eq!(source_state.outboxes.len(), 2);
    drop(source_local);
    drop(target_local);
    assert_eq!(
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a")
            .expect("reopen source")
            .snapshot()
            .expect("source snapshot"),
        source_state
    );
    assert_eq!(
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b")
            .expect("reopen target")
            .snapshot()
            .expect("target snapshot"),
        target_state
    );
}
