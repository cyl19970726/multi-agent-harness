#![allow(clippy::result_large_err)]

use ed25519_dalek::{Signer, SigningKey};
use firm_fabric::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const COMPANY: &str = "company-test";
const SCHEMA_DIGEST: &str = "schema-bundle-v1";
const TOKEN_A: &str = "enrollment-token-node-a-0000000000000001";
const TOKEN_B: &str = "enrollment-token-node-b-0000000000000002";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "agentfirm-fabric-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).expect("create isolated test root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
        if let Ok(target) = std::fs::canonicalize(&self.0) {
            assert!(target.starts_with(&temp));
            assert!(target
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("agentfirm-fabric-")));
            std::fs::remove_dir_all(target).expect("remove isolated test root");
        }
    }
}

fn actor(id: &str, roles: &[&str]) -> AuthenticatedActor {
    AuthenticatedActor {
        company_id: COMPANY.into(),
        actor_id: id.into(),
        actor_kind: ActorKind::Human,
        role_bindings: roles.iter().map(|role| (*role).to_string()).collect(),
        session_id: format!("session-{id}"),
        issued_at_unix_ms: 1,
        expires_at_unix_ms: 1_000_000,
    }
}

fn hello(node: &str, instance: &str, cert: &str, fingerprint: &str) -> NodeHello {
    NodeHello {
        company_id: COMPANY.into(),
        node_id: node.into(),
        instance_id: instance.into(),
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: SCHEMA_DIGEST.into(),
        features: BTreeSet::from(["durable-routing".into()]),
        build_sha: "build-test".into(),
        last_persisted_route_seq: 0,
        unresolved_operation_ids: BTreeSet::new(),
        certificate_serial: cert.into(),
        public_key_fingerprint: fingerprint.into(),
    }
}

fn signing_key(node: &str) -> SigningKey {
    SigningKey::from_bytes(&match node {
        "node-a" => [1; 32],
        "node-b" => [2; 32],
        _ => [3; 32],
    })
}

fn fingerprint(node: &str) -> String {
    sha256_hex(signing_key(node).verifying_key().to_bytes())
}

fn enrollment_proof(enrollment_id: &str, node: &str, cert: &str) -> EnrollmentProof {
    let challenge = firm_fabric::enrollment::enrollment_challenge(
        COMPANY,
        enrollment_id,
        node,
        cert,
        SCHEMA_DIGEST,
    );
    let key = signing_key(node);
    EnrollmentProof {
        public_key: key.verifying_key().to_bytes().to_vec(),
        signature: key.sign(challenge.as_bytes()).to_bytes().to_vec(),
        challenge,
    }
}

fn hello_proof(
    hello: &NodeHello,
    control_plane_generation: u64,
    key: &SigningKey,
) -> NodeHelloProof {
    let challenge =
        firm_fabric::node_gateway::node_hello_challenge(COMPANY, control_plane_generation, hello)
            .expect("hello challenge");
    NodeHelloProof {
        public_key: key.verifying_key().to_bytes().to_vec(),
        signature: key.sign(challenge.as_bytes()).to_bytes().to_vec(),
        challenge,
    }
}

fn connect_node<K: ArtifactKeyBackend>(
    control: &ControlPlane<'_, K>,
    generation: u64,
    hello: &NodeHello,
    key: &SigningKey,
    now_unix_ms: u64,
) -> Result<NodeWelcome, FabricError> {
    control.connect_gateway(
        generation,
        hello,
        &hello_proof(hello, generation, key),
        now_unix_ms,
    )
}

fn enroll_nodes<K: ArtifactKeyBackend>(control: &ControlPlane<'_, K>, generation: u64) {
    let host = actor("host", &["company_host"]);
    for (enrollment, token, node, cert) in [
        ("enroll-a", TOKEN_A, "node-a", "cert-a"),
        ("enroll-b", TOKEN_B, "node-b", "cert-b"),
    ] {
        control
            .create_enrollment(
                &host,
                generation,
                enrollment,
                token,
                node,
                BTreeSet::from(["durable-routing".into(), "artifact-transfer".into()]),
                500_000,
                10,
            )
            .expect("create enrollment");
        control
            .consume_enrollment(
                generation,
                token,
                node,
                node,
                &enrollment_proof(enrollment, node, cert),
                cert,
                900_000,
                SCHEMA_DIGEST,
                20,
            )
            .expect("consume enrollment");
    }
}

fn operation(source_generation: u64, control_generation: u64) -> RoutedOperation {
    let body = json!({"probe": "reachable"});
    RoutedOperation {
        id: "operation-1".into(),
        company_id: COMPANY.into(),
        kind: "fabric.probe.v1".into(),
        source_node_id: "node-a".into(),
        target_node_id: "node-b".into(),
        source_gateway_generation: source_generation,
        control_plane_generation: control_generation,
        source_execution_space_id: Some("space-a".into()),
        target_execution_space_id: Some("space-b".into()),
        actor: actor("fabric-client", &["fabric_submit"]),
        actor_runtime_generation: Some(1),
        authorization_context: BTreeMap::from([("scope".into(), "probe".into())]),
        idempotency_key: "idempotency-1".into(),
        ordering_key: "probe:node-a:node-b".into(),
        correlation_id: "correlation-1".into(),
        causation_id: None,
        expected_target_revision: None,
        body_schema: "agentfirm.remote_fabric.probe.v1".into(),
        body_digest: json_digest(&body).expect("digest body"),
        body,
        priority: OperationPriority::Normal,
        created_at_unix_ms: 100,
        expires_at_unix_ms: 50_000,
        protocol_version: FABRIC_PROTOCOL_VERSION,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    }
}

#[test]
fn one_use_enrollment_and_stale_control_plane_have_zero_side_effects() {
    let root = TestRoot::new("enrollment");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let old = ControlPlane::new(COMPANY, "control-old", &store, &keys, [9; 32]);
    let lease = old.acquire_lease("cp-lease-1", 0, 1).expect("lease");
    let host = actor("host", &["company_host"]);
    old.create_enrollment(
        &host,
        lease.control_plane_generation,
        "enrollment-1",
        TOKEN_A,
        "node-a",
        BTreeSet::new(),
        1000,
        2,
    )
    .expect("create enrollment");
    old.consume_enrollment(
        lease.control_plane_generation,
        TOKEN_A,
        "node-a",
        "Node A",
        &enrollment_proof("enrollment-1", "node-a", "cert-a"),
        "cert-a",
        10_000,
        SCHEMA_DIGEST,
        3,
    )
    .expect("consume once");
    let before = store.snapshot().expect("snapshot");
    let replay = old
        .consume_enrollment(
            lease.control_plane_generation,
            TOKEN_A,
            "node-a-replay",
            "Node Replay",
            &enrollment_proof("enrollment-1", "node-a-replay", "cert-replay"),
            "cert-replay",
            10_000,
            SCHEMA_DIGEST,
            4,
        )
        .expect_err("one-use enrollment must reject replay");
    assert_eq!(replay.code, FabricErrorCode::EnrollmentConsumed);
    assert_eq!(store.snapshot().expect("snapshot"), before);

    let successor = ControlPlane::new(COMPANY, "control-new", &store, &keys, [9; 32]);
    let next = successor
        .acquire_lease("cp-lease-2", lease.revision, 31_001)
        .expect("successor after expiry");
    let before_stale = store.snapshot().expect("snapshot");
    let stale = old
        .create_enrollment(
            &host,
            next.control_plane_generation,
            "stale-enrollment",
            "stale-enrollment-token-000000000000000000",
            "stale",
            BTreeSet::new(),
            31_100,
            31_010,
        )
        .expect_err("stale instance cannot borrow successor generation");
    assert_eq!(stale.code, FabricErrorCode::ControlPlaneStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
}

#[test]
fn durable_route_replays_exactly_and_fences_stale_source_generation() {
    let root = TestRoot::new("route");
    let store = FabricStore::open(root.path()).expect("open store");
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
    let mut unsupported = request.clone();
    unsupported.id = "operation-unsupported-capability".into();
    unsupported.idempotency_key = "idempotency-unsupported-capability".into();
    unsupported.kind = "runtime_command.reference.v1".into();
    let before_unsupported = store.snapshot().expect("snapshot");
    let unavailable = control
        .accept_operation(lease.control_plane_generation, unsupported, 99)
        .expect_err("operation capability must be authorized on both Nodes");
    assert_eq!(unavailable.code, FabricErrorCode::FeatureIncompatible);
    assert_eq!(store.snapshot().expect("snapshot"), before_unsupported);
    let (_, attempt, accepted, replayed) = control
        .accept_operation(lease.control_plane_generation, request.clone(), 100)
        .expect("accept operation");
    assert!(!replayed);
    assert_eq!(accepted.kind, ReceiptKind::ControlPlaneAccepted);
    let (_, _, replay_receipt, replayed) = control
        .accept_operation(lease.control_plane_generation, request.clone(), 101)
        .expect("exact replay");
    assert!(replayed);
    assert_eq!(accepted, replay_receipt);
    let before_conflict = store.snapshot().expect("snapshot");
    let mut changed = request.clone();
    changed.id = "operation-changed-under-same-key".into();
    changed.body = json!({"probe": "different"});
    changed.body_digest = json_digest(&changed.body).expect("digest changed body");
    let conflict = control
        .accept_operation(lease.control_plane_generation, changed, 101)
        .expect_err("same key with another fingerprint must fail closed");
    assert_eq!(conflict.code, FabricErrorCode::IdempotencyConflict);
    assert_eq!(store.snapshot().expect("snapshot"), before_conflict);

    let before_out_of_order = store.snapshot().expect("snapshot");
    let out_of_order = control
        .record_application_result(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            true,
            101,
        )
        .expect_err("application cannot precede durable target inbox");
    assert_eq!(out_of_order.code, FabricErrorCode::OperationUnknown);
    assert_eq!(store.snapshot().expect("snapshot"), before_out_of_order);
    let (inbox, persisted, replayed) = control
        .persist_target_inbox(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &request.id,
            &request_digest,
            attempt.route_seq,
            102,
        )
        .expect("persist inbox");
    assert!(!replayed);
    assert_eq!(persisted.kind, ReceiptKind::TargetPersisted);
    let (terminal_inbox, terminal, replayed) = control
        .record_application_result(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            true,
            103,
        )
        .expect("record result");
    assert!(!replayed);
    assert_eq!(terminal.kind, ReceiptKind::OperationApplied);
    assert_eq!(terminal_inbox.state, LocalInboxState::Applied);
    let state = store.snapshot().expect("snapshot");
    assert_eq!(state.operations.len(), 1);
    assert_eq!(state.inboxes.len(), 1);
    assert_eq!(
        state.outboxes[&request.id].local_state,
        LocalOutboxState::Terminal
    );
    assert_eq!(
        state.outboxes[&request.id].terminal_receipt_ref,
        Some(terminal.id)
    );
    assert_eq!(inbox.request_digest, request_digest);

    control
        .heartbeat_lease(lease.control_plane_generation, lease.revision, 29_000)
        .expect("keep Control Plane alive");
    let successor_hello = hello("node-a", "gateway-a-2", "cert-a", &fingerprint("node-a"));
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
        .accept_operation(lease.control_plane_generation, stale, 30_032)
        .expect_err("stale source generation must fail");
    assert_eq!(error.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}

#[test]
fn commit_failure_and_torn_final_frame_recover_without_partial_state() {
    let root = TestRoot::new("recovery");
    let journal;
    let durable;
    {
        let store = FabricStore::open(root.path()).expect("open store");
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(COMPANY, [7; 32]);
        let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
        let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
        durable = store.snapshot().expect("durable snapshot");
        store.fail_next_commit_for_test();
        let error = control
            .create_enrollment(
                &actor("host", &["company_host"]),
                lease.control_plane_generation,
                "must-not-commit",
                TOKEN_A,
                "node-a",
                BTreeSet::new(),
                1000,
                2,
            )
            .expect_err("forced commit failure");
        assert_eq!(error.code, FabricErrorCode::StoreUnavailable);
        assert_eq!(store.snapshot().expect("snapshot"), durable);
        journal = store.journal_path().to_path_buf();
    }
    OpenOptions::new()
        .append(true)
        .open(&journal)
        .expect("open journal")
        .write_all(b"{\"transaction_sequence\":")
        .expect("append torn frame");
    let reopened = FabricStore::open(root.path()).expect("ignore torn final frame");
    assert_eq!(reopened.snapshot().expect("snapshot"), durable);

    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &reopened, &keys, [9; 32]);
    let lease = reopened.snapshot().expect("snapshot").control_plane_leases[COMPANY].clone();
    reopened.fail_after_append_for_test();
    let unknown = control
        .create_enrollment(
            &actor("host", &["company_host"]),
            lease.control_plane_generation,
            "commit-ack-lost",
            TOKEN_B,
            "node-b",
            BTreeSet::new(),
            1000,
            3,
        )
        .expect_err("lost commit acknowledgement is unknown, never effect-none");
    assert_eq!(unknown.code, FabricErrorCode::RecoveryRequired);
    assert_eq!(unknown.effect, EffectCertainty::Unknown);
    assert_eq!(
        reopened
            .snapshot()
            .expect_err("store latches unavailable")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    drop(control);
    drop(reopened);
    let recovered = FabricStore::open(root.path()).expect("reopen complete committed frame");
    assert!(recovered
        .snapshot()
        .expect("snapshot")
        .enrollments
        .contains_key("commit-ack-lost"));
}

#[test]
fn artifact_digest_scope_encryption_and_one_use_capability_fail_closed() {
    let root = TestRoot::new("artifact");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let bytes = b"bounded deterministic artifact";
    let writer = actor("writer", &["artifact_write"]);
    let (manifest, upload) = control
        .initiate_artifact(
            &writer,
            lease.control_plane_generation,
            "artifact-1",
            "node-a",
            None,
            "text/plain",
            bytes.len() as u64,
            &sha256_hex(bytes),
            ArtifactClassification::CompanyInternal,
            BTreeSet::from(["reader".into()]),
            100,
        )
        .expect("initiate artifact");
    let completed = control
        .complete_artifact(lease.control_plane_generation, &upload, bytes, 101)
        .expect("complete artifact");
    assert_eq!(completed.id, manifest.id);
    let replay = control
        .complete_artifact(lease.control_plane_generation, &upload, bytes, 102)
        .expect_err("upload capability is one-use");
    assert_eq!(replay.code, FabricErrorCode::CapabilityConsumed);
    let reader = actor("reader", &["artifact_read"]);
    let download = control
        .issue_download_capability(
            &reader,
            lease.control_plane_generation,
            &manifest.id,
            "node-b",
            103,
        )
        .expect("issue download capability");
    assert_eq!(
        control
            .download_artifact(lease.control_plane_generation, &download, 104)
            .expect("decrypt artifact"),
        bytes
    );
    let consumed = control
        .download_artifact(lease.control_plane_generation, &download, 105)
        .expect_err("download capability is one-use");
    assert_eq!(consumed.code, FabricErrorCode::CapabilityConsumed);
    let journal_bytes = std::fs::read(store.journal_path()).expect("read journal");
    assert!(!journal_bytes
        .windows(bytes.len())
        .any(|window| window == bytes));

    let secret = b"-----BEGIN PRIVATE KEY-----\nnot-real";
    let (_, secret_upload) = control
        .initiate_artifact(
            &writer,
            lease.control_plane_generation,
            "artifact-secret",
            "node-a",
            None,
            "text/plain",
            secret.len() as u64,
            &sha256_hex(secret),
            ArtifactClassification::Sensitive,
            BTreeSet::from(["reader".into()]),
            106,
        )
        .expect("manifest can precede content inspection");
    let before = store.snapshot().expect("snapshot");
    let rejected = control
        .complete_artifact(lease.control_plane_generation, &secret_upload, secret, 107)
        .expect_err("secret-like payload must fail closed");
    assert_eq!(rejected.code, FabricErrorCode::ArtifactTampered);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}

#[test]
fn checked_in_valid_schema_fixtures_match_rust_wire_types() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/remote-fabric/fixtures/valid");
    let node: CompanyNode = serde_json::from_slice(
        &std::fs::read(root.join("company-node.json")).expect("read CompanyNode fixture"),
    )
    .expect("CompanyNode fixture matches Rust");
    assert_eq!(node.schema_version, FABRIC_SCHEMA_VERSION);
    let enrollment: NodeEnrollment = serde_json::from_slice(
        &std::fs::read(root.join("node-enrollment.json")).expect("read enrollment fixture"),
    )
    .expect("NodeEnrollment fixture matches Rust");
    assert_eq!(enrollment.status, EnrollmentStatus::Pending);
    let gateway: NodeGatewayLease = serde_json::from_slice(
        &std::fs::read(root.join("node-gateway-lease.json")).expect("read gateway fixture"),
    )
    .expect("NodeGatewayLease fixture matches Rust");
    assert_eq!(gateway.gateway_generation, 1);
    let operation: RoutedOperation = serde_json::from_slice(
        &std::fs::read(root.join("routed-operation.json")).expect("read operation fixture"),
    )
    .expect("RoutedOperation fixture matches Rust");
    assert_eq!(operation.protocol_version, FABRIC_PROTOCOL_VERSION);
    operation
        .validate_digest()
        .expect("fixture body digest matches");
    let receipt: RouteReceipt = serde_json::from_slice(
        &std::fs::read(root.join("route-receipt.json")).expect("read receipt fixture"),
    )
    .expect("RouteReceipt fixture matches Rust");
    assert_eq!(receipt.control_plane_generation, 2);
    let artifact: RemoteArtifactManifest = serde_json::from_slice(
        &std::fs::read(root.join("artifact-manifest.json")).expect("read artifact fixture"),
    )
    .expect("artifact fixture matches Rust");
    assert_eq!(artifact.size_bytes, 128);
}

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
    let impersonation = control
        .connect_gateway(
            lease.control_plane_generation,
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
    let rotated_hello = hello(
        "node-a",
        "gateway-a-2",
        "cert-a-rotated",
        &sha256_hex(next_key.verifying_key().to_bytes()),
    );
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

#[test]
fn concurrent_one_use_enrollment_has_exactly_one_winner() {
    let root = TestRoot::new("concurrent-enrollment");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    control
        .create_enrollment(
            &actor("host", &["company_host"]),
            lease.control_plane_generation,
            "enroll-a",
            TOKEN_A,
            "node-a",
            BTreeSet::new(),
            1000,
            2,
        )
        .expect("create enrollment");
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let client = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
            client.consume_enrollment(
                lease.control_plane_generation,
                TOKEN_A,
                "node-a",
                "Node A",
                &enrollment_proof("enroll-a", "node-a", "cert-a"),
                "cert-a",
                10_000,
                SCHEMA_DIGEST,
                3,
            )
        });
        let second = scope.spawn(|| {
            let client = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
            client.consume_enrollment(
                lease.control_plane_generation,
                TOKEN_A,
                "node-a-duplicate",
                "Node A duplicate",
                &enrollment_proof("enroll-a", "node-a-duplicate", "cert-a-duplicate"),
                "cert-a-duplicate",
                10_000,
                SCHEMA_DIGEST,
                3,
            )
        });
        [
            first.join().expect("first thread"),
            second.join().expect("second thread"),
        ]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .filter(|error| error.code == FabricErrorCode::EnrollmentConsumed)
            .count(),
        1
    );
    let state = store.snapshot().expect("snapshot");
    assert_eq!(state.nodes.len(), 1);
    assert_eq!(state.certificates.len(), 1);
}

#[test]
fn retry_requires_a_new_target_generation_and_reconcile_never_blind_replays() {
    let root = TestRoot::new("retry-reconcile");
    let store = FabricStore::open(root.path()).expect("open store");
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
    let (_, first_attempt, _, _) = control
        .accept_operation(lease.control_plane_generation, request.clone(), 100)
        .expect("accept operation");
    let same_generation = control
        .retry_operation(lease.control_plane_generation, &request.id, 101)
        .expect("same target generation is exact replay");
    assert!(same_generation.2);
    assert_eq!(same_generation.0, first_attempt);

    let cp_second = control
        .heartbeat_lease(lease.control_plane_generation, lease.revision, 29_000)
        .expect("keep Control Plane alive");
    let successor_hello = hello("node-b", "gateway-b-2", "cert-b", &fingerprint("node-b"));
    let successor = connect_node(
        &control,
        lease.control_plane_generation,
        &successor_hello,
        &signing_key("node-b"),
        30_031,
    )
    .expect("target successor");
    let (second_attempt, _, replayed) = control
        .retry_operation(lease.control_plane_generation, &request.id, 30_032)
        .expect("retry effect-none operation on successor");
    assert!(!replayed);
    assert_eq!(second_attempt.attempt_no, 2);
    assert_eq!(
        second_attempt.target_gateway_generation,
        successor.gateway_generation
    );
    let before_stale = store.snapshot().expect("snapshot");
    let stale = control
        .persist_target_inbox(
            lease.control_plane_generation,
            "node-b",
            target.gateway_generation,
            &request.id,
            &request_digest,
            first_attempt.route_seq,
            30_033,
        )
        .expect_err("expired predecessor cannot persist after successor takeover");
    assert_eq!(stale.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
    control
        .persist_target_inbox(
            lease.control_plane_generation,
            "node-b",
            successor.gateway_generation,
            &request.id,
            &request_digest,
            second_attempt.route_seq,
            30_034,
        )
        .expect("successor persists inbox");
    let (_, terminal, _) = control
        .record_application_result(
            lease.control_plane_generation,
            "node-b",
            successor.gateway_generation,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            true,
            30_035,
        )
        .expect("successor records terminal result");

    let cp_third = control
        .heartbeat_lease(lease.control_plane_generation, cp_second.revision, 58_000)
        .expect("keep Control Plane alive for reconciliation");
    assert_eq!(
        cp_third.control_plane_generation,
        lease.control_plane_generation
    );
    let third_hello = hello("node-b", "gateway-b-3", "cert-b", &fingerprint("node-b"));
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
            &BTreeSet::from([request.id]),
            60_033,
        )
        .expect("successor reconciles durable prior-generation terminal receipt");
    assert_eq!(reconciled, vec![terminal]);
}
