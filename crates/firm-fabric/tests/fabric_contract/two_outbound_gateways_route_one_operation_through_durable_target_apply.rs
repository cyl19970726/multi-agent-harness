use super::*;

#[test]
fn two_outbound_gateways_route_one_operation_through_durable_target_apply() {
    use firm_fabric::gateway_runtime::{
        serve_control_plane_session, NodeGatewayConnection, ProbeApplication,
    };
    use firm_fabric::transport::{
        accept_control_plane_mtls, ControlPlaneTlsFiles, NodeFabricConfig, NodeTlsIdentityFiles,
    };

    let root = TestRoot::new("two-live-gateways");
    let company = COMPANY;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64;
    let ca = firm_fabric::pki::generate_ca(company).expect("generate CA");
    let server_certificate =
        firm_fabric::pki::issue_control_plane_server_certificate(&ca, "localhost", now)
            .expect("issue Control Plane certificate");
    let ca_path = root.path().join("ca.pem");
    let server_cert_path = root.path().join("server.pem");
    let server_key_path = root.path().join("server-key.pem");
    std::fs::write(&ca_path, &ca.certificate_pem).unwrap();
    std::fs::write(&server_cert_path, &server_certificate.certificate_chain_pem).unwrap();
    std::fs::write(&server_key_path, &server_certificate.private_key_pem).unwrap();
    secure_private_key(&server_key_path);

    let fabric_root = root.path().join("control");
    let store = FabricStore::open(&fabric_root).expect("Control Plane Store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(company, [7; 32]);
    let control = ControlPlane::new(company, "control-live", &store, &keys, [9; 32]);
    let lease = control
        .acquire_lease("control-live-lease", 0, now)
        .expect("acquire Control Plane lease");
    let host = AuthenticatedActor {
        company_id: company.into(),
        actor_id: "host-live".into(),
        actor_kind: ActorKind::Human,
        role_bindings: BTreeSet::from([
            "company_host".into(),
            "artifact_write".into(),
            "artifact_read".into(),
        ]),
        session_id: "host-live-session".into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
    };

    let mut node_materials = Vec::new();
    for (index, node_id) in ["node-a", "node-b"].into_iter().enumerate() {
        let csr = firm_fabric::pki::generate_node_csr(company, node_id).expect("Node CSR");
        let issued =
            firm_fabric::pki::issue_node_certificate(&ca, &csr.csr_pem, company, node_id, now)
                .expect("Node certificate");
        let token = format!("live-enrollment-token-{node_id}-0000000000000000");
        let enrollment_id = format!("enrollment-{node_id}");
        control
            .create_enrollment(
                &host,
                lease.control_plane_generation,
                &enrollment_id,
                &token,
                node_id,
                BTreeSet::from(["durable-routing".into(), "artifact-transfer".into()]),
                now + 60_000,
                now + index as u64,
            )
            .expect("create enrollment");
        let challenge = firm_fabric::enrollment::enrollment_challenge(
            company,
            &enrollment_id,
            node_id,
            &issued.serial,
            SCHEMA_DIGEST,
        );
        let proof =
            firm_fabric::pki::enrollment_proof_from_node_key(&csr.private_key_pem, challenge)
                .expect("Node proof");
        control
            .consume_enrollment(
                lease.control_plane_generation,
                &token,
                node_id,
                node_id,
                &proof,
                &issued.serial,
                issued.expires_at_unix_ms,
                SCHEMA_DIGEST,
                now + 10 + index as u64,
            )
            .expect("consume enrollment");
        let cert_path = root.path().join(format!("{node_id}.pem"));
        let key_path = root.path().join(format!("{node_id}-key.pem"));
        std::fs::write(
            &cert_path,
            format!("{}{}", issued.certificate_pem, ca.certificate_pem),
        )
        .unwrap();
        std::fs::write(&key_path, &csr.private_key_pem).unwrap();
        secure_private_key(&key_path);
        node_materials.push((
            node_id.to_string(),
            csr,
            issued,
            NodeTlsIdentityFiles {
                client_certificate_chain_pem: cert_path,
                client_private_key_pem: key_path,
                control_plane_ca_pem: ca_path.clone(),
            },
        ));
    }

    let artifact_bytes = b"live-artifact-capability";
    let (artifact, upload) = control
        .initiate_artifact(
            &host,
            lease.control_plane_generation,
            "artifact-live",
            "node-a",
            None,
            "application/json",
            artifact_bytes.len() as u64,
            &sha256_hex(artifact_bytes),
            ArtifactClassification::CompanyInternal,
            BTreeSet::from(["node-b".into()]),
            now + 20,
        )
        .expect("Host initiates bounded artifact");
    control
        .complete_artifact(
            lease.control_plane_generation,
            &upload,
            artifact_bytes,
            now + 21,
        )
        .expect("complete bounded artifact");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind gateway");
    let port = listener.local_addr().unwrap().port();
    let tls = ControlPlaneTlsFiles {
        server_certificate_chain_pem: server_cert_path,
        server_private_key_pem: server_key_path,
        node_ca_pem: ca_path,
    };
    let server_fabric_root = fabric_root.clone();
    let server = std::thread::spawn(move || {
        let mut sessions = Vec::new();
        for _ in 0..2 {
            let (tcp, _) = listener.accept().expect("accept outbound gateway");
            let tls = tls.clone();
            let fabric_root = server_fabric_root.clone();
            sessions.push(std::thread::spawn(move || {
                let (mut socket, peer) =
                    accept_control_plane_mtls(tcp, &tls).expect("mTLS gateway");
                let store = FabricStore::open(fabric_root).expect("reopen shared Store");
                let keys = InMemoryArtifactKeyBackend::default();
                keys.insert(COMPANY, [7; 32]);
                let control = ControlPlane::new(COMPANY, "control-live", &store, &keys, [9; 32]);
                serve_control_plane_session(
                    &mut socket,
                    &peer,
                    &control,
                    lease.control_plane_generation,
                )
                .expect("serve authenticated gateway");
            }));
        }
        for session in sessions {
            session.join().expect("gateway session");
        }
    });

    let connect_node = |material: &(
        String,
        firm_fabric::pki::NodeCsrMaterial,
        firm_fabric::pki::IssuedNodeCertificate,
        NodeTlsIdentityFiles,
    )| {
        let node_id = &material.0;
        NodeGatewayConnection::connect(
            &NodeFabricConfig {
                company_id: company.into(),
                node_id: node_id.clone(),
                control_plane_url: format!(
                    "wss://localhost:{port}{}",
                    firm_fabric::transport::FABRIC_GATEWAY_PATH
                ),
                reconnect_floor_ms: 10,
                reconnect_ceiling_ms: 1_000,
            },
            &material.3,
            NodeHello {
                company_id: company.into(),
                node_id: node_id.clone(),
                instance_id: format!("gateway-{node_id}"),
                node_daemon_id: format!("node-daemon:{node_id}"),
                node_daemon_generation: 1,
                protocol_min: FABRIC_PROTOCOL_VERSION,
                protocol_max: FABRIC_PROTOCOL_VERSION,
                schema_bundle_digest: SCHEMA_DIGEST.into(),
                features: BTreeSet::from(["durable-routing".into()]),
                build_sha: "live-build".into(),
                last_persisted_route_seq: 0,
                unresolved_operation_ids: BTreeSet::new(),
                certificate_serial: material.2.serial.clone(),
                public_key_fingerprint: material.1.public_key_fingerprint.clone(),
            },
        )
        .expect("connect Node gateway")
    };
    let mut node_b = connect_node(&node_materials[1]);
    let mut node_a = connect_node(&node_materials[0]);
    let download = node_b
        .request_artifact_download(&artifact.id)
        .expect("mTLS Node requests a server-built self-bound download capability");
    assert_eq!(download.node_id, "node-b");
    assert_eq!(download.artifact_id, artifact.id);
    assert_eq!(download.purpose, ArtifactCapabilityPurpose::Download);
    let local_a = NodeLocalFabricStore::open(root.path().join("node-a"), company, "node-a")
        .expect("Node A local Store");
    let local_b = NodeLocalFabricStore::open(root.path().join("node-b"), company, "node-b")
        .expect("Node B local Store");
    local_a
        .bind_gateway_session(&node_a.session)
        .expect("bind Node A gateway session");
    local_b
        .bind_gateway_session(&node_b.session)
        .expect("bind Node B gateway session");
    node_b.heartbeat().expect("empty target heartbeat");
    let empty_batch = node_b
        .apply_next(&local_b, &mut ProbeApplication)
        .expect_err("empty pending batch is explicit, not a timeout");
    assert_eq!(empty_batch.code, FabricErrorCode::TargetOffline);
    assert_eq!(empty_batch.message, "pending delivery batch is complete");
    let source_actor = AuthenticatedActor {
        company_id: company.into(),
        actor_id: "node-a".into(),
        actor_kind: ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: "node-a-local-session".into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
    };
    let body = json!({"probe":"hello-node-b"});
    let routed = RoutedOperation {
        id: "operation-live-a-b".into(),
        company_id: company.into(),
        kind: PROBE_OPERATION_KIND.into(),
        source_authority: OperationSourceAuthority::Node,
        source_node_id: Some("node-a".into()),
        target_node_id: "node-b".into(),
        source_gateway_generation: Some(node_a.session.gateway_generation),
        source_node_daemon_id: Some(node_a.session.node_daemon_id.clone()),
        source_node_daemon_generation: Some(node_a.session.node_daemon_generation),
        control_plane_generation: node_a.session.control_plane_generation,
        source_execution_space_id: None,
        target_execution_space_id: None,
        actor: source_actor.clone(),
        actor_runtime_generation: None,
        authorization_context: BTreeMap::new(),
        idempotency_key: "live-a-b-once".into(),
        ordering_key: "probe-node-b".into(),
        correlation_id: "live-correlation".into(),
        causation_id: None,
        expected_target_revision: None,
        body_schema: PROBE_BODY_SCHEMA.into(),
        body_digest: json_digest(&body).expect("body digest"),
        body,
        priority: OperationPriority::Normal,
        created_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        protocol_version: FABRIC_PROTOCOL_VERSION,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
        canonicalization_version: FABRIC_CANONICALIZATION_VERSION.into(),
    };
    let accepted = node_a
        .submit_operation(&local_a, &source_actor, routed.clone())
        .expect("Control Plane accepts source operation");
    assert_eq!(accepted.kind, ReceiptKind::ControlPlaneAccepted);
    node_b.heartbeat().expect("target heartbeat");
    let applied = node_b
        .apply_next(&local_b, &mut ProbeApplication)
        .expect("target durably applies operation");
    assert_eq!(applied.kind, ReceiptKind::OperationApplied);
    assert_eq!(applied.application_effect, Some(EffectCertainty::Applied));

    let mut expired = routed.clone();
    expired.id = "operation-live-expired".into();
    expired.idempotency_key = "live-expired-once".into();
    expired.correlation_id = "live-expired-correlation".into();
    expired.created_at_unix_ms = firm_fabric::gateway_runtime::now_unix_ms().unwrap();
    expired.expires_at_unix_ms = expired.created_at_unix_ms + 1_000;
    node_a
        .submit_operation(&local_a, &source_actor, expired.clone())
        .expect("Control Plane accepts operation before its deadline");
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    node_b
        .heartbeat()
        .expect("same target gateway remains connected");
    let rejected = node_b
        .apply_next(&local_b, &mut ProbeApplication)
        .expect("expired delivery returns typed terminal receipt without closing gateway");
    assert_eq!(rejected.kind, ReceiptKind::OperationRejected);
    assert_eq!(
        rejected.application_effect,
        Some(EffectCertainty::NotApplied)
    );
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code),
        Some(FabricErrorCode::OperationExpired)
    );
    let expired_state = local_b.snapshot().expect("expired target state");
    assert!(expired_state.ordering_tombstones.contains_key(&expired.id));
    assert!(!expired_state.inboxes.contains_key(&expired.id));
    assert!(!expired_state.results.contains_key(&expired.id));

    let mut after_expiry = routed.clone();
    after_expiry.id = "operation-live-after-expiry".into();
    after_expiry.idempotency_key = "live-after-expiry-once".into();
    after_expiry.correlation_id = "live-after-expiry-correlation".into();
    after_expiry.created_at_unix_ms = firm_fabric::gateway_runtime::now_unix_ms().unwrap();
    after_expiry.expires_at_unix_ms = after_expiry.created_at_unix_ms + 60_000;
    node_a
        .submit_operation(&local_a, &source_actor, after_expiry.clone())
        .expect("Control Plane accepts valid sequence after expired tombstone");
    node_b
        .heartbeat()
        .expect("active gateway continues after expired delivery");
    let applied_after_expiry = node_b
        .apply_next(&local_b, &mut ProbeApplication)
        .expect("valid next sequence applies on the same gateway");
    assert_eq!(applied_after_expiry.kind, ReceiptKind::OperationApplied);

    let final_state = store.snapshot().expect("final Control Plane state");
    assert_eq!(final_state.operations.len(), 3);
    assert_eq!(final_state.attempts.len(), 3);
    assert!(final_state.receipts.values().any(|receipt| {
        receipt.operation_id == "operation-live-a-b"
            && receipt.kind == ReceiptKind::OperationApplied
    }));
    assert_eq!(
        local_b
            .snapshot()
            .expect("Node B final state")
            .results
            .len(),
        2
    );
    node_a.close().expect("close Node A");
    node_b.close().expect("close Node B");
    server.join().expect("join gateway server");
}
