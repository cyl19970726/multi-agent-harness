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

/// Hard deadline for one fixture phase (readiness handshake, source
/// submit/reconcile, target heartbeat/apply). A phase fails only after the
/// whole budget elapses without protocol progress. Machine load stretches
/// individual frame latencies; it cannot keep a healthy Control Plane silent
/// for the entire budget, so the verdict cannot flip under load.
const PHASE_BUDGET: Duration = Duration::from_secs(30);
/// Upper bound for one blocking frame read inside a phase. The slice only
/// paces how often the phase re-checks PHASE_BUDGET; it never decides the
/// verdict by itself.
const READ_SLICE: Duration = Duration::from_secs(2);
/// Parent-side hard deadline for the whole three-process run.
const RUN_BUDGET: Duration = Duration::from_secs(120);
fn schema_digest() -> String {
    sha256_hex(include_bytes!(
        "../../../schemas/remote-fabric/schema-bundle.v1.json"
    ))
}

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
                &schema_digest(),
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
    wait_for_file(&port_file, PHASE_BUDGET);
    let mut node_b = spawn_child(&test_binary, "node-b", &output);
    wait_for_file(&runtime.join("node-b-ready"), PHASE_BUDGET);
    let mut node_a = spawn_child(&test_binary, "node-a", &output);
    wait_for_file(&runtime.join("node-a-ready"), PHASE_BUDGET);
    let control_listeners = inspect_tcp_listeners(control_child.id());
    let node_a_listeners = inspect_tcp_listeners(node_a.id());
    let node_b_listeners = inspect_tcp_listeners(node_b.id());
    let gateway_port = fs::read_to_string(&port_file)
        .expect("gateway port")
        .trim()
        .to_string();
    assert!(
        control_listeners
            .iter()
            .any(|line| line.contains(&format!("127.0.0.1:{gateway_port}"))),
        "Control Plane process did not own the expected loopback gateway listener: {control_listeners:?}"
    );
    assert!(
        node_a_listeners.is_empty() && node_b_listeners.is_empty(),
        "outbound-only Node processes must own no TCP listeners: node-a={node_a_listeners:?} node-b={node_b_listeners:?}"
    );
    write_regular(&runtime.join("listener-inspection-complete"), "continue");
    wait_children_first_failure(
        &mut [
            ("Node A", &mut node_a),
            ("Node B", &mut node_b),
            ("Control Plane", &mut control_child),
        ],
        &runtime,
    );

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
            "inspection":"lsof-process-owned-tcp-listeners",
            "control_plane_pid":control_child.id(),
            "control_plane_gateway_listeners":control_listeners,
            "node_a_pid":node_a.id(),
            "node_a_inbound_collaboration_listeners":node_a_listeners,
            "node_b_pid":node_b.id(),
            "node_b_inbound_collaboration_listeners":node_b_listeners,
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
            "schema_bundle_digest":schema_digest(),
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
    let deadline = Instant::now() + PHASE_BUDGET;
    let mut sessions = Vec::new();
    while sessions.len() < 2 && Instant::now() < deadline {
        match listener.accept() {
            Ok((tcp, _)) => {
                tcp.set_nonblocking(false)
                    .expect("restore blocking mode for TLS/WebSocket handshake");
                let tls = tls.clone();
                let root = root.clone();
                sessions.push(thread::spawn(move || -> Result<(), FabricError> {
                    let store = FabricStore::open(root.join("control-plane-store"))?;
                    let keys = InMemoryArtifactKeyBackend::default();
                    keys.insert(COMPANY, [7; 32]);
                    let control =
                        ControlPlane::new(COMPANY, CONTROL_INSTANCE, &store, &keys, [9; 32]);
                    let (mut socket, peer) = accept_control_plane_mtls(tcp, &tls)?;
                    serve_control_plane_session(&mut socket, &peer, &control, generation)
                }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("gateway accept failed: {error}"),
        }
    }
    assert_eq!(sessions.len(), 2, "both Node processes connected");

    // Readiness handshake: the Nodes may not issue their first
    // heartbeat/reconcile until the Control Plane durably serves both gateway
    // sessions. Without this a slow accept/thread-schedule on a loaded runner
    // made the first frame read race session setup.
    let ready_started = Instant::now();
    loop {
        let snapshot = store.snapshot().expect("Control Plane snapshot");
        if snapshot.gateway_leases.len() == 2 {
            break;
        }
        assert!(
            ready_started.elapsed() < PHASE_BUDGET,
            "phase 'control readiness handshake' exceeded its hard deadline: {:?} elapsed with {} of 2 gateway sessions established",
            ready_started.elapsed(),
            snapshot.gateway_leases.len()
        );
        thread::sleep(Duration::from_millis(20));
    }
    write_regular(&root.join("gateway-sessions-ready"), "ready");

    for (index, session) in sessions.into_iter().enumerate() {
        match session.join().expect("gateway session thread") {
            Ok(()) => {}
            // A Node that already failed takes its connection down with it;
            // the resulting Broken pipe/reset is a consequence of that first
            // failure, not a new one — report it and let the parent name the
            // Node's own recorded failure instead of a cascaded unwrap.
            Err(error) if is_peer_vanished(&error) => {
                eprintln!(
                    "gateway session {index} ended after its peer vanished: {}",
                    error.message
                );
            }
            Err(error) => {
                record_failure(
                    &root,
                    "control",
                    &format!("gateway session {index} failed: {}", error.message),
                );
                panic!("gateway session {index} failed: {}", error.message);
            }
        }
    }
}

/// True when the error only reports that the peer process is already gone
/// (orderly close, reset, or a write into a closed connection), as opposed to
/// a protocol or authority violation inside this session.
fn is_peer_vanished(error: &FabricError) -> bool {
    if error.code == FabricErrorCode::TargetOffline {
        return true;
    }
    let message = error.message.to_lowercase();
    [
        "broken pipe",
        "connection reset",
        "close_notify",
        "peer closed",
        "unexpected eof",
        "early eof",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// The one frame-read timeout a loaded runner can legitimately produce; the
/// connection authority is unchanged, so the phase retries it within its own
/// deadline instead of failing.
fn is_frame_read_timeout(error: &FabricError) -> bool {
    error.code == FabricErrorCode::TargetOffline
        && error.retryable
        && error.message == "Fabric frame read timed out without changing connection authority"
}

fn record_failure(root: &Path, label: &str, message: &str) {
    let stamp = now_unix_ms()
        .map(|ms| ms.to_string())
        .unwrap_or_else(|_| "0".into());
    write_regular(
        &root.join(format!("{label}-failure")),
        &format!("{stamp} {label}: {message}"),
    );
}

fn node_worker(node_id: &str, target_node_id: &str, source: bool) {
    let label = if source { "node-a" } else { "node-b" };
    if let Err(error) = node_worker_inner(node_id, target_node_id, source) {
        record_failure(&child_root(), label, &error);
        panic!("{error}");
    }
}

/// Remaining read budget for one blocking frame inside a phase: the smaller of
/// READ_SLICE and the time left before the phase deadline, so a slow frame can
/// only consume the phase budget, never exceed it.
fn read_slice(phase_started: Instant) -> Duration {
    let remaining = PHASE_BUDGET.saturating_sub(phase_started.elapsed());
    READ_SLICE.min(remaining).max(Duration::from_millis(100))
}

fn node_worker_inner(node_id: &str, target_node_id: &str, source: bool) -> Result<(), String> {
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
        node_daemon_id: format!("node-daemon:{node_id}"),
        node_daemon_generation: 1,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: schema_digest(),
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
    let mut gateway = NodeGatewayConnection::connect(&config, &tls, hello)
        .map_err(|error| format!("phase 'gateway connect' failed: {error}"))?;
    gateway
        .set_read_timeout(Some(READ_SLICE))
        .map_err(|error| format!("phase 'gateway connect' failed: {error}"))?;
    local
        .bind_gateway_session(&gateway.session)
        .map_err(|error| format!("phase 'gateway connect' failed: {error}"))?;
    write_regular(&root.join(format!("{label}-ready")), "ready");
    // Readiness handshake: both gateway sessions are durably served before
    // the first heartbeat/reconcile, and the parent completed its listener
    // inspection before the operation phase.
    wait_for_file(&root.join("gateway-sessions-ready"), PHASE_BUDGET);
    wait_for_file(&root.join("listener-inspection-complete"), PHASE_BUDGET);
    if source {
        source_operation_phase(gateway, &local, &root, node_id, target_node_id)
    } else {
        target_apply_phase(gateway, &local, &root)
    }
}

fn source_operation_phase(
    mut gateway: NodeGatewayConnection,
    local: &NodeLocalFabricStore,
    root: &Path,
    node_id: &str,
    target_node_id: &str,
) -> Result<(), String> {
    let phase_started = Instant::now();
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
        authorization_context: BTreeMap::from([("capability".into(), "durable-routing".into())]),
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
    // The idempotency key makes a resubmission after a frame-read timeout
    // return the same acceptance, so retrying the submit within the phase
    // budget is exact rather than a second operation.
    loop {
        gateway
            .set_read_timeout(Some(read_slice(phase_started)))
            .map_err(|error| format!("phase 'source submit' failed: {error}"))?;
        match gateway.submit_operation(local, &actor, operation.clone()) {
            Ok(_) => break,
            Err(error) if is_frame_read_timeout(&error) => {}
            Err(error) => {
                return Err(format!(
                    "phase 'source submit' failed after {:?}: {error}",
                    phase_started.elapsed()
                ));
            }
        }
        if phase_started.elapsed() >= PHASE_BUDGET {
            return Err(format!(
                "phase 'source submit' exceeded its hard deadline: {:?} elapsed",
                phase_started.elapsed()
            ));
        }
    }
    loop {
        gateway
            .set_read_timeout(Some(read_slice(phase_started)))
            .map_err(|error| format!("phase 'source reconcile' failed: {error}"))?;
        match gateway
            .reconcile_operations(local, BTreeSet::from(["process-probe-operation".into()]))
        {
            Ok(receipts) => {
                if receipts.iter().any(|receipt| {
                    receipt.kind == ReceiptKind::OperationApplied
                        && receipt.application_effect == Some(EffectCertainty::Applied)
                }) {
                    write_regular(&root.join("node-a-terminal"), "operation_applied");
                    close_after_terminal(gateway, "node-a");
                    return Ok(());
                }
            }
            Err(error) if is_frame_read_timeout(&error) => {}
            Err(error) => {
                return Err(format!(
                    "phase 'source reconcile' failed after {:?}: {error}",
                    phase_started.elapsed()
                ));
            }
        }
        if phase_started.elapsed() >= PHASE_BUDGET {
            return Err(format!(
                "phase 'source reconcile' exceeded its hard deadline: {:?} elapsed without a terminal applied receipt",
                phase_started.elapsed()
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn target_apply_phase(
    mut gateway: NodeGatewayConnection,
    local: &NodeLocalFabricStore,
    root: &Path,
) -> Result<(), String> {
    let phase_started = Instant::now();
    let mut application = ProbeApplication;
    loop {
        gateway
            .set_read_timeout(Some(read_slice(phase_started)))
            .map_err(|error| format!("phase 'target heartbeat' failed: {error}"))?;
        match gateway.heartbeat() {
            Ok(()) => {}
            Err(error) if is_frame_read_timeout(&error) => {}
            Err(error) => {
                return Err(format!(
                    "phase 'target heartbeat' failed after {:?}: {error}",
                    phase_started.elapsed()
                ));
            }
        }
        gateway
            .set_read_timeout(Some(read_slice(phase_started)))
            .map_err(|error| format!("phase 'target apply' failed: {error}"))?;
        match gateway.apply_next(local, &mut application) {
            Ok(receipt) if receipt.kind == ReceiptKind::OperationApplied => {
                write_regular(&root.join("node-b-terminal"), "operation_applied");
                close_after_terminal(gateway, "node-b");
                return Ok(());
            }
            Ok(_) => {}
            Err(error) if is_frame_read_timeout(&error) => {}
            Err(error)
                if error.code == FabricErrorCode::TargetOffline
                    && error.message == "pending delivery batch is complete" => {}
            Err(error) => {
                return Err(format!(
                    "phase 'target apply' failed after {:?}: {error}",
                    phase_started.elapsed()
                ));
            }
        }
        if phase_started.elapsed() >= PHASE_BUDGET {
            return Err(format!(
                "phase 'target apply' exceeded its hard deadline: {:?} elapsed without applying the routed operation",
                phase_started.elapsed()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// The terminal receipt is already durable at this point; a close handshake
/// error from an already-torn-down connection is teardown noise, not evidence.
fn close_after_terminal(gateway: NodeGatewayConnection, label: &str) {
    if let Err(error) = gateway.close() {
        eprintln!(
            "{label} gateway close after terminal receipt: {}",
            error.message
        );
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

/// Reap the three child processes, reporting the FIRST recorded failure. Each
/// child writes a `{label}-failure` file (timestamped) before it panics, so a
/// cascaded exit — e.g. the Control Plane's session thread noticing a Node
/// that already died — never masks the root cause.
fn wait_children_first_failure(children: &mut [(&str, &mut Child)], runtime: &Path) {
    let started = Instant::now();
    let mut pending: BTreeSet<usize> = (0..children.len()).collect();
    loop {
        let mut exited = Vec::new();
        for &index in &pending {
            if let Some(status) = children[index].1.try_wait().expect("poll child") {
                exited.push((index, status));
            }
        }
        for (index, status) in exited {
            pending.remove(&index);
            if !status.success() {
                for (other_index, (_, child)) in children.iter_mut().enumerate() {
                    if pending.contains(&other_index) {
                        child.kill().ok();
                        child.wait().ok();
                    }
                }
                panic!(
                    "{} exited with {status}; first recorded failure: {}",
                    children[index].0,
                    first_recorded_failure(runtime)
                );
            }
        }
        if pending.is_empty() {
            return;
        }
        assert!(
            started.elapsed() < RUN_BUDGET,
            "phase 'three-process run' exceeded its hard deadline: {:?} elapsed with {} child processes still running",
            started.elapsed(),
            pending.len()
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn first_recorded_failure(runtime: &Path) -> String {
    let mut failures: Vec<(u128, String)> = fs::read_dir(runtime)
        .expect("read runtime directory for failure records")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with("-failure"))
        .filter_map(|entry| {
            let content = fs::read_to_string(entry.path()).ok()?;
            let (stamp, _) = content.split_once(' ')?;
            Some((stamp.parse::<u128>().ok()?, content))
        })
        .collect();
    failures.sort_by_key(|(stamp, _)| *stamp);
    failures
        .into_iter()
        .next()
        .map(|(_, content)| content)
        .unwrap_or_else(|| "none (child exited without recording a failure)".into())
}

fn wait_for_file(path: &Path, timeout: Duration) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < timeout,
            "phase 'wait for {}' exceeded its hard deadline: {:?} elapsed",
            path.display(),
            started.elapsed()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn inspect_tcp_listeners(pid: u32) -> Vec<String> {
    let output = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:LISTEN"])
        .output()
        .expect("lsof is required for real listener acceptance evidence");
    assert!(
        output.status.success() || output.status.code() == Some(1),
        "lsof listener inspection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .map(str::to_string)
        .collect()
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
