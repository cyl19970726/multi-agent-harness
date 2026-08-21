use super::*;

#[test]
fn enrollment_proof_and_certificate_rotation_are_cryptographic_and_generation_fenced() {
    let root = TestRoot::new("certificate-rotation");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let first_hello = hello("node-a", "gateway-a-1", "cert-a", &fingerprint("node-a"));
    let mut unauthorized_feature_hello = first_hello.clone();
    unauthorized_feature_hello
        .features
        .insert("remote-runtime".into());
    let before_feature = store.snapshot().expect("snapshot");
    let feature_error = connect_node(
        &control,
        lease.control_plane_generation,
        &unauthorized_feature_hello,
        &signing_key("node-a"),
        28,
    )
    .expect_err("Node cannot widen enrollment capabilities in NodeHello");
    assert_eq!(feature_error.code, FabricErrorCode::FeatureIncompatible);
    assert_eq!(store.snapshot().expect("snapshot"), before_feature);
    let before_impersonation = store.snapshot().expect("snapshot");
    let insecure_transport = control
        .connect_gateway(
            lease.control_plane_generation,
            &firm_fabric::transport::VerifiedMtlsPeer {
                tls_version: "TLS1.2".into(),
                ..verified_peer(&first_hello)
            },
            &first_hello,
            &hello_proof(
                &first_hello,
                lease.control_plane_generation,
                &signing_key("node-a"),
            ),
            29,
        )
        .expect_err("TLS below 1.3 must fail closed");
    assert_eq!(
        insecure_transport.code,
        FabricErrorCode::ProtocolIncompatible
    );
    assert_eq!(store.snapshot().expect("snapshot"), before_impersonation);
    let impersonation = control
        .connect_gateway(
            lease.control_plane_generation,
            &firm_fabric::transport::VerifiedMtlsPeer {
                node_id: "node-b".into(),
                ..verified_peer(&first_hello)
            },
            &first_hello,
            &hello_proof(
                &first_hello,
                lease.control_plane_generation,
                &signing_key("node-b"),
            ),
            29,
        )
        .expect_err("Node body cannot impersonate another mTLS identity");
    assert_eq!(impersonation.code, FabricErrorCode::UnauthorizedActor);
    assert_eq!(store.snapshot().expect("snapshot"), before_impersonation);
    let welcome = connect_node(
        &control,
        lease.control_plane_generation,
        &first_hello,
        &signing_key("node-a"),
        30,
    )
    .expect("connect with enrolled certificate");
    let next_key = SigningKey::from_bytes(&[4; 32]);
    let challenge = firm_fabric::enrollment::certificate_rotation_challenge(
        COMPANY,
        "node-a",
        "cert-a",
        "cert-a-rotated",
        1,
        SCHEMA_DIGEST,
    );
    let mut proof = EnrollmentProof {
        public_key: next_key.verifying_key().to_bytes().to_vec(),
        signature: signing_key("node-b")
            .sign(challenge.as_bytes())
            .to_bytes()
            .to_vec(),
        challenge: challenge.clone(),
    };
    let before = store.snapshot().expect("snapshot");
    let invalid = control
        .rotate_node_certificate(
            lease.control_plane_generation,
            "node-a",
            welcome.gateway_generation,
            &welcome.node_daemon_id,
            welcome.node_daemon_generation,
            "cert-a",
            "cert-a-rotated",
            1,
            &proof,
            900_000,
            40,
        )
        .expect_err("foreign key cannot prove possession");
    assert_eq!(invalid.code, FabricErrorCode::EnrollmentInvalid);
    assert_eq!(store.snapshot().expect("snapshot"), before);

    proof.signature = next_key.sign(challenge.as_bytes()).to_bytes().to_vec();
    let (node, certificate) = control
        .rotate_node_certificate(
            lease.control_plane_generation,
            "node-a",
            welcome.gateway_generation,
            &welcome.node_daemon_id,
            welcome.node_daemon_generation,
            "cert-a",
            "cert-a-rotated",
            1,
            &proof,
            900_000,
            41,
        )
        .expect("rotate certificate");
    assert_eq!(node.node_revision, 2);
    assert_eq!(
        certificate.public_key_fingerprint,
        sha256_hex(next_key.verifying_key().to_bytes())
    );
    let old_hello = hello(
        "node-a",
        "gateway-old-cert",
        "cert-a",
        &fingerprint("node-a"),
    );
    let old = connect_node(
        &control,
        lease.control_plane_generation,
        &old_hello,
        &signing_key("node-a"),
        42,
    )
    .expect_err("revoked certificate cannot reconnect");
    assert_eq!(old.code, FabricErrorCode::UnauthorizedActor);
    let mut rotated_hello = hello(
        "node-a",
        "gateway-a-2",
        "cert-a-rotated",
        &sha256_hex(next_key.verifying_key().to_bytes()),
    );
    rotated_hello.node_daemon_generation = 2;
    let successor = connect_node(
        &control,
        lease.control_plane_generation,
        &rotated_hello,
        &next_key,
        42,
    )
    .expect("rotated certificate reconnects");
    assert_eq!(successor.gateway_generation, welcome.gateway_generation + 1);
}
