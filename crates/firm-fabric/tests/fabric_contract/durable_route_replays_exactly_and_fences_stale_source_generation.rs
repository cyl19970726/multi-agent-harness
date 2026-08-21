use super::*;

#[test]
fn durable_route_replays_exactly_and_fences_stale_source_generation() {
    let root = TestRoot::new("route");
    let target_root = TestRoot::new("route-target");
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
    let target_session = fabric_session(
        "node-b",
        target.gateway_generation,
        lease.control_plane_generation,
    );
    target_local
        .bind_gateway_session(&target_session)
        .expect("bind target session");
    let mut request = operation(source.gateway_generation, lease.control_plane_generation);
    request.actor = actor("body-selected-admin", &["fabric_submit", "company_host"]);
    let before_hostile = store.snapshot().expect("snapshot");
    let hostile_session = fabric_session(
        "node-b",
        source.gateway_generation,
        lease.control_plane_generation,
    );
    let wrong_source = control
        .accept_operation(
            lease.control_plane_generation,
            &hostile_session,
            &actor("fabric-client", &["fabric_submit"]),
            request.clone(),
            98,
        )
        .expect_err("wire body cannot select another source Node");
    assert_eq!(wrong_source.code, FabricErrorCode::SourceMismatch);
    assert_eq!(store.snapshot().expect("snapshot"), before_hostile);
    let mut forged_actor = request.clone();
    forged_actor.id = "operation-forged-actor".into();
    forged_actor.idempotency_key = "idempotency-forged-actor".into();
    forged_actor.actor = actor("fabric-admin", &["fabric_submit", "company_host"]);
    let permission_widening = control
        .accept_operation(
            lease.control_plane_generation,
            &fabric_session(
                "node-a",
                source.gateway_generation,
                lease.control_plane_generation,
            ),
            &actor("unprivileged", &["company_viewer"]),
            forged_actor,
            98,
        )
        .expect_err("wire actor cannot widen authenticated permissions");
    assert_eq!(permission_widening.code, FabricErrorCode::UnauthorizedActor);
    assert_eq!(store.snapshot().expect("snapshot"), before_hostile);
    let mut unsupported = request.clone();
    unsupported.id = "operation-unsupported-capability".into();
    unsupported.idempotency_key = "idempotency-unsupported-capability".into();
    unsupported.kind = RUNTIME_COMMAND_REFERENCE_KIND.into();
    unsupported.body_schema = RUNTIME_COMMAND_REFERENCE_SCHEMA.into();
    unsupported.body = json!({
        "runtime_command_id": "runtime-command:unsupported",
        "intent_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "target_execution_space_id": "space-b",
        "canonical_command_intent": {
            "id": "runtime-command:unsupported",
            "target_execution_space_id": "space-b",
            "command": "resume_session",
            "idempotency_key": "unsupported",
            "expected_version": 0,
            "expires_unix_ms": 90000,
            "payload": {},
            "payload_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "issued_at": "unix-ms:100"
        }
    });
    unsupported.body_digest = json_digest(&unsupported.body).expect("runtime reference digest");
    let mut forged_target_operator = unsupported.clone();
    forged_target_operator.id = "operation-forged-target-operator".into();
    forged_target_operator.idempotency_key = "idempotency-forged-target-operator".into();
    forged_target_operator.body["canonical_command_intent"]["authenticated_actor"] = json!({
        "kind": "service",
        "id": "node-daemon:node-b"
    });
    forged_target_operator.body["canonical_command_intent"]["required_capability"] =
        json!("agent_session.stop");
    forged_target_operator.body_digest =
        json_digest(&forged_target_operator.body).expect("hostile RuntimeCommand digest");
    let before_target_impersonation = store.snapshot().expect("snapshot");
    let impersonation = control
        .accept_operation(
            lease.control_plane_generation,
            &fabric_session(
                "node-a",
                source.gateway_generation,
                lease.control_plane_generation,
            ),
            &actor("fabric-client", &["fabric_submit"]),
            forged_target_operator,
            99,
        )
        .expect_err("source Node cannot select target Operator identity or permission");
    assert_eq!(impersonation.code, FabricErrorCode::InvalidPayload);
    assert_eq!(impersonation.effect, EffectCertainty::None);
    assert_eq!(
        store.snapshot().expect("snapshot"),
        before_target_impersonation
    );
    let before_unsupported = store.snapshot().expect("snapshot");
    let unavailable = control
        .accept_operation(
            lease.control_plane_generation,
            &fabric_session(
                "node-a",
                source.gateway_generation,
                lease.control_plane_generation,
            ),
            &actor("fabric-client", &["fabric_submit"]),
            unsupported,
            99,
        )
        .expect_err("operation capability must be authorized on both Nodes");
    assert_eq!(unavailable.code, FabricErrorCode::FeatureIncompatible);
    assert_eq!(store.snapshot().expect("snapshot"), before_unsupported);
    let (canonical_request, attempt, accepted, replayed) = control
        .accept_operation(
            lease.control_plane_generation,
            &fabric_session(
                "node-a",
                source.gateway_generation,
                lease.control_plane_generation,
            ),
            &actor("fabric-client", &["fabric_submit"]),
            request.clone(),
            100,
        )
        .expect("accept operation");
    assert_eq!(canonical_request.actor.actor_id, "fabric-client");
    let request = canonical_request;
    let request_digest = json_digest(&request).expect("request digest");
    assert!(!replayed);
    assert_eq!(accepted.kind, ReceiptKind::ControlPlaneAccepted);
    let (_, _, replay_receipt, replayed) = control
        .accept_operation(
            lease.control_plane_generation,
            &fabric_session(
                "node-a",
                source.gateway_generation,
                lease.control_plane_generation,
            ),
            &actor("fabric-client", &["fabric_submit"]),
            request.clone(),
            101,
        )
        .expect("exact replay");
    assert!(replayed);
    assert_eq!(accepted, replay_receipt);
    let before_conflict = store.snapshot().expect("snapshot");
    let mut changed = request.clone();
    changed.id = "operation-changed-under-same-key".into();
    changed.body = json!({"probe": "different"});
    changed.body_digest = json_digest(&changed.body).expect("digest changed body");
    let conflict = control
        .accept_operation(
            lease.control_plane_generation,
            &fabric_session(
                "node-a",
                source.gateway_generation,
                lease.control_plane_generation,
            ),
            &actor("fabric-client", &["fabric_submit"]),
            changed,
            101,
        )
        .expect_err("same key with another fingerprint must fail closed");
    assert_eq!(conflict.code, FabricErrorCode::IdempotencyConflict);
    assert_eq!(store.snapshot().expect("snapshot"), before_conflict);

    let before_out_of_order = target_local.snapshot().expect("snapshot");
    let out_of_order = target_local
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            101,
        )
        .expect_err("application cannot precede durable target inbox");
    assert_eq!(out_of_order.code, FabricErrorCode::OperationUnknown);
    assert_eq!(
        target_local.snapshot().expect("snapshot"),
        before_out_of_order
    );
    let (inbox, replayed) = target_local
        .persist_inbox(&target_session, &request, &attempt, 102)
        .expect("persist local inbox");
    assert!(!replayed);
    let (_, persisted, replayed) = control
        .record_target_persisted(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &target.node_daemon_id,
            target.node_daemon_generation,
            &request.id,
            &request_digest,
            attempt.route_seq,
            102,
        )
        .expect("persist inbox");
    assert!(!replayed);
    assert_eq!(persisted.kind, ReceiptKind::TargetPersisted);
    target_local
        .claim_inbox(&target_session, &request, 103)
        .expect("claim before native effect");
    let (terminal_inbox, local_result, replayed) = target_local
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            103,
        )
        .expect("record result");
    assert!(!replayed);
    let (_, terminal, replayed) = control
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
            103,
        )
        .expect("record application receipt");
    assert!(!replayed);
    assert_eq!(terminal.kind, ReceiptKind::OperationApplied);
    assert_eq!(terminal.application_effect, Some(EffectCertainty::Applied));
    assert_eq!(attempt.effect, EffectCertainty::None);
    assert_eq!(terminal_inbox.state, LocalInboxState::Applied);
    let state = store.snapshot().expect("snapshot");
    assert_eq!(state.operations.len(), 1);
    assert_eq!(terminal.kind, ReceiptKind::OperationApplied);
    assert_eq!(inbox.request_digest, request_digest);

    control
        .heartbeat_lease(lease.control_plane_generation, lease.revision, 29_000)
        .expect("keep Control Plane alive");
    let successor_hello = rotate_gateway_certificate(
        &control,
        &store,
        lease.control_plane_generation,
        &source,
        "node-a",
        "cert-a",
        "cert-a-successor",
        29_500,
    );
    let stale_hello = hello(
        "node-a",
        "gateway-stale-daemon",
        "cert-a",
        &fingerprint("node-a"),
    );
    let before_stale_reconnect = store.snapshot().expect("before stale reconnect");
    let stale_reconnect = connect_node(
        &control,
        lease.control_plane_generation,
        &stale_hello,
        &signing_key("node-a"),
        30_031,
    )
    .expect_err("expired gateway cannot self-report its predecessor NodeDaemon authority");
    assert_eq!(stale_reconnect.code, FabricErrorCode::UnauthorizedActor);
    assert_eq!(stale_reconnect.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().unwrap(), before_stale_reconnect);
    let successor = connect_node(
        &control,
        lease.control_plane_generation,
        &successor_hello,
        &signing_key("node-a"),
        30_031,
    )
    .expect("source successor after lease expiry");
    assert_eq!(successor.gateway_generation, source.gateway_generation + 1);
    let before = store.snapshot().expect("snapshot");
    let mut stale = operation(source.gateway_generation, lease.control_plane_generation);
    stale.id = "operation-stale".into();
    stale.idempotency_key = "idempotency-stale".into();
    let error = control
        .accept_operation(
            lease.control_plane_generation,
            &fabric_session(
                "node-a",
                source.gateway_generation,
                lease.control_plane_generation,
            ),
            &actor("fabric-client", &["fabric_submit"]),
            stale,
            30_032,
        )
        .expect_err("stale source generation must fail");
    assert_eq!(error.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}
