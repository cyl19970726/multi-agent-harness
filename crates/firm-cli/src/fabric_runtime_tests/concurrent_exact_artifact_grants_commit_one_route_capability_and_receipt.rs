use super::*;

#[test]
fn concurrent_exact_artifact_grants_commit_one_route_capability_and_receipt() {
    let root = std::env::temp_dir().join(format!(
        "agentfirm-artifact-grant-concurrent-replay-{}-{}",
        std::process::id(),
        now_unix_ms().unwrap()
    ));
    let collaboration_root = root.join("collaboration");
    let fabric_root = root.join("fabric");
    std::fs::create_dir_all(&root).unwrap();
    let (attestation, delegation, policy, publication, _) = current_remote_fact_fixture();
    seed_current_remote_fact_authority(&collaboration_root, &attestation, &delegation, &policy);

    let fabric_store = harness_fabric::FabricStore::open(&fabric_root).unwrap();
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(&delegation.company_id, [7; 32]);
    let control = ControlPlane::new(
        &delegation.company_id,
        "control-artifact-replay",
        &fabric_store,
        &keys,
        [9; 32],
    );
    let lease = control
        .acquire_lease("lease-artifact-replay", 0, 100)
        .unwrap();
    let schema_digest = remote_fabric_schema_bundle_digest();
    let artifact_capability =
        harness_core::collaboration::RoutedBusinessKind::ArtifactGrant.required_capability();
    let allowed_capabilities = BTreeSet::from([
        "artifact-transfer".into(),
        "cross-team-collaboration".into(),
        "durable-routing".into(),
        artifact_capability,
    ]);
    let company_host = AuthenticatedActor {
        company_id: delegation.company_id.clone(),
        actor_id: "company-host:test".into(),
        actor_kind: harness_fabric::ActorKind::Human,
        role_bindings: BTreeSet::from(["company_host".into()]),
        session_id: "company-host:test".into(),
        issued_at_unix_ms: 100,
        expires_at_unix_ms: 20_000,
    };
    let mut source_certificate = None;
    for node_id in [
        &delegation.source_node_id,
        &delegation.target_placement.node_id,
    ] {
        let enrollment_id = format!("enrollment-{node_id}");
        let token = format!("artifact-replay-token-{node_id}-00000000000000000000");
        let daemon_id = format!("daemon-{node_id}");
        control
            .create_enrollment_bound(
                &company_host,
                lease.control_plane_generation,
                &enrollment_id,
                &token,
                node_id,
                allowed_capabilities.clone(),
                &daemon_id,
                1,
                10_000,
                110,
            )
            .unwrap();
        let csr = harness_fabric::pki::generate_node_csr(&delegation.company_id, node_id).unwrap();
        let (_, certificate) = control
            .consume_enrollment_csr(
                lease.control_plane_generation,
                &token,
                node_id,
                node_id,
                &csr.csr_pem,
                &format!("certificate-{node_id}"),
                20_000,
                &schema_digest,
                120,
            )
            .unwrap();
        if node_id == &delegation.source_node_id {
            source_certificate = Some(certificate);
        }
    }
    let source_certificate = source_certificate.unwrap();
    let source_hello = NodeHello {
        company_id: delegation.company_id.clone(),
        node_id: delegation.source_node_id.clone(),
        instance_id: "gateway-source".into(),
        node_daemon_id: format!("daemon-{}", delegation.source_node_id),
        node_daemon_generation: 1,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: schema_digest.clone(),
        features: allowed_capabilities,
        build_sha: "artifact-replay-test-build".into(),
        last_persisted_route_seq: 0,
        unresolved_operation_ids: BTreeSet::new(),
        certificate_serial: source_certificate.serial.clone(),
        public_key_fingerprint: source_certificate.public_key_fingerprint.clone(),
    };
    control
        .connect_gateway_mtls(
            lease.control_plane_generation,
            &harness_fabric::transport::VerifiedMtlsPeer {
                company_id: source_hello.company_id.clone(),
                node_id: source_hello.node_id.clone(),
                certificate_serial: source_hello.certificate_serial.clone(),
                public_key_fingerprint: source_hello.public_key_fingerprint.clone(),
                tls_version: "TLS1.3".into(),
                websocket_subprotocol: harness_fabric::transport::FABRIC_WEBSOCKET_SUBPROTOCOL
                    .into(),
            },
            &source_hello,
            130,
        )
        .unwrap();

    let artifact_id = "artifact-concurrent-replay";
    let artifact_bytes = b"hello";
    let artifact_writer = AuthenticatedActor {
        company_id: delegation.company_id.clone(),
        actor_id: delegation.target_host_ref.id.clone(),
        actor_kind: harness_fabric::ActorKind::AgentMember,
        role_bindings: BTreeSet::from(["artifact_write".into()]),
        session_id: "artifact-writer:test".into(),
        issued_at_unix_ms: 140,
        expires_at_unix_ms: 10_000,
    };
    let (_, upload_capability) = control
        .initiate_collaboration_artifact(
            &artifact_writer,
            lease.control_plane_generation,
            artifact_id,
            &delegation.target_placement.node_id,
            &delegation.target_placement.team_id,
            &publication.fact_work_ref.work_id,
            None,
            "text/plain",
            artifact_bytes.len() as u64,
            &harness_fabric::sha256_hex(artifact_bytes),
            harness_fabric::ArtifactClassification::CompanyInternal,
            BTreeSet::from([attestation.source_host_ref.id.clone()]),
            150,
        )
        .unwrap();
    control
        .complete_artifact(
            lease.control_plane_generation,
            &upload_capability,
            artifact_bytes,
            160,
        )
        .unwrap();

    let request = CollaborationArtifactGrantHttpRequest {
        target_execution_space_id: delegation.source_work_ref.execution_space_id.clone(),
        expires_unix_ms: 10_000,
    };
    let control_actor = AuthenticatedActor {
        company_id: delegation.company_id.clone(),
        actor_id: format!("control-plane:{}", lease.control_plane_generation),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["company_control_plane".into(), "fabric_submit".into()]),
        session_id: format!("control-plane:{}", lease.control_plane_generation),
        issued_at_unix_ms: 200,
        expires_at_unix_ms: 20_000,
    };
    let start = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            start.wait();
            grant_collaboration_artifact(
                &HarnessStore::new(&collaboration_root),
                &control,
                lease.control_plane_generation,
                &delegation.id,
                artifact_id,
                &request,
                &delegation.target_host_ref,
                &control_actor,
                "grant-concurrent-replay-key",
                delegation.revision,
                300,
            )
        });
        let second = scope.spawn(|| {
            start.wait();
            grant_collaboration_artifact(
                &HarnessStore::new(&collaboration_root),
                &control,
                lease.control_plane_generation,
                &delegation.id,
                artifact_id,
                &request,
                &delegation.target_host_ref,
                &control_actor,
                "grant-concurrent-replay-key",
                delegation.revision,
                301,
            )
        });
        [
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
        ]
    });
    let first_receipt: RouteReceipt =
        serde_json::from_value(results[0]["receipt"].clone()).unwrap();
    let second_receipt: RouteReceipt =
        serde_json::from_value(results[1]["receipt"].clone()).unwrap();
    assert_eq!(
        first_receipt, second_receipt,
        "exact retries return one receipt"
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| result["replayed"] == serde_json::json!(true))
            .count(),
        1,
        "exactly one concurrent request must be the frozen replay"
    );
    let state = fabric_store.snapshot().unwrap();
    assert_eq!(state.operations.len(), 1, "one durable route");
    assert_eq!(state.attempts.len(), 1, "one durable route attempt");
    assert_eq!(
        state
            .receipts
            .values()
            .filter(|receipt| receipt.kind == ReceiptKind::ControlPlaneAccepted)
            .count(),
        1,
        "one durable accepted receipt"
    );
    let operation = state.operations.values().next().unwrap();
    let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
        operation.closed_body().unwrap()
    else {
        panic!("artifact grant route must remain a closed collaboration operation")
    };
    let payload: CollaborationArtifactGrantEnvelope =
        serde_json::from_value(reference.payload).unwrap();
    assert_eq!(payload.read_capability.artifact_id, artifact_id);
    assert!(payload.read_capability.one_use, "one one-use capability");
    std::fs::remove_dir_all(root).unwrap();
}
