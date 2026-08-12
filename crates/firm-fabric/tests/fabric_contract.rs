#![allow(clippy::result_large_err)]

use ed25519_dalek::{Signer, SigningKey};
use firm_fabric::transport::{
    decode_frame, encode_frame, FabricSessionFence, NodeFabricConfig, MAX_FABRIC_FRAME_BYTES,
};
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

#[cfg(unix)]
fn secure_private_key(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("restrict private key fixture");
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
        node_daemon_id: format!("node-daemon:{node}"),
        node_daemon_generation: 1,
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

fn verified_peer(hello: &NodeHello) -> firm_fabric::transport::VerifiedMtlsPeer {
    firm_fabric::transport::VerifiedMtlsPeer {
        company_id: hello.company_id.clone(),
        node_id: hello.node_id.clone(),
        certificate_serial: hello.certificate_serial.clone(),
        public_key_fingerprint: hello.public_key_fingerprint.clone(),
        tls_version: "TLS1.3".into(),
        websocket_subprotocol: firm_fabric::transport::FABRIC_WEBSOCKET_SUBPROTOCOL.into(),
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
        &verified_peer(hello),
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
        source_authority: OperationSourceAuthority::Node,
        source_node_id: Some("node-a".into()),
        target_node_id: "node-b".into(),
        source_gateway_generation: Some(source_generation),
        source_node_daemon_id: Some("node-daemon:node-a".into()),
        source_node_daemon_generation: Some(1),
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
        canonicalization_version: FABRIC_CANONICALIZATION_VERSION.into(),
    }
}

fn fabric_session(
    node_id: &str,
    gateway_generation: u64,
    control_plane_generation: u64,
) -> FabricSessionFence {
    FabricSessionFence {
        company_id: COMPANY.into(),
        node_id: node_id.into(),
        gateway_generation,
        node_daemon_id: format!("node-daemon:{node_id}"),
        node_daemon_generation: 1,
        control_plane_generation,
    }
}

#[allow(clippy::too_many_arguments)]
fn rotate_gateway_certificate(
    control: &ControlPlane<'_, InMemoryArtifactKeyBackend>,
    store: &FabricStore,
    control_generation: u64,
    welcome: &NodeWelcome,
    node_id: &str,
    current_serial: &str,
    next_serial: &str,
    now_unix_ms: u64,
) -> NodeHello {
    let node = store.snapshot().expect("snapshot").nodes[node_id].clone();
    let challenge = firm_fabric::enrollment::certificate_rotation_challenge(
        COMPANY,
        node_id,
        current_serial,
        next_serial,
        node.node_revision,
        SCHEMA_DIGEST,
    );
    let key = signing_key(node_id);
    let proof = EnrollmentProof {
        public_key: key.verifying_key().to_bytes().to_vec(),
        signature: key.sign(challenge.as_bytes()).to_bytes().to_vec(),
        challenge,
    };
    let (_, certificate) = control
        .rotate_node_certificate(
            control_generation,
            node_id,
            welcome.gateway_generation,
            &welcome.node_daemon_id,
            welcome.node_daemon_generation,
            current_serial,
            next_serial,
            node.node_revision,
            &proof,
            now_unix_ms + 600_000,
            now_unix_ms,
        )
        .expect("rotate gateway certificate under current NodeDaemon authority");
    let mut hello = hello(
        node_id,
        &format!("gateway-{node_id}-successor"),
        next_serial,
        &certificate.public_key_fingerprint,
    );
    hello.node_daemon_id = certificate.node_daemon_id;
    hello.node_daemon_generation = certificate.node_daemon_generation;
    hello
}

#[test]
fn operation_registry_requires_closed_kind_schema_and_body_scope() {
    let mut request = operation(3, 2);
    assert!(matches!(
        request.closed_body().expect("closed probe"),
        ClosedOperationBody::Probe(_)
    ));

    request.kind = RUNTIME_COMMAND_REFERENCE_KIND.into();
    request.body_schema = RUNTIME_COMMAND_REFERENCE_SCHEMA.into();
    request.body = json!({
        "runtime_command_id": "runtime-command:remote-1",
        "intent_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "target_execution_space_id": "space-b",
        "canonical_command_intent": {
            "id": "runtime-command:remote-1",
            "target_execution_space_id": "space-b",
            "command": "resume_session",
            "idempotency_key": "remote-runtime-1",
            "expected_version": 4,
            "expires_unix_ms": 90000,
            "payload": {"session_id": "session-b", "session_generation": 3},
            "payload_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "issued_at": "unix-ms:100"
        }
    });
    request.body_digest = json_digest(&request.body).expect("body digest");
    assert!(matches!(
        request.closed_body().expect("closed runtime reference"),
        ClosedOperationBody::RuntimeCommand(_)
    ));

    let mut browser_authority = request.clone();
    browser_authority.body["actor_id"] = json!("browser-selected-host");
    browser_authority.body_digest =
        json_digest(&browser_authority.body).expect("hostile body digest");
    assert_eq!(
        browser_authority
            .closed_body()
            .expect_err("unknown authority field must fail closed")
            .code,
        FabricErrorCode::InvalidPayload
    );

    let mut wrong_scope = request.clone();
    wrong_scope.body["target_execution_space_id"] = json!("space-c");
    wrong_scope.body_digest = json_digest(&wrong_scope.body).expect("wrong-scope digest");
    assert_eq!(
        wrong_scope
            .closed_body()
            .expect_err("body scope cannot override route scope")
            .code,
        FabricErrorCode::InvalidPayload
    );

    let mut mismatched_schema = request;
    mismatched_schema.body_schema = MESSAGE_REFERENCE_SCHEMA.into();
    assert_eq!(
        mismatched_schema
            .closed_body()
            .expect_err("kind and schema are one frozen pair")
            .code,
        FabricErrorCode::SchemaIncompatible
    );
}

#[test]
fn canonical_json_v1_is_key_order_independent_and_rejects_floats() {
    let left = json!({"z": [3, {"b": 2, "a": 1}], "a": "value"});
    let right = json!({"a": "value", "z": [3, {"a": 1, "b": 2}]});
    assert_eq!(
        canonical_json_bytes(&left).expect("canonical left"),
        br#"{"a":"value","z":[3,{"a":1,"b":2}]}"#
    );
    assert_eq!(json_digest(&left).unwrap(), json_digest(&right).unwrap());
    assert_eq!(
        json_digest(&json!({"unsafe": 1.5}))
            .expect_err("wire canonicalization must reject floats")
            .code,
        FabricErrorCode::InvalidPayload
    );
}

#[test]
fn message_route_requires_verified_immutable_payload_not_identity_only() {
    let message_body_digest = format!("sha256:{}", sha256_hex("hello"));
    let mut request = operation(1, 1);
    request.kind = MESSAGE_REFERENCE_KIND.into();
    request.body_schema = MESSAGE_REFERENCE_SCHEMA.into();
    request.body = json!({
        "message_id": "message:remote-1",
        "body_digest": message_body_digest,
        "canonical_message_envelope": {
            "id": "message:remote-1",
            "body": "hello",
            "body_digest": message_body_digest,
        },
        "message_object_ref": null,
    });
    request.body_digest = json_digest(&request.body).expect("route body digest");
    assert!(matches!(
        request.closed_body().expect("immutable embedded message"),
        ClosedOperationBody::Message(_)
    ));

    let mut identity_only = request.clone();
    identity_only.body["canonical_message_envelope"] = serde_json::Value::Null;
    identity_only.body_digest = json_digest(&identity_only.body).unwrap();
    assert_eq!(
        identity_only
            .closed_body()
            .expect_err("message identity and digest alone are not a payload")
            .code,
        FabricErrorCode::InvalidPayload
    );

    let mut tampered = request;
    tampered.body["canonical_message_envelope"]["body"] = json!("tampered");
    tampered.body_digest = json_digest(&tampered.body).unwrap();
    assert_eq!(
        tampered
            .closed_body()
            .expect_err("target must verify the immutable message body digest")
            .code,
        FabricErrorCode::InvalidPayload
    );
}

#[test]
fn control_plane_source_is_closed_and_node_daemon_parent_fences_successors() {
    let root = TestRoot::new("source-authority-and-daemon-parent");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let target_hello = hello("node-b", "gateway-b", "cert-b", &fingerprint("node-b"));
    connect_node(
        &control,
        lease.control_plane_generation,
        &target_hello,
        &signing_key("node-b"),
        30,
    )
    .expect("connect target");

    let mut control_operation = operation(1, lease.control_plane_generation);
    control_operation.id = "operation-control-plane".into();
    control_operation.idempotency_key = "idempotency-control-plane".into();
    control_operation.source_authority = OperationSourceAuthority::ControlPlane;
    control_operation.source_node_id = None;
    control_operation.source_gateway_generation = None;
    control_operation.source_node_daemon_id = None;
    control_operation.source_node_daemon_generation = None;
    control_operation.source_execution_space_id = None;
    let (_, _, _, replayed) = control
        .accept_control_plane_operation(
            lease.control_plane_generation,
            &actor("control-service", &["fabric_submit"]),
            control_operation.clone(),
            100,
        )
        .expect("Control Plane source routes without a fabricated Node");
    assert!(!replayed);

    let before_forged = store.snapshot().expect("snapshot");
    let mut forged = control_operation;
    forged.id = "operation-forged-control-source".into();
    forged.idempotency_key = "idempotency-forged-control-source".into();
    forged.source_node_daemon_id = Some("node-daemon:forged".into());
    assert_eq!(
        control
            .accept_control_plane_operation(
                lease.control_plane_generation,
                &actor("control-service", &["fabric_submit"]),
                forged,
                101,
            )
            .expect_err("Control Plane source cannot claim Node authority")
            .code,
        FabricErrorCode::SourceMismatch
    );
    assert_eq!(store.snapshot().expect("snapshot"), before_forged);

    let source_hello = hello("node-a", "gateway-a", "cert-a", &fingerprint("node-a"));
    let source = connect_node(
        &control,
        lease.control_plane_generation,
        &source_hello,
        &signing_key("node-a"),
        102,
    )
    .expect("connect source");
    let mut stale_daemon = operation(source.gateway_generation, lease.control_plane_generation);
    stale_daemon.id = "operation-stale-daemon".into();
    stale_daemon.idempotency_key = "idempotency-stale-daemon".into();
    stale_daemon.source_node_daemon_generation = Some(source.node_daemon_generation + 1);
    let before_stale = store.snapshot().expect("snapshot");
    assert_eq!(
        control
            .accept_operation(
                lease.control_plane_generation,
                &fabric_session(
                    "node-a",
                    source.gateway_generation,
                    lease.control_plane_generation,
                ),
                &actor("fabric-client", &["fabric_submit"]),
                stale_daemon,
                103,
            )
            .expect_err("gateway lease is a child of exact NodeDaemon generation")
            .code,
        FabricErrorCode::NodeStaleGeneration
    );
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
}

fn accept_fabric_operation<K: ArtifactKeyBackend>(
    control: &ControlPlane<'_, K>,
    control_plane_generation: u64,
    source_gateway_generation: u64,
    operation: RoutedOperation,
    now_unix_ms: u64,
) -> Result<(RoutedOperation, RouteAttempt, RouteReceipt, bool), FabricError> {
    control.accept_operation(
        control_plane_generation,
        &fabric_session(
            "node-a",
            source_gateway_generation,
            control_plane_generation,
        ),
        &actor("fabric-client", &["fabric_submit"]),
        operation,
        now_unix_ms,
    )
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
    for invalid_expiry in [
        3,
        3 + firm_fabric::enrollment::NODE_CERTIFICATE_LIFETIME_MAX_MS + 1,
    ] {
        let before_invalid = store.snapshot().expect("snapshot");
        let invalid = old
            .consume_enrollment(
                lease.control_plane_generation,
                TOKEN_A,
                "node-a",
                "Node A",
                &enrollment_proof("enrollment-1", "node-a", "cert-a"),
                "cert-a",
                invalid_expiry,
                SCHEMA_DIGEST,
                3,
            )
            .expect_err("certificate lifetime must be bounded");
        assert_eq!(invalid.code, FabricErrorCode::EnrollmentInvalid);
        assert_eq!(store.snapshot().expect("snapshot"), before_invalid);
    }
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
fn control_plane_store_is_durably_bound_to_one_company() {
    let root = TestRoot::new("control-company-binding");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let canonical = ControlPlane::new(COMPANY, "control-a", &store, &keys, [9; 32]);
    canonical
        .acquire_lease("lease-a", 0, 1)
        .expect("bind canonical Company");
    let before = store.snapshot().expect("bound snapshot");
    let foreign = ControlPlane::new("company-foreign", "control-b", &store, &keys, [8; 32]);
    let error = foreign
        .acquire_lease("lease-foreign", 0, 2)
        .expect_err("same physical FabricStore cannot serve another Company");
    assert_eq!(error.code, FabricErrorCode::WrongCompany);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().expect("unchanged snapshot"), before);
    drop(store);
    let reopened = FabricStore::open(root.path()).expect("reopen store");
    assert_eq!(
        reopened
            .snapshot()
            .expect("reopened snapshot")
            .authority_company_id
            .as_deref(),
        Some(COMPANY)
    );
}

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
fn durable_checkpoint_recovers_stale_or_torn_cache_and_never_hides_journal_tamper() {
    let root = TestRoot::new("durable-checkpoint");
    let checkpoint = root.path().join("fabric-checkpoint.json");
    let journal;
    let expected;
    let stale_checkpoint;
    {
        let store = FabricStore::open(root.path()).expect("open store");
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(COMPANY, [7; 32]);
        let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
        let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
        stale_checkpoint = std::fs::read(&checkpoint).expect("first durable checkpoint");
        control
            .heartbeat_lease(lease.control_plane_generation, lease.revision, 2)
            .expect("second durable transaction");
        expected = store.snapshot().expect("current snapshot");
        journal = store.journal_path().to_path_buf();
    }

    std::fs::write(&checkpoint, &stale_checkpoint).expect("restore stale checkpoint");
    let reopened = FabricStore::open(root.path()).expect("replay suffix after stale checkpoint");
    let checkpoint_modified = std::fs::metadata(&checkpoint)
        .expect("checkpoint metadata")
        .modified()
        .expect("checkpoint modified time");
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert_eq!(reopened.snapshot().expect("snapshot"), expected);
    assert_eq!(
        std::fs::metadata(&checkpoint)
            .expect("checkpoint metadata after read")
            .modified()
            .expect("checkpoint modified time after read"),
        checkpoint_modified,
        "a current checkpoint is read-only on snapshot"
    );
    drop(reopened);

    std::fs::write(&checkpoint, b"{torn-checkpoint").expect("write torn checkpoint cache");
    let reopened = FabricStore::open(root.path()).expect("fall back to full journal validation");
    assert_eq!(reopened.snapshot().expect("snapshot"), expected);
    drop(reopened);

    let mut journal_bytes = std::fs::read(&journal).expect("read journal");
    let changed = journal_bytes
        .iter_mut()
        .find(|byte| **byte == b'c')
        .expect("journal has a byte to tamper");
    *changed = b'd';
    std::fs::write(&journal, journal_bytes).expect("tamper validated checkpoint prefix");
    assert_eq!(
        FabricStore::open(root.path())
            .err()
            .expect("checkpoint cannot hide journal tamper")
            .code,
        FabricErrorCode::StoreUnavailable
    );
}

#[test]
fn control_plane_backup_restores_exact_transaction_and_rejects_tamper_or_overwrite() {
    let root = TestRoot::new("control-backup-restore");
    let source_root = root.path().join("source");
    let store = FabricStore::open(&source_root).expect("source Store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-backup", &store, &keys, [9; 32]);
    control
        .acquire_lease("backup-lease", 0, 100)
        .expect("populate source Store");
    let expected = store.snapshot().unwrap();
    let backup_root = root.path().join("backup");
    let manifest = store.create_backup(&backup_root).expect("atomic backup");
    assert_eq!(manifest.transaction_sequence, expected.revision);
    assert_eq!(manifest.state_digest, json_digest(&expected).unwrap());

    let restored_root = root.path().join("restored");
    let restored_manifest = FabricStore::restore_backup(&backup_root, &restored_root)
        .expect("validated empty-root restore");
    assert_eq!(restored_manifest, manifest);
    assert_eq!(
        FabricStore::open(&restored_root)
            .unwrap()
            .snapshot()
            .unwrap(),
        expected
    );

    let occupied = root.path().join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("authority.jsonl"), b"do-not-overwrite").unwrap();
    assert_eq!(
        FabricStore::restore_backup(&backup_root, &occupied)
            .expect_err("restore cannot overwrite existing authority")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    OpenOptions::new()
        .append(true)
        .open(backup_root.join("fabric-transactions.jsonl"))
        .unwrap()
        .write_all(b"tamper")
        .unwrap();
    let tampered_target = root.path().join("tampered-target");
    assert_eq!(
        FabricStore::restore_backup(&backup_root, &tampered_target)
            .expect_err("digest-bound backup rejects tampering")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    assert!(!tampered_target.exists());
}

#[test]
fn node_local_journal_recovers_lost_ack_without_duplicate_native_effect() {
    let source_root = TestRoot::new("local-source-recovery");
    let request = operation(1, 1);
    let source_session = fabric_session("node-a", 1, 1);
    let source =
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a").expect("source store");
    source
        .bind_gateway_session(&source_session)
        .expect("bind source session");
    let before = source.snapshot().expect("source snapshot");
    source.fail_next_commit_for_test();
    let rejected = source
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            100,
        )
        .expect_err("failure before append must be effect-none");
    assert_eq!(rejected.code, FabricErrorCode::StoreUnavailable);
    assert_eq!(rejected.effect, EffectCertainty::None);
    assert_eq!(source.snapshot().expect("source snapshot"), before);

    source.fail_after_append_for_test();
    let unknown = source
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            101,
        )
        .expect_err("lost local append acknowledgement is unknown");
    assert_eq!(unknown.code, FabricErrorCode::RecoveryRequired);
    assert_eq!(unknown.effect, EffectCertainty::Unknown);
    assert_eq!(
        source
            .snapshot()
            .expect_err("source latches unavailable")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    drop(source);
    let recovered_source = NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a")
        .expect("reopen source store");
    let (_, replayed) = recovered_source
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            102,
        )
        .expect("exact outbox replay after reopen");
    assert!(replayed);
    assert_eq!(
        recovered_source
            .snapshot()
            .expect("source snapshot")
            .outboxes
            .len(),
        1
    );
    for (company_id, node_id) in [(COMPANY, "node-b"), ("company-foreign", "node-a")] {
        let error = match NodeLocalFabricStore::open(source_root.path(), company_id, node_id) {
            Ok(_) => panic!("durably bound Node-local root must reject another authority"),
            Err(error) => error,
        };
        assert_eq!(error.code, FabricErrorCode::WrongCompany);
        assert_eq!(error.effect, EffectCertainty::None);
    }

    let target_root = TestRoot::new("local-target-recovery");
    let target =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let target_session = fabric_session("node-b", 1, 1);
    target
        .bind_gateway_session(&target_session)
        .expect("bind target session");
    let attempt = RouteAttempt {
        id: "route-attempt:operation-1:1".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: 1,
        control_plane_generation: 1,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    target
        .persist_inbox(&target_session, &request, &attempt, 100)
        .expect("persist target inbox");
    target
        .claim_inbox(&target_session, &request, 101)
        .expect("claim before native effect");
    assert_eq!(
        target.unresolved_operation_ids().expect("unresolved ids"),
        BTreeSet::from([request.id.clone()])
    );
    let duplicate_claim = target
        .claim_inbox(&target_session, &request, 102)
        .expect_err("duplicate claim cannot blindly repeat a native effect");
    assert_eq!(duplicate_claim.code, FabricErrorCode::RecoveryRequired);
    target.fail_after_append_for_test();
    let unknown = target
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            103,
        )
        .expect_err("native result append acknowledgement is unknown");
    assert_eq!(unknown.code, FabricErrorCode::RecoveryRequired);
    drop(target);
    let recovered_target = NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b")
        .expect("reopen target store");
    let (inbox, result, replayed) = recovered_target
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            104,
        )
        .expect("exact native result replay after reopen");
    assert!(replayed);
    assert_eq!(inbox.state, LocalInboxState::Applied);
    assert_eq!(result.effect, EffectCertainty::Applied);
    assert!(recovered_target
        .unresolved_operation_ids()
        .expect("unresolved ids")
        .is_empty());
    assert_eq!(
        recovered_target
            .snapshot()
            .expect("target snapshot")
            .results
            .len(),
        1
    );
}

#[test]
fn two_process_style_node_outbox_handles_share_one_atomic_journal() {
    let root = TestRoot::new("cross-process-node-local-lock");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut joins = Vec::new();
    for _ in 0..2 {
        let root = root.path().to_path_buf();
        let barrier = barrier.clone();
        joins.push(std::thread::spawn(move || {
            let store = NodeLocalFabricStore::open(root, COMPANY, "node-a")
                .expect("open independent Node-local handle");
            let request = operation(1, 1);
            let session = fabric_session("node-a", 1, 1);
            let source_actor = request.actor.clone();
            store
                .bind_gateway_session(&session)
                .expect("bind shared exact session");
            barrier.wait();
            store.prepare_outbox(&session, &source_actor, &request, 100)
        }));
    }
    let results = joins
        .into_iter()
        .map(|join| join.join().expect("join writer").expect("prepare outbox"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|(_, replay)| !replay).count(), 1);
    assert_eq!(results.iter().filter(|(_, replay)| *replay).count(), 1);
    assert_eq!(
        NodeLocalFabricStore::open(root.path(), COMPANY, "node-a")
            .expect("reopen Node-local Store")
            .snapshot()
            .expect("snapshot")
            .outboxes
            .len(),
        1
    );
}

#[test]
fn node_local_gateway_session_queues_exact_recoverable_operation_and_fences_predecessor() {
    let root = TestRoot::new("durable-local-session-outbox");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a").expect("local store");
    let session = fabric_session("node-a", 2, 3);
    store
        .bind_gateway_session(&session)
        .expect("bind authenticated gateway session");
    let mut request = operation(2, 3);
    request.source_gateway_generation = Some(2);
    request.control_plane_generation = 3;
    request.body_digest = json_digest(&request.body).unwrap();
    let actor = request.actor.clone();
    let (outbox, replayed) = store
        .prepare_outbox(&session, &actor, &request, 100)
        .expect("queue exact operation");
    assert!(!replayed);
    assert_eq!(outbox.operation.as_ref(), Some(&request));
    assert_eq!(
        store.pending_outbox_operations().expect("pending queue"),
        vec![request]
    );
    let before = store.snapshot().expect("before stale bind");
    assert_eq!(
        store
            .bind_gateway_session(&fabric_session("node-a", 1, 3))
            .expect_err("predecessor gateway cannot overwrite durable session")
            .code,
        FabricErrorCode::NodeStaleGeneration
    );
    assert_eq!(store.snapshot().expect("after stale bind"), before);
    assert_eq!(store.active_session().unwrap(), Some(session));
}

#[test]
fn target_successor_session_fences_predecessor_before_claim_or_result_side_effects() {
    let root = TestRoot::new("target-successor-local-fence");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-b").expect("local store");
    let predecessor = fabric_session("node-b", 4, 3);
    store
        .bind_gateway_session(&predecessor)
        .expect("bind predecessor");
    let request = operation(2, 3);
    let attempt = RouteAttempt {
        id: "route-attempt:successor-fence:1".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: predecessor.gateway_generation,
        control_plane_generation: predecessor.control_plane_generation,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    store
        .persist_inbox(&predecessor, &request, &attempt, 100)
        .expect("predecessor persists before takeover");

    let successor = fabric_session("node-b", 5, 3);
    store
        .bind_gateway_session(&successor)
        .expect("bind successor");
    let before = store.snapshot().expect("snapshot after successor bind");

    for rejected in [
        store.claim_inbox(&predecessor, &request, 101).map(|_| ()),
        store
            .record_application_result(
                &predecessor,
                &request.id,
                "agentfirm.remote_fabric.probe_result.v1",
                json!({"must_not_exist": true}),
                EffectCertainty::Applied,
                101,
            )
            .map(|_| ()),
    ] {
        let error = rejected.expect_err("predecessor must be fenced under the Store lock");
        assert_eq!(error.code, FabricErrorCode::NodeStaleGeneration);
        assert_eq!(error.effect, EffectCertainty::None);
        assert_eq!(store.snapshot().expect("zero-delta snapshot"), before);
    }
    assert!(store.snapshot().unwrap().results.is_empty());
    assert_eq!(
        store.snapshot().unwrap().inboxes[&request.id].state,
        LocalInboxState::Persisted
    );
}

#[test]
fn offline_source_outbox_rebinds_only_after_empty_current_generation_reconcile() {
    let root = TestRoot::new("offline-source-successor-rebind");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a").expect("local store");
    let predecessor = fabric_session("node-a", 2, 3);
    store.bind_gateway_session(&predecessor).unwrap();
    let mut request = operation(2, 3);
    request.source_gateway_generation = Some(2);
    request.control_plane_generation = 3;
    request.body_digest = json_digest(&request.body).unwrap();
    store
        .prepare_outbox(&predecessor, &request.actor, &request, 100)
        .expect("durable offline queue");

    let successor = fabric_session("node-a", 3, 4);
    store.bind_gateway_session(&successor).unwrap();
    let rebound = store
        .rebind_unaccepted_outbox(&successor, &request.id, &[])
        .expect("empty current-generation reconciliation proves pre-acceptance rebind is safe");
    assert_eq!(rebound.source_gateway_generation, Some(3));
    assert_eq!(rebound.control_plane_generation, 4);
    assert_eq!(
        store.pending_outbox_operations().unwrap(),
        vec![rebound.clone()]
    );

    let accepted = RouteReceipt {
        id: "accepted-receipt".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        target_node_id: "node-b".into(),
        target_gateway_generation: 9,
        control_plane_generation: 4,
        route_seq: 1,
        kind: ReceiptKind::ControlPlaneAccepted,
        application_effect: None,
        result_schema: None,
        result: None,
        result_digest: None,
        error: None,
        created_at_unix_ms: 101,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let before = store.snapshot().unwrap();
    let error = store
        .rebind_unaccepted_outbox(&successor, &request.id, std::slice::from_ref(&accepted))
        .expect_err("accepted route truth cannot be rebound");
    assert_eq!(error.code, FabricErrorCode::IdempotencyConflict);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().unwrap(), before);

    store
        .mark_outbox_receipt(&successor, &accepted)
        .expect("Control Plane acceptance becomes sole route truth");
    let accepted_before_expiry = store.snapshot().unwrap();
    assert_eq!(
        store
            .expire_unaccepted_outbox(&successor, &request.id, 1_000)
            .expect("accepted operation is left to reconciliation"),
        None
    );
    assert_eq!(store.snapshot().unwrap(), accepted_before_expiry);
}

#[test]
fn expired_unaccepted_source_outbox_settles_locally_without_route_or_native_effect() {
    let root = TestRoot::new("expired-unaccepted-source-outbox");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a").expect("local store");
    let predecessor = fabric_session("node-a", 2, 3);
    store.bind_gateway_session(&predecessor).unwrap();
    let mut request = operation(2, 3);
    request.source_gateway_generation = Some(2);
    request.control_plane_generation = 3;
    request.expires_at_unix_ms = 150;
    request.body_digest = json_digest(&request.body).unwrap();
    store
        .prepare_outbox(&predecessor, &request.actor, &request, 100)
        .expect("durable offline queue");

    let successor = fabric_session("node-a", 3, 4);
    store.bind_gateway_session(&successor).unwrap();
    let before = store.snapshot().unwrap();
    let early = store
        .expire_unaccepted_outbox(&successor, &request.id, 149)
        .expect_err("live operation cannot be locally expired");
    assert_eq!(early.code, FabricErrorCode::ExpectedRevisionConflict);
    assert_eq!(store.snapshot().unwrap(), before);

    let terminal = store
        .expire_unaccepted_outbox(&successor, &request.id, 150)
        .expect("expired unaccepted operation settles locally")
        .expect("local terminal result");
    assert_eq!(terminal.local_state, LocalOutboxState::Terminal);
    assert_eq!(
        terminal.terminal_receipt_ref.as_deref(),
        Some("local:not_applied:operation_expired:operation-1")
    );
    assert!(store.pending_outbox_operations().unwrap().is_empty());
    let settled = store.snapshot().unwrap();
    assert!(settled.inboxes.is_empty());
    assert!(settled.results.is_empty());

    assert_eq!(
        store
            .expire_unaccepted_outbox(&successor, &request.id, 151)
            .expect("terminal replay is stable")
            .expect("local terminal replay"),
        terminal
    );
    let stale = store
        .expire_unaccepted_outbox(&predecessor, &request.id, 151)
        .expect_err("predecessor cannot mutate successor state");
    assert_eq!(stale.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(store.snapshot().unwrap(), settled);
}

#[test]
fn node_local_store_rejects_foreign_and_stale_sessions_with_zero_delta() {
    let source_root = TestRoot::new("local-session-fence-source");
    let source =
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a").expect("source store");
    let request = operation(7, 3);
    let before = source.snapshot().expect("source snapshot");
    for hostile in [
        fabric_session("node-b", 7, 3),
        fabric_session("node-a", 6, 3),
        fabric_session("node-a", 7, 2),
    ] {
        let error = source
            .prepare_outbox(
                &hostile,
                &actor("fabric-client", &["fabric_submit"]),
                &request,
                100,
            )
            .expect_err("foreign or stale source session must fail closed");
        assert!(matches!(
            error.code,
            FabricErrorCode::SourceMismatch | FabricErrorCode::NodeStaleGeneration
        ));
        assert_eq!(error.effect, EffectCertainty::None);
        assert_eq!(source.snapshot().expect("source snapshot"), before);
    }
    let error = source
        .prepare_outbox(
            &fabric_session("node-a", 7, 3),
            &actor("sibling-agent", &["fabric_submit"]),
            &request,
            100,
        )
        .expect_err("wire actor cannot differ from authenticated source actor");
    assert_eq!(error.code, FabricErrorCode::UnauthorizedActor);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(source.snapshot().expect("source snapshot"), before);

    let target_root = TestRoot::new("local-session-fence-target");
    let target =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let attempt = RouteAttempt {
        id: "route-attempt:operation-1:1".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: 9,
        control_plane_generation: 3,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let before = target.snapshot().expect("target snapshot");
    for hostile in [
        fabric_session("node-a", 9, 3),
        fabric_session("node-b", 8, 3),
        fabric_session("node-b", 9, 2),
    ] {
        let error = target
            .persist_inbox(&hostile, &request, &attempt, 100)
            .expect_err("foreign or stale target session must fail closed");
        assert!(matches!(
            error.code,
            FabricErrorCode::SourceMismatch | FabricErrorCode::NodeStaleGeneration
        ));
        assert_eq!(error.effect, EffectCertainty::None);
        assert_eq!(target.snapshot().expect("target snapshot"), before);
    }
    let mut hostile_body = request.clone();
    hostile_body.body["authority"] = json!("browser-selected-host");
    hostile_body.body_digest = json_digest(&hostile_body.body).expect("hostile body digest");
    let error = target
        .persist_inbox(
            &fabric_session("node-b", 9, 3),
            &hostile_body,
            &attempt,
            100,
        )
        .expect_err("target independently rejects an unregistered body shape");
    assert_eq!(error.code, FabricErrorCode::InvalidPayload);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(target.snapshot().expect("target snapshot"), before);
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
    let first_hello = hello("node-b", "gateway-b-1", "cert-b", &fingerprint("node-b"));
    let first_gateway = connect_node(
        &control,
        lease.control_plane_generation,
        &first_hello,
        &signing_key("node-b"),
        103,
    )
    .expect("connect first target gateway");
    control
        .issue_gateway_download_capability(
            &reader,
            lease.control_plane_generation,
            first_gateway.gateway_generation,
            &first_gateway.node_daemon_id,
            first_gateway.node_daemon_generation,
            &manifest.id,
            "node-b",
            104,
        )
        .expect("current target gateway can request exact artifact capability");
    control
        .heartbeat_lease(lease.control_plane_generation, lease.revision, 29_000)
        .expect("keep Control Plane authority current");
    let successor_hello = rotate_gateway_certificate(
        &control,
        &store,
        lease.control_plane_generation,
        &first_gateway,
        "node-b",
        "cert-b",
        "cert-b-successor",
        29_500,
    );
    connect_node(
        &control,
        lease.control_plane_generation,
        &successor_hello,
        &signing_key("node-b"),
        30_104,
    )
    .expect("successor target gateway binds after predecessor expiry");
    let before_stale_capability = store.snapshot().expect("snapshot");
    let stale_capability = control
        .issue_gateway_download_capability(
            &reader,
            lease.control_plane_generation,
            first_gateway.gateway_generation,
            &first_gateway.node_daemon_id,
            first_gateway.node_daemon_generation,
            &manifest.id,
            "node-b",
            30_105,
        )
        .expect_err("predecessor gateway cannot mint artifact capability");
    assert_eq!(stale_capability.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(stale_capability.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale_capability);
    let download = control
        .issue_download_capability(
            &reader,
            lease.control_plane_generation,
            &manifest.id,
            "node-b",
            30_106,
        )
        .expect("issue download capability");
    assert_eq!(
        control
            .download_artifact(lease.control_plane_generation, &download, 30_107)
            .expect("decrypt artifact"),
        bytes
    );
    let consumed = control
        .download_artifact(lease.control_plane_generation, &download, 30_108)
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
            30_109,
        )
        .expect("manifest can precede content inspection");
    let before = store.snapshot().expect("snapshot");
    let rejected = control
        .complete_artifact(
            lease.control_plane_generation,
            &secret_upload,
            secret,
            30_110,
        )
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
    let hello: NodeHello = serde_json::from_slice(
        &std::fs::read(root.join("node-hello.json")).expect("read NodeHello fixture"),
    )
    .expect("NodeHello fixture matches Rust");
    assert_eq!(hello.protocol_min, FABRIC_PROTOCOL_VERSION);
    let welcome: NodeWelcome = serde_json::from_slice(
        &std::fs::read(root.join("node-welcome.json")).expect("read NodeWelcome fixture"),
    )
    .expect("NodeWelcome fixture matches Rust");
    assert_eq!(welcome.gateway_generation, 1);
    let frame: FabricFrame = serde_json::from_slice(
        &std::fs::read(root.join("fabric-frame.json")).expect("read FabricFrame fixture"),
    )
    .expect("FabricFrame fixture matches Rust");
    frame
        .validate()
        .expect("FabricFrame fixture digest matches");
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
fn two_process_style_control_plane_stores_have_one_exclusive_generation_winner() {
    let root = TestRoot::new("cross-process-control-plane-lock");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut joins = Vec::new();
    for instance in ["control-a", "control-b"] {
        let root = root.path().to_path_buf();
        let barrier = barrier.clone();
        joins.push(std::thread::spawn(move || {
            let store = FabricStore::open(root).expect("open independent FabricStore handle");
            let keys = InMemoryArtifactKeyBackend::default();
            keys.insert(COMPANY, [7; 32]);
            let control = ControlPlane::new(COMPANY, instance, &store, &keys, [9; 32]);
            barrier.wait();
            control.acquire_lease(&format!("lease-{instance}"), 0, 1)
        }));
    }
    let results = joins
        .into_iter()
        .map(|join| join.join().expect("join competitor"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        FabricStore::open(root.path())
            .expect("reopen authoritative Store")
            .snapshot()
            .expect("snapshot")
            .control_plane_leases
            .len(),
        1
    );
}

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

#[test]
fn wire_config_and_frame_codec_are_closed_and_generation_fenced() {
    let config = NodeFabricConfig {
        company_id: COMPANY.into(),
        node_id: "node-a".into(),
        control_plane_url: "wss://control.agentfirm.test/v1/node-gateway/connect".into(),
        reconnect_floor_ms: 250,
        reconnect_ceiling_ms: 10_000,
    };
    config.validate().expect("outbound secure endpoint");
    for endpoint in [
        "ws://control.agentfirm.test/v1/node-gateway/connect",
        "https://control.agentfirm.test/v1/node-gateway/connect",
        "wss://token@control.agentfirm.test/v1/node-gateway/connect",
        "wss://control.agentfirm.test/v1/node-gateway/connect?node=browser-selected",
    ] {
        let mut hostile = config.clone();
        hostile.control_plane_url = endpoint.into();
        assert!(hostile.validate().is_err(), "must reject {endpoint}");
    }

    let payload = FabricPayload::Heartbeat {
        observed_at_unix_ms: 100,
    };
    let frame = FabricFrame::new(
        "frame-1",
        COMPANY,
        "node-a",
        3,
        "node-daemon:node-a",
        5,
        2,
        100,
        "correlation-1",
        payload,
    )
    .expect("create frame");
    let bytes = encode_frame(&frame).expect("encode frame");
    assert_eq!(decode_frame(&bytes).expect("decode frame"), frame);
    FabricSessionFence {
        company_id: COMPANY.into(),
        node_id: "node-a".into(),
        gateway_generation: 3,
        node_daemon_id: "node-daemon:node-a".into(),
        node_daemon_generation: 5,
        control_plane_generation: 2,
    }
    .validate_frame(&frame)
    .expect("exact session fence");
    let before = frame.clone();
    let stale = FabricSessionFence {
        company_id: COMPANY.into(),
        node_id: "node-a".into(),
        gateway_generation: 4,
        node_daemon_id: "node-daemon:node-a".into(),
        node_daemon_generation: 5,
        control_plane_generation: 2,
    }
    .validate_frame(&frame)
    .expect_err("successor generation fences predecessor frame");
    assert_eq!(stale.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(frame, before);

    let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).expect("frame JSON");
    unknown["browser_authority"] = json!("host");
    let unknown_bytes = serde_json::to_vec(&unknown).expect("encode hostile frame");
    assert_eq!(
        decode_frame(&unknown_bytes)
            .expect_err("unknown field must fail closed")
            .code,
        FabricErrorCode::InvalidPayload
    );
    assert_eq!(
        decode_frame(&vec![b'x'; MAX_FABRIC_FRAME_BYTES + 1])
            .expect_err("oversized frame must fail before parsing")
            .code,
        FabricErrorCode::InvalidPayload
    );
}

#[test]
fn outbound_mtls_identity_rejects_missing_and_symlinked_key_material_before_network() {
    use firm_fabric::transport::NodeTlsIdentityFiles;
    let root = TestRoot::new("tls-identity-files");
    let cert = root.path().join("client-cert.pem");
    let key = root.path().join("client-key.pem");
    let ca = root.path().join("ca.pem");
    std::fs::write(&cert, b"certificate").expect("write certificate fixture");
    std::fs::write(&key, b"key").expect("write key fixture");
    secure_private_key(&key);
    std::fs::write(&ca, b"ca").expect("write CA fixture");
    let identity = NodeTlsIdentityFiles {
        client_certificate_chain_pem: cert.clone(),
        client_private_key_pem: key.clone(),
        control_plane_ca_pem: ca,
    };
    identity.validate().expect("regular credential handles");

    let linked_key = root.path().join("linked-key.pem");
    std::os::unix::fs::symlink(&key, &linked_key).expect("create hostile symlink");
    let hostile = NodeTlsIdentityFiles {
        client_private_key_pem: linked_key,
        ..identity
    };
    assert_eq!(
        hostile
            .validate()
            .expect_err("credential loader cannot follow a key symlink")
            .effect,
        EffectCertainty::None
    );
}

#[test]
fn control_plane_ca_issues_exact_company_execution_node_client_identity() {
    let company = "company-a";
    let node = "11111111-1111-4111-8111-111111111111";
    let ca = firm_fabric::pki::generate_ca(company).expect("generate Company CA");
    let csr = firm_fabric::pki::generate_node_csr(company, node).expect("generate Node CSR");
    let issued = firm_fabric::pki::issue_node_certificate(&ca, &csr.csr_pem, company, node, 1_000)
        .expect("issue exact Node client certificate");
    let certificates = rustls_pemfile::certs(&mut std::io::BufReader::new(
        issued.certificate_pem.as_bytes(),
    ))
    .collect::<Result<Vec<_>, _>>()
    .expect("parse issued certificate");
    let identity = firm_fabric::pki::parse_peer_node_identity(&certificates[0])
        .expect("parse mTLS peer identity");
    assert_eq!(identity.company_id, company);
    assert_eq!(identity.node_id, node);
    assert_eq!(identity.public_key_fingerprint, csr.public_key_fingerprint);
    assert_eq!(identity.certificate_serial, issued.serial);

    let hostile = firm_fabric::pki::issue_node_certificate(
        &ca,
        &csr.csr_pem,
        company,
        "22222222-2222-4222-8222-222222222222",
        1_000,
    )
    .expect_err("CSR cannot be reassigned to another ExecutionNode");
    assert_eq!(hostile.code, FabricErrorCode::UnauthorizedActor);
}

#[test]
fn real_loopback_wss_requires_mtls_hostname_and_frozen_subprotocol() {
    use firm_fabric::transport::{
        accept_control_plane_mtls, connect_outbound_mtls, connect_outbound_mtls_material,
        ControlPlaneTlsFiles, NodeFabricConfig, NodeTlsIdentityFiles, NodeTlsIdentityMaterial,
    };
    let root = TestRoot::new("real-loopback-mtls");
    let company = "company-a";
    let node = "11111111-1111-4111-8111-111111111111";
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64;
    let ca = firm_fabric::pki::generate_ca(company).expect("generate CA");
    let csr = firm_fabric::pki::generate_node_csr(company, node).expect("generate Node CSR");
    let node_certificate =
        firm_fabric::pki::issue_node_certificate(&ca, &csr.csr_pem, company, node, now_unix_ms)
            .expect("issue Node certificate");
    let server_certificate =
        firm_fabric::pki::issue_control_plane_server_certificate(&ca, "localhost", now_unix_ms)
            .expect("issue server certificate");
    let ca_path = root.path().join("ca.pem");
    let node_cert_path = root.path().join("node.pem");
    let node_key_path = root.path().join("node-key.pem");
    let server_cert_path = root.path().join("server.pem");
    let server_key_path = root.path().join("server-key.pem");
    std::fs::write(&ca_path, &ca.certificate_pem).unwrap();
    std::fs::write(
        &node_cert_path,
        format!("{}{}", node_certificate.certificate_pem, ca.certificate_pem),
    )
    .unwrap();
    std::fs::write(&node_key_path, &csr.private_key_pem).unwrap();
    std::fs::write(&server_cert_path, &server_certificate.certificate_chain_pem).unwrap();
    std::fs::write(&server_key_path, &server_certificate.private_key_pem).unwrap();
    secure_private_key(&node_key_path);
    secure_private_key(&server_key_path);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind Control Plane");
    let port = listener.local_addr().unwrap().port();
    let server_tls = ControlPlaneTlsFiles {
        server_certificate_chain_pem: server_cert_path,
        server_private_key_pem: server_key_path,
        node_ca_pem: ca_path.clone(),
    };
    let server = std::thread::spawn(move || {
        (0..2)
            .map(|_| {
                let (tcp, _) = listener.accept().expect("accept outbound Node connection");
                let (mut socket, identity) = accept_control_plane_mtls(tcp, &server_tls)
                    .expect("accept verified mTLS WebSocket");
                let frame = firm_fabric::transport::read_frame(&mut socket)
                    .expect("read authenticated Fabric frame");
                socket.close(None).ok();
                (identity, frame)
            })
            .collect::<Vec<_>>()
    });
    let config = NodeFabricConfig {
        company_id: company.into(),
        node_id: node.into(),
        control_plane_url: format!(
            "wss://localhost:{port}{}",
            firm_fabric::transport::FABRIC_GATEWAY_PATH
        ),
        reconnect_floor_ms: 100,
        reconnect_ceiling_ms: 1_000,
    };
    let client_tls = NodeTlsIdentityFiles {
        client_certificate_chain_pem: node_cert_path,
        client_private_key_pem: node_key_path,
        control_plane_ca_pem: ca_path,
    };
    let mut socket = connect_outbound_mtls(&config, &client_tls).expect("connect outbound mTLS");
    let sent = FabricFrame::new(
        "frame-loopback-heartbeat",
        company,
        node,
        7,
        format!("node-daemon:{node}"),
        3,
        11,
        now_unix_ms,
        "correlation-loopback",
        FabricPayload::Heartbeat {
            observed_at_unix_ms: now_unix_ms,
        },
    )
    .expect("build Fabric frame");
    firm_fabric::transport::write_frame(&mut socket, &sent)
        .expect("write authenticated Fabric frame");
    socket.close(None).ok();
    let keychain_like = NodeTlsIdentityMaterial {
        client_certificate_chain_pem: format!(
            "{}{}",
            node_certificate.certificate_pem, ca.certificate_pem
        )
        .into_bytes(),
        client_private_key_pem: csr.private_key_pem.as_bytes().to_vec(),
        control_plane_ca_pem: ca.certificate_pem.as_bytes().to_vec(),
    };
    let mut memory_socket = connect_outbound_mtls_material(&config, &keychain_like)
        .expect("connect using OS-credential material without a temporary private-key file");
    firm_fabric::transport::write_frame(&mut memory_socket, &sent).unwrap();
    memory_socket.close(None).ok();
    let observed_sessions = server.join().expect("join Control Plane");
    for (identity, observed) in observed_sessions {
        assert_eq!(identity.company_id, company);
        assert_eq!(identity.node_id, node);
        assert_eq!(identity.public_key_fingerprint, csr.public_key_fingerprint);
        assert_eq!(observed, sent);
    }
}

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

#[test]
fn durable_rate_limit_rejects_new_work_but_preserves_exact_replay() {
    let root = TestRoot::new("rate-limit");
    let store = FabricStore::open_with_limits(
        root.path(),
        FabricStoreLimits {
            max_operations_per_minute_per_source_actor: 1,
            ..FabricStoreLimits::default()
        },
    )
    .expect("open bounded store");
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
    connect_node(
        &control,
        lease.control_plane_generation,
        &target_hello,
        &signing_key("node-b"),
        30,
    )
    .expect("target connect");
    let first = operation(source.gateway_generation, lease.control_plane_generation);
    accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        first.clone(),
        100,
    )
    .expect("first operation is within limit");
    assert!(
        accept_fabric_operation(
            &control,
            lease.control_plane_generation,
            source.gateway_generation,
            first.clone(),
            101,
        )
        .expect("exact replay bypasses new-work rate accounting")
        .3
    );
    let before = store.snapshot().expect("snapshot");
    let mut second = first;
    second.id = "operation-rate-limited".into();
    second.idempotency_key = "idempotency-rate-limited".into();
    let limited = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        second,
        102,
    )
    .expect_err("new operation exceeds durable rate limit");
    assert_eq!(limited.code, FabricErrorCode::RateLimited);
    assert!(limited.retryable);
    assert_eq!(limited.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}

#[test]
fn offline_queue_capacity_rejects_at_count_and_over_bytes_with_zero_delta() {
    fn bounded_control(label: &str, limits: FabricStoreLimits) -> (TestRoot, FabricStore) {
        let root = TestRoot::new(label);
        let store = FabricStore::open_with_limits(root.path(), limits).expect("bounded store");
        (root, store)
    }

    for (label, limits) in [
        (
            "queue-count-boundary",
            FabricStoreLimits {
                max_queued_operations_per_node: 1,
                ..FabricStoreLimits::default()
            },
        ),
        (
            "queue-byte-boundary",
            FabricStoreLimits {
                max_queued_bytes_per_node: 1,
                ..FabricStoreLimits::default()
            },
        ),
    ] {
        let (_root, store) = bounded_control(label, limits);
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
        connect_node(
            &control,
            lease.control_plane_generation,
            &target_hello,
            &signing_key("node-b"),
            30,
        )
        .expect("target connect");

        let first = operation(source.gateway_generation, lease.control_plane_generation);
        if label == "queue-count-boundary" {
            accept_fabric_operation(
                &control,
                lease.control_plane_generation,
                source.gateway_generation,
                first.clone(),
                100,
            )
            .expect("the one allowed queue slot is accepted");
        }
        let before = store
            .snapshot()
            .expect("snapshot before capacity rejection");
        let mut rejected_operation = first;
        rejected_operation.id = format!("operation-{label}");
        rejected_operation.idempotency_key = format!("idempotency-{label}");
        let rejected = accept_fabric_operation(
            &control,
            lease.control_plane_generation,
            source.gateway_generation,
            rejected_operation,
            101,
        )
        .expect_err("capacity boundary rejects without accepting partial route state");
        assert_eq!(rejected.code, FabricErrorCode::QueueCapacity);
        assert_eq!(rejected.effect, EffectCertainty::None);
        assert_eq!(store.snapshot().expect("snapshot after rejection"), before);
    }
}

#[test]
fn expired_offline_operation_cannot_persist_or_cross_native_effect_boundary() {
    let target_root = TestRoot::new("expired-target");
    let target =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let session = fabric_session("node-b", 1, 1);
    target.bind_gateway_session(&session).expect("bind session");
    let request = operation(1, 1);
    let attempt = RouteAttempt {
        id: "route-attempt:operation-1:1".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: 1,
        control_plane_generation: 1,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Sent,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let before = target.snapshot().expect("snapshot");
    let expired = target
        .persist_inbox(&session, &request, &attempt, request.expires_at_unix_ms)
        .expect_err("expired offline work cannot be persisted on reconnect");
    assert_eq!(expired.code, FabricErrorCode::OperationExpired);
    assert_eq!(expired.effect, EffectCertainty::None);
    assert_eq!(target.snapshot().expect("snapshot"), before);

    target
        .persist_inbox(&session, &request, &attempt, request.expires_at_unix_ms - 1)
        .expect("unexpired operation can persist");
    let persisted = target.snapshot().expect("persisted snapshot");
    let expired_claim = target
        .claim_inbox(&session, &request, request.expires_at_unix_ms)
        .expect_err("expiry is rechecked before native effect");
    assert_eq!(expired_claim.code, FabricErrorCode::OperationExpired);
    assert_eq!(expired_claim.effect, EffectCertainty::None);
    assert_eq!(target.snapshot().expect("snapshot"), persisted);
}

#[test]
fn successor_reconnect_settles_expired_offline_operation_as_not_applied() {
    let root = TestRoot::new("expired-successor-control");
    let target_root = TestRoot::new("expired-successor-target");
    let store = FabricStore::open(root.path()).expect("open store");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
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
    let mut request = operation(source.gateway_generation, lease.control_plane_generation);
    request.expires_at_unix_ms = 200;
    let (_, first_attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        request.clone(),
        100,
    )
    .expect("queue while target is offline");
    let target_before = target_local.snapshot().expect("target before reconnect");
    control
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
    .expect("successor reconnect settles expiry");
    let state = store.snapshot().expect("settled state");
    let attempt = state
        .attempts
        .get(&first_attempt.id)
        .expect("first attempt");
    assert_eq!(attempt.state, RouteAttemptState::Ended);
    assert_eq!(attempt.error_code, Some(FabricErrorCode::OperationExpired));
    assert_eq!(attempt.effect, EffectCertainty::None);
    assert_eq!(attempt.ended_at_unix_ms, Some(30_031));
    let receipt = state
        .receipts
        .values()
        .find(|receipt| {
            receipt.operation_id == request.id && receipt.kind == ReceiptKind::OperationRejected
        })
        .expect("typed terminal expiry receipt");
    assert_eq!(
        receipt.target_gateway_generation,
        successor.gateway_generation
    );
    assert_eq!(
        receipt.application_effect,
        Some(EffectCertainty::NotApplied)
    );
    assert_eq!(
        receipt.error.as_ref().map(|error| error.code),
        Some(FabricErrorCode::OperationExpired)
    );
    assert_eq!(
        receipt.result_schema.as_deref(),
        Some("agentfirm.remote_fabric.expired.v1")
    );
    assert_eq!(
        target_local.snapshot().expect("target after reconnect"),
        target_before
    );
}

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

#[test]
fn enrollment_revocation_is_exact_cas_and_prevents_later_consumption() {
    let root = TestRoot::new("enrollment-revoke");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    let host = actor("host", &["company_host"]);
    let enrollment = control
        .create_enrollment(
            &host,
            lease.control_plane_generation,
            "enroll-revoked",
            TOKEN_A,
            "Node revoked before join",
            BTreeSet::new(),
            1000,
            2,
        )
        .expect("create enrollment");
    let before_stale = store.snapshot().expect("snapshot");
    let stale = control
        .revoke_enrollment(
            &host,
            lease.control_plane_generation,
            &enrollment.id,
            enrollment.revision + 1,
            3,
        )
        .expect_err("stale CAS cannot revoke enrollment");
    assert_eq!(stale.code, FabricErrorCode::ExpectedRevisionConflict);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
    let revoked = control
        .revoke_enrollment(
            &host,
            lease.control_plane_generation,
            &enrollment.id,
            enrollment.revision,
            3,
        )
        .expect("revoke pending enrollment");
    assert_eq!(revoked.status, EnrollmentStatus::Revoked);
    assert_eq!(revoked.revision, enrollment.revision + 1);
    let before_consume = store.snapshot().expect("snapshot");
    let rejected = control
        .consume_enrollment(
            lease.control_plane_generation,
            TOKEN_A,
            "node-a",
            "Node A",
            &enrollment_proof("enroll-revoked", "node-a", "cert-a"),
            "cert-a",
            10_000,
            SCHEMA_DIGEST,
            4,
        )
        .expect_err("revoked token cannot be consumed");
    assert_eq!(rejected.code, FabricErrorCode::EnrollmentRevoked);
    assert_eq!(store.snapshot().expect("snapshot"), before_consume);
}

#[test]
fn control_plane_successor_immediately_fences_prior_live_gateway_generation() {
    let root = TestRoot::new("control-plane-takeover");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let old = ControlPlane::new(COMPANY, "control-old", &store, &keys, [9; 32]);
    let first = old.acquire_lease("cp-lease-1", 0, 1).expect("first lease");
    enroll_nodes(&old, first.control_plane_generation);
    let hello_one = hello("node-a", "gateway-a-1", "cert-a", &fingerprint("node-a"));
    let gateway_one = connect_node(
        &old,
        first.control_plane_generation,
        &hello_one,
        &signing_key("node-a"),
        30,
    )
    .expect("first gateway");
    old.heartbeat_gateway(
        first.control_plane_generation,
        "node-a",
        gateway_one.gateway_generation,
        &gateway_one.node_daemon_id,
        gateway_one.node_daemon_generation,
        1,
        29_000,
    )
    .expect("old gateway lease remains live past Control Plane expiry");
    let successor = ControlPlane::new(COMPANY, "control-new", &store, &keys, [9; 32]);
    let second = successor
        .acquire_lease("cp-lease-2", first.revision, 30_001)
        .expect("Control Plane successor");
    let hello_two = hello("node-a", "gateway-a-2", "cert-a", &fingerprint("node-a"));
    let gateway_two = connect_node(
        &successor,
        second.control_plane_generation,
        &hello_two,
        &signing_key("node-a"),
        30_002,
    )
    .expect("new Control Plane generation fences old live gateway immediately");
    assert_eq!(
        gateway_two.gateway_generation,
        gateway_one.gateway_generation + 1
    );
    let before = store.snapshot().expect("snapshot");
    let stale = old
        .heartbeat_gateway(
            first.control_plane_generation,
            "node-a",
            gateway_one.gateway_generation,
            &gateway_one.node_daemon_id,
            gateway_one.node_daemon_generation,
            2,
            30_003,
        )
        .expect_err("old Control Plane and gateway generations are fenced");
    assert_eq!(stale.code, FabricErrorCode::ControlPlaneStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}

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

#[test]
fn expired_ordering_tombstone_survives_replay_and_successor_before_valid_next() {
    let target_root = TestRoot::new("expired-ordering-successor");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let first_session = fabric_session("node-b", 4, 3);
    target_local
        .bind_gateway_session(&first_session)
        .expect("bind first target session");

    let mut first = operation(2, 3);
    first.id = "operation-expired-order-1".into();
    first.idempotency_key = "expired-order-1".into();
    first.ordering_key = "runtime:ordered-session".into();
    first.expires_at_unix_ms = 100;
    let first_attempt = RouteAttempt {
        id: "attempt-expired-order-1".into(),
        company_id: COMPANY.into(),
        operation_id: first.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: first_session.gateway_generation,
        control_plane_generation: first_session.control_plane_generation,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Sent,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 90,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let (tombstone, replayed) = target_local
        .consume_expired_ordering_tombstone(&first_session, &first, &first_attempt, 101)
        .expect("consume expired sequence one");
    assert!(!replayed);
    assert_eq!(tombstone.ordering_seq, 1);
    let (_, replayed) = target_local
        .consume_expired_ordering_tombstone(&first_session, &first, &first_attempt, 102)
        .expect("exact tombstone replay");
    assert!(replayed);

    let mut successor = first_session.clone();
    successor.gateway_generation += 1;
    successor.node_daemon_generation += 1;
    target_local
        .bind_gateway_session(&successor)
        .expect("bind successor after tombstone");
    let mut second = operation(2, 3);
    second.id = "operation-valid-order-2".into();
    second.idempotency_key = "valid-order-2".into();
    second.ordering_key = first.ordering_key.clone();
    let second_attempt = RouteAttempt {
        id: "attempt-valid-order-2".into(),
        company_id: COMPANY.into(),
        operation_id: second.id.clone(),
        attempt_no: 2,
        target_node_id: "node-b".into(),
        target_gateway_generation: successor.gateway_generation,
        control_plane_generation: successor.control_plane_generation,
        route_seq: 2,
        ordering_seq: 2,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 103,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    target_local
        .persist_inbox(&successor, &second, &second_attempt, 104)
        .expect("valid sequence two persists after replay and successor reconnect");
    let state = target_local.snapshot().expect("final target state");
    assert_eq!(state.ordering_tombstones.len(), 1);
    assert_eq!(state.inboxes.len(), 1);
    assert_eq!(state.persisted_ordering_sequences[&second.ordering_key], 2);
    assert!(!state.inboxes.contains_key(&first.id));
    assert!(!state.results.contains_key(&first.id));
}

#[test]
fn diagnostics_derive_connection_and_recovery_truth_without_mutation() {
    let root = TestRoot::new("diagnostics");
    let target_root = TestRoot::new("diagnostics-target");
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
    let request = operation(source.gateway_generation, lease.control_plane_generation);
    let request_digest = json_digest(&request).expect("request digest");
    let (_, attempt, _, _) = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        request.clone(),
        100,
    )
    .expect("route operation");
    target_local
        .persist_inbox(&target_session, &request, &attempt, 101)
        .expect("persist local target inbox");
    control
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
        .expect("persist target inbox");
    control
        .mark_unknown(lease.control_plane_generation, &request.id, 102)
        .expect("mark unknown after transport loss");
    let before = store.snapshot().expect("snapshot");
    let report = inspect_fabric(&store, COMPANY, 103).expect("read diagnostics");
    assert!(report.control_plane_online);
    assert_eq!(report.recovery_required_count, 1);
    assert_eq!(report.nodes.len(), 2);
    let target_report = report
        .nodes
        .iter()
        .find(|node| node.node_id == "node-b")
        .expect("target diagnostics");
    assert_eq!(
        target_report.connection_status,
        NodeConnectionStatus::Online
    );
    assert_eq!(target_report.recovery_required_operations, vec![request.id]);
    assert_eq!(target_report.last_persisted_route_seq, attempt.route_seq);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}

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
