#![allow(clippy::result_large_err)]

use firm_fabric::gateway_runtime::{
    now_unix_ms, serve_control_plane_session, NodeGatewayConnection, ProbeApplication,
};
use firm_fabric::transport::{
    accept_control_plane_mtls, ControlPlaneTlsFiles, NodeFabricConfig, NodeTlsIdentityFiles,
};
use firm_fabric::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

const COMPANY: &str = "company-process-test";
const NODE_A: &str = "11111111-1111-4111-8111-111111111111";
const NODE_B: &str = "22222222-2222-4222-8222-222222222222";
const CONTROL_INSTANCE: &str = "control-process-test";
const SCHEMA_DIGEST: &str = "process-schema-v1";

#[test]
#[ignore = "explicit three-process Remote Fabric acceptance"]
fn remote_fabric_three_process_acceptance() {
    if let Ok(role) = std::env::var("AGENTFIRM_FABRIC_CHILD_ROLE") {
        match role.as_str() {
            "control" => control_worker(),
            "node-a" => node_worker(NODE_A, NODE_B, true),
            "node-b" => node_worker(NODE_B, NODE_A, false),
            _ => panic!("unknown child role {role}"),
        }
        return;
    }
    parent_acceptance();
}

fn parent_acceptance() {
    let output = std::env::var_os("FABRIC_ACCEPTANCE_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "agentfirm-remote-fabric-process-{}",
                std::process::id()
            ))
        });
    fs::create_dir_all(&output).expect("create acceptance output");
    assert!(
        fs::read_dir(&output)
            .expect("read acceptance output")
            .next()
            .is_none(),
        "acceptance output must be an empty dedicated directory"
    );
    let runtime = output.join("runtime");
    fs::create_dir(&runtime).expect("create isolated secret runtime directory");
    let control_root = runtime.join("control-plane-store");
    let now = now_unix_ms().expect("clock");
    let store = FabricStore::open(&control_root).expect("Control Plane Store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, CONTROL_INSTANCE, &store, &keys, [9; 32]);
    let lease = control
        .acquire_lease("process-control-lease", 0, now)
        .expect("Control Plane lease");
    let ca = pki::generate_ca(COMPANY).expect("Company CA");
    let server = pki::issue_control_plane_server_certificate(&ca, "localhost", now)
        .expect("Control Plane server certificate");
    write_secret(&runtime.join("ca-key.pem"), &ca.private_key_pem);
    write_regular(&runtime.join("ca-cert.pem"), &ca.certificate_pem);
    write_secret(&runtime.join("server-key.pem"), &server.private_key_pem);
    write_regular(
        &runtime.join("server-cert.pem"),
        &server.certificate_chain_pem,
    );
    let host = AuthenticatedActor {
        company_id: COMPANY.into(),
        actor_id: "host-process".into(),
        actor_kind: ActorKind::Human,
        role_bindings: BTreeSet::from(["company_host".into()]),
        session_id: "host-process-session".into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
    };
    for (node_id, label) in [(NODE_A, "node-a"), (NODE_B, "node-b")] {
        let material = pki::generate_node_csr(COMPANY, node_id).expect("Node CSR");
        let certificate =
            pki::issue_node_certificate(&ca, &material.csr_pem, COMPANY, node_id, now)
                .expect("Node certificate");
        let token = format!("process-enrollment-token-{label}-0000000000000000");
        control
            .create_enrollment(
                &host,
                lease.control_plane_generation,
                &format!("enrollment-{label}"),
                &token,
                label,
                BTreeSet::from([
                    "durable-routing".into(),
                    "remote-runtime".into(),
                    "remote-message".into(),
                    "artifact-transfer".into(),
                ]),
                now + 60_000,
                now + 1,
            )
            .expect("create enrollment");
        control
            .consume_enrollment_csr(
                lease.control_plane_generation,
                &token,
                node_id,
                label,
                &material.csr_pem,
                &certificate.serial,
                certificate.expires_at_unix_ms,
                SCHEMA_DIGEST,
                now + 2,
            )
            .expect("consume enrollment");
        write_secret(
            &runtime.join(format!("{label}-key.pem")),
            &material.private_key_pem,
        );
        write_regular(
            &runtime.join(format!("{label}-cert.pem")),
            &format!("{}{}", certificate.certificate_pem, ca.certificate_pem),
        );
        write_regular(
            &runtime.join(format!("{label}-fingerprint")),
            &material.public_key_fingerprint,
        );
        write_regular(
            &runtime.join(format!("{label}-serial")),
            &certificate.serial,
        );
    }
    drop(control);
    drop(store);

    let test_binary = std::env::current_exe().expect("test binary");
    let mut control_child = spawn_child(&test_binary, "control", &output);
    let port_file = runtime.join("gateway-port");
    wait_for_file(&port_file, Duration::from_secs(10));
    let mut node_b = spawn_child(&test_binary, "node-b", &output);
    wait_for_file(&runtime.join("node-b-ready"), Duration::from_secs(10));
    let mut node_a = spawn_child(&test_binary, "node-a", &output);
    wait_success(&mut node_a, "Node A", Duration::from_secs(20));
    wait_success(&mut node_b, "Node B", Duration::from_secs(20));
    wait_success(&mut control_child, "Control Plane", Duration::from_secs(20));

    let recovered = FabricStore::open(&control_root).expect("reopen Control Plane Store");
    let state = recovered.snapshot().expect("final Fabric snapshot");
    let terminal = state
        .receipts
        .values()
        .find(|receipt| {
            receipt.operation_id == "process-probe-operation"
                && receipt.kind == ReceiptKind::OperationApplied
        })
        .expect("target application receipt");
    assert_eq!(terminal.application_effect, Some(EffectCertainty::Applied));
    write_json(&output.join("nodes.json"), &state.nodes);
    write_json(
        &output.join("control-plane-leases.json"),
        &state.control_plane_leases,
    );
    write_json(&output.join("gateway-leases.json"), &state.gateway_leases);
    write_json(&output.join("operations.json"), &state.operations);
    write_json(&output.join("attempts.json"), &state.attempts);
    write_json(&output.join("receipts.json"), &state.receipts);
    write_json(
        &output.join("reconcile.json"),
        &json!({
            "operation_id":"process-probe-operation",
            "source_terminal":fs::read_to_string(runtime.join("node-a-terminal")).unwrap(),
            "target_terminal":fs::read_to_string(runtime.join("node-b-terminal")).unwrap(),
            "blind_replay":false,
        }),
    );
    write_json(&output.join("artifact-manifests.json"), &state.artifacts);
    write_json(
        &output.join("port-scan.json"),
        &json!({
            "node_inbound_collaboration_listeners":[],
            "control_plane_gateway_listener":"outbound-node-connections-only",
        }),
    );
    write_json(
        &output.join("fabric-acceptance.json"),
        &json!({
            "ok":true,
            "processes":3,
            "submitted_revision":std::env::var("FABRIC_ACCEPTANCE_REVISION").unwrap_or_else(|_| "working-revision".into()),
            "company_id":COMPANY,
            "node_ids":[NODE_A,NODE_B],
            "control_plane_generation":lease.control_plane_generation,
            "gateway_generations":state.gateway_leases.values().map(|lease| json!({"node_id":lease.node_id,"gateway_generation":lease.gateway_generation,"node_daemon_generation":lease.node_daemon_generation})).collect::<Vec<_>>(),
            "operation_ids":["process-probe-operation"],
            "effect":"applied",
            "protocol_version":FABRIC_PROTOCOL_VERSION,
            "schema_version":FABRIC_SCHEMA_VERSION,
            "schema_bundle_digest":SCHEMA_DIGEST,
            "canonicalization_version":FABRIC_CANONICALIZATION_VERSION,
        }),
    );
    drop(recovered);
    assert_eq!(runtime.parent(), Some(output.as_path()));
    assert_eq!(
        runtime.file_name().and_then(|name| name.to_str()),
        Some("runtime")
    );
    assert!(!fs::symlink_metadata(&runtime)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_dir_all(&runtime).expect("remove isolated runtime secrets after acceptance");
    println!(
        "three-process Remote Fabric acceptance: {}",
        output.display()
    );
}

fn control_worker() {
    let root = child_root();
    let store = FabricStore::open(root.join("control-plane-store")).expect("Control Plane Store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let generation =
        store.snapshot().unwrap().control_plane_leases[COMPANY].control_plane_generation;
    let tls = ControlPlaneTlsFiles {
        server_certificate_chain_pem: root.join("server-cert.pem"),
        server_private_key_pem: root.join("server-key.pem"),
        node_ca_pem: root.join("ca-cert.pem"),
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("gateway listener");
    listener.set_nonblocking(true).unwrap();
    write_regular(
        &root.join("gateway-port"),
        &listener.local_addr().unwrap().port().to_string(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut sessions = Vec::new();
    while sessions.len() < 2 && Instant::now() < deadline {
        match listener.accept() {
            Ok((tcp, _)) => {
                tcp.set_nonblocking(false)
                    .expect("restore blocking mode for TLS/WebSocket handshake");
                let tls = tls.clone();
                let root = root.clone();
                sessions.push(thread::spawn(move || {
                    let store = FabricStore::open(root.join("control-plane-store")).unwrap();
                    let keys = InMemoryArtifactKeyBackend::default();
                    keys.insert(COMPANY, [7; 32]);
                    let control =
                        ControlPlane::new(COMPANY, CONTROL_INSTANCE, &store, &keys, [9; 32]);
                    let (mut socket, peer) = accept_control_plane_mtls(tcp, &tls).unwrap();
                    serve_control_plane_session(&mut socket, &peer, &control, generation).unwrap();
                }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("gateway accept failed: {error}"),
        }
    }
    assert_eq!(sessions.len(), 2, "both Node processes connected");
    for session in sessions {
        session.join().expect("gateway session");
    }
}

fn node_worker(node_id: &str, target_node_id: &str, source: bool) {
    let root = child_root();
    let label = if source { "node-a" } else { "node-b" };
    let port = fs::read_to_string(root.join("gateway-port"))
        .unwrap()
        .trim()
        .parse::<u16>()
        .unwrap();
    let local = NodeLocalFabricStore::open(root.join(format!("{label}-store")), COMPANY, node_id)
        .expect("Node-local Store");
    let hello = NodeHello {
        company_id: COMPANY.into(),
        node_id: node_id.into(),
        instance_id: format!("gateway-{label}"),
        node_daemon_id: format!("daemon-{label}"),
        node_daemon_generation: 1,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: SCHEMA_DIGEST.into(),
        features: BTreeSet::from([
            "durable-routing".into(),
            "remote-runtime".into(),
            "remote-message".into(),
            "artifact-transfer".into(),
        ]),
        build_sha: "process-test".into(),
        last_persisted_route_seq: 0,
        unresolved_operation_ids: BTreeSet::new(),
        certificate_serial: fs::read_to_string(root.join(format!("{label}-serial")))
            .unwrap()
            .trim()
            .into(),
        public_key_fingerprint: fs::read_to_string(root.join(format!("{label}-fingerprint")))
            .unwrap()
            .trim()
            .into(),
    };
    let config = NodeFabricConfig {
        company_id: COMPANY.into(),
        node_id: node_id.into(),
        control_plane_url: format!("wss://localhost:{port}/v1/node-gateway/connect"),
        reconnect_floor_ms: 10,
        reconnect_ceiling_ms: 100,
    };
    let tls = NodeTlsIdentityFiles {
        client_certificate_chain_pem: root.join(format!("{label}-cert.pem")),
        client_private_key_pem: root.join(format!("{label}-key.pem")),
        control_plane_ca_pem: root.join("ca-cert.pem"),
    };
    let mut gateway =
        NodeGatewayConnection::connect(&config, &tls, hello).expect("connect gateway");
    gateway
        .set_read_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    local.bind_gateway_session(&gateway.session).unwrap();
    write_regular(&root.join(format!("{label}-ready")), "ready");
    if source {
        let body = json!({"probe":"three-process"});
        let actor = AuthenticatedActor {
            company_id: COMPANY.into(),
            actor_id: node_id.into(),
            actor_kind: ActorKind::Service,
            role_bindings: BTreeSet::from(["fabric_submit".into()]),
            session_id: "source-node-daemon".into(),
            issued_at_unix_ms: now_unix_ms().unwrap(),
            expires_at_unix_ms: now_unix_ms().unwrap() + 30_000,
        };
        let operation = RoutedOperation {
            id: "process-probe-operation".into(),
            company_id: COMPANY.into(),
            kind: PROBE_OPERATION_KIND.into(),
            source_authority: OperationSourceAuthority::Node,
            source_node_id: Some(node_id.into()),
            target_node_id: target_node_id.into(),
            source_gateway_generation: Some(gateway.session.gateway_generation),
            source_node_daemon_id: Some(gateway.session.node_daemon_id.clone()),
            source_node_daemon_generation: Some(gateway.session.node_daemon_generation),
            control_plane_generation: gateway.session.control_plane_generation,
            source_execution_space_id: None,
            target_execution_space_id: None,
            actor: actor.clone(),
            actor_runtime_generation: None,
            authorization_context: BTreeMap::from([(
                "capability".into(),
                "durable-routing".into(),
            )]),
            idempotency_key: "process-probe-key".into(),
            ordering_key: "process-probe".into(),
            correlation_id: "process-probe-correlation".into(),
            causation_id: None,
            expected_target_revision: None,
            body_schema: PROBE_BODY_SCHEMA.into(),
            body_digest: json_digest(&body).unwrap(),
            body,
            priority: OperationPriority::Normal,
            created_at_unix_ms: now_unix_ms().unwrap(),
            expires_at_unix_ms: now_unix_ms().unwrap() + 30_000,
            protocol_version: FABRIC_PROTOCOL_VERSION,
            schema_version: FABRIC_SCHEMA_VERSION.into(),
            canonicalization_version: FABRIC_CANONICALIZATION_VERSION.into(),
        };
        gateway
            .submit_operation(&local, &actor, operation)
            .expect("submit source operation");
        for _ in 0..100 {
            let receipts = gateway
                .reconcile_operations(&local, BTreeSet::from(["process-probe-operation".into()]))
                .expect("source reconcile");
            if receipts.iter().any(|receipt| {
                receipt.kind == ReceiptKind::OperationApplied
                    && receipt.application_effect == Some(EffectCertainty::Applied)
            }) {
                write_regular(&root.join("node-a-terminal"), "operation_applied");
                gateway.close().unwrap();
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("source did not observe terminal reconciliation");
    } else {
        let mut application = ProbeApplication;
        for _ in 0..100 {
            gateway.heartbeat().expect("target heartbeat");
            match gateway.apply_next(&local, &mut application) {
                Ok(receipt) if receipt.kind == ReceiptKind::OperationApplied => {
                    write_regular(&root.join("node-b-terminal"), "operation_applied");
                    gateway.close().unwrap();
                    return;
                }
                Ok(_) => {}
                Err(error) if error.code == FabricErrorCode::TargetOffline && error.retryable => {}
                Err(error) => panic!("target apply failed: {error}"),
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("target did not apply routed operation");
    }
}

fn spawn_child(binary: &Path, role: &str, root: &Path) -> Child {
    Command::new(binary)
        .arg("--exact")
        .arg("remote_fabric_three_process_acceptance")
        .arg("--ignored")
        .arg("--nocapture")
        .env("AGENTFIRM_FABRIC_CHILD_ROLE", role)
        .env("FABRIC_ACCEPTANCE_OUTPUT", root)
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {role}: {error}"))
}

fn wait_success(child: &mut Child, label: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            assert!(status.success(), "{label} exited with {status}");
            return;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            panic!("{label} timed out");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_file(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn child_root() -> PathBuf {
    PathBuf::from(std::env::var_os("FABRIC_ACCEPTANCE_OUTPUT").expect("acceptance root"))
        .join("runtime")
}

fn write_secret(path: &Path, value: &str) {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .expect("create secret file");
    file.write_all(value.as_bytes()).expect("write secret");
    file.sync_all().expect("sync secret");
}

fn write_regular(path: &Path, value: &str) {
    fs::write(path, value).expect("write acceptance file");
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).expect("write JSON evidence");
}
