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
        "command_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "target_execution_space_id": "space-b",
        "target_node_daemon_id": "node-daemon:node-b",
        "target_node_daemon_generation": 4
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
    let message_body_digest = format!(
        "sha256:{}",
        json_digest(&json!({"body": "hello"})).expect("message body digest")
    );
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
        "command_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "target_execution_space_id": "space-b",
        "target_node_daemon_id": "node-daemon:node-b",
        "target_node_daemon_generation": 1
    });
    unsupported.body_digest = json_digest(&unsupported.body).expect("runtime reference digest");
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
        .persist_inbox(&target_session, &request, &attempt)
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
        .claim_inbox(&target_session, &request.id)
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
fn node_local_journal_recovers_lost_ack_without_duplicate_native_effect() {
    let source_root = TestRoot::new("local-source-recovery");
    let request = operation(1, 1);
    let source_session = fabric_session("node-a", 1, 1);
    let source =
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a").expect("source store");
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
        .persist_inbox(&target_session, &request, &attempt)
        .expect("persist target inbox");
    target
        .claim_inbox(&target_session, &request.id)
        .expect("claim before native effect");
    assert_eq!(
        target.unresolved_operation_ids().expect("unresolved ids"),
        BTreeSet::from([request.id.clone()])
    );
    let duplicate_claim = target
        .claim_inbox(&target_session, &request.id)
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
            .persist_inbox(&hostile, &request, &attempt)
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
        .persist_inbox(&fabric_session("node-b", 9, 3), &hostile_body, &attempt)
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
    let successor_hello = hello("node-b", "gateway-b-2", "cert-b", &fingerprint("node-b"));
    let successor = connect_node(
        &control,
        lease.control_plane_generation,
        &successor_hello,
        &signing_key("node-b"),
        30_031,
    )
    .expect("target successor");
    let successor_session = fabric_session(
        "node-b",
        successor.gateway_generation,
        lease.control_plane_generation,
    );
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
        .persist_inbox(&successor_session, &request, &second_attempt)
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
        .claim_inbox(&successor_session, &request.id)
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
        .persist_inbox(&target_session, &first, &attempt)
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
        .claim_inbox(&target_session, &first.id)
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
        .persist_inbox(&target_session, &second, &second_attempt)
        .expect_err("seq=2 cannot pass unresolved seq=1");
    assert_eq!(gap.code, FabricErrorCode::ExpectedRevisionConflict);
    assert_eq!(target_local.snapshot().expect("snapshot"), before);
    target_local
        .persist_inbox(&target_session, &first, &first_attempt)
        .expect("persist seq=1");
    target_local
        .persist_inbox(&target_session, &second, &second_attempt)
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
        .persist_inbox(&target_session, &request, &attempt)
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
        .mark_outbox_receipt(&accepted)
        .expect("source records Control Plane acceptance");
    let (target_inbox, replayed) = target_local
        .persist_inbox(&target_session, &request, &attempt)
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
        .mark_outbox_receipt(&persisted)
        .expect("source records target persistence");
    target_local
        .claim_inbox(&target_session, &request.id)
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
        .mark_outbox_receipt(&terminal)
        .expect("source converges terminal outbox");
    assert_eq!(terminal_outbox.local_state, LocalOutboxState::Terminal);

    let control_state = store.snapshot().expect("Control Plane snapshot");
    assert_eq!(control_state.operations.len(), 1);
    assert_eq!(control_state.receipts.len(), 3);
    let source_state = source_local.snapshot().expect("source snapshot");
    assert_eq!(source_state.outboxes.len(), 1);
    assert!(source_state.inboxes.is_empty());
    let target_state = target_local.snapshot().expect("target snapshot");
    assert_eq!(target_state.inboxes.len(), 1);
    assert_eq!(target_state.results.len(), 1);
    assert!(target_state.outboxes.is_empty());
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
