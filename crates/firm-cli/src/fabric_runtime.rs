//! Process entrypoints for the Company Control Plane and outbound NodeGateway.

#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use harness_fabric::gateway_runtime::{
    now_unix_ms, serve_control_plane_session, NodeApplication, NodeGatewayConnection,
    ProbeApplication,
};
use harness_fabric::transport::{
    accept_control_plane_mtls, ControlPlaneTlsFiles, NodeFabricConfig, NodeTlsIdentityFiles,
    NodeTlsIdentityMaterial,
};
use harness_fabric::{
    ArtifactClassification, AuthenticatedActor, ControlPlane, FabricError, FabricErrorCode,
    InMemoryArtifactKeyBackend, NodeAdministrativeStatus, NodeHello, FABRIC_PROTOCOL_VERSION,
};
use harness_store::remote_fabric_store::RemoteFabricStoreLayout;
use harness_store::HarnessStore;

use super::{CliError, CliResult, ResolvedStore};

const GATEWAY_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn fabric_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(CliError::Usage(
            "fabric requires control-plane|node-gateway".into(),
        ));
    };
    match command {
        "control-plane" => control_plane_command(resolved, &args[1..]),
        "node-gateway" => node_gateway_command(store, resolved, &args[1..]),
        "route" => route_command(store, resolved, &args[1..]),
        other => Err(CliError::Usage(format!("unknown fabric command: {other}"))),
    }
}

fn route_command(
    wave4c_store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    if args.first().map(String::as_str) != Some("queue") {
        return Err(CliError::Usage(
            "fabric route queue --company <id> --target-node <uuid> --target-space <id> --kind probe|runtime|message --body-file <path> --operation-id <id> --idempotency-key <key> --ordering-key <key> [--source-space <id>]".into(),
        ));
    }
    let company_id = required(args, "--company")?;
    let target_node_id = required(args, "--target-node")?;
    let target_execution_space_id = required(args, "--target-space")?;
    let operation_id = required(args, "--operation-id")?;
    let idempotency_key = required(args, "--idempotency-key")?;
    let ordering_key = required(args, "--ordering-key")?;
    let kind = required(args, "--kind")?;
    let body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(required_path(args, "--body-file")?)?)?;
    let node_id = super::read_local_node_id()?;
    if target_node_id == node_id {
        return Err(CliError::Usage(
            "Remote Fabric route target must be a distinct ExecutionNode".into(),
        ));
    }
    let now = now_unix_ms().map_err(fabric_error)?;
    let firm_home = firm_home(resolved, args)?;
    let layout = RemoteFabricStoreLayout::open(&firm_home).map_err(fabric_error)?;
    let local = layout
        .open_node_local(&company_id, &node_id)
        .map_err(fabric_error)?;
    let session = local
        .active_session()
        .map_err(fabric_error)?
        .ok_or_else(|| {
            CliError::Usage(
                "NodeGateway has no durable active session; start the current NodeGateway first"
                    .into(),
            )
        })?;
    let lease = wave4c_store
        .latest_node_daemon_lease(&node_id)?
        .filter(|lease| {
            lease.status == harness_core::NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > now
                && lease.daemon_id == session.node_daemon_id
                && lease.generation == session.node_daemon_generation
        })
        .ok_or_else(|| {
            CliError::Usage(
                "NodeGateway session is not a child of the exact current NodeDaemonLease".into(),
            )
        })?;
    let (
        wire_kind,
        body_schema,
        source_execution_space_id,
        expected_target_revision,
        priority,
        expires_at_unix_ms,
    ) = match kind.as_str() {
        "probe" => {
            let probe: harness_fabric::FabricProbeBody = serde_json::from_value(body.clone())?;
            if probe.probe.trim().is_empty() {
                return Err(CliError::Usage(
                    "remote probe must contain a bounded non-empty probe label".into(),
                ));
            }
            (
                harness_fabric::PROBE_OPERATION_KIND,
                harness_fabric::PROBE_BODY_SCHEMA,
                value(args, "--source-space"),
                None,
                harness_fabric::OperationPriority::Control,
                now.saturating_add(5 * 60_000),
            )
        }
        "runtime" => {
            let reference: harness_fabric::RuntimeCommandReference =
                serde_json::from_value(body.clone())?;
            let envelope: harness_core::agentfirm_api::ControlCommandEnvelope =
                serde_json::from_value(reference.canonical_command_envelope.clone())?;
            if envelope.target_node_id != target_node_id
                || envelope.execution_space_id != target_execution_space_id
                || envelope.authenticated_actor.kind
                    != harness_core::agentfirm_api::ActorKind::Service
                || envelope.authenticated_actor.id != node_id
            {
                return Err(CliError::Usage(
                        "remote RuntimeCommand must bind the exact target and current source Node service authority"
                            .into(),
                    ));
            }
            (
                harness_fabric::RUNTIME_COMMAND_REFERENCE_KIND,
                harness_fabric::RUNTIME_COMMAND_REFERENCE_SCHEMA,
                value(args, "--source-space"),
                Some(envelope.expected_version),
                harness_fabric::OperationPriority::Control,
                envelope.expires_unix_ms,
            )
        }
        "message" => {
            let reference: harness_fabric::MessageReference = serde_json::from_value(body.clone())?;
            let envelope = reference
                .canonical_message_envelope
                .as_ref()
                .ok_or_else(|| {
                    CliError::Usage(
                        "route queue currently requires an embedded canonical Message envelope"
                            .into(),
                    )
                })?;
            let message: harness_core::agentfirm_api::Message =
                serde_json::from_value(envelope.clone())?;
            if !wave4c_store
                .fabric_messages(&message.source_execution_space_id)?
                .iter()
                .any(|stored| stored == &message)
            {
                return Err(CliError::Usage(
                        "remote Message must already exist as the exact immutable source-authored Message"
                            .into(),
                    ));
            }
            (
                harness_fabric::MESSAGE_REFERENCE_KIND,
                harness_fabric::MESSAGE_REFERENCE_SCHEMA,
                Some(message.source_execution_space_id),
                Some(0),
                harness_fabric::OperationPriority::Normal,
                now.saturating_add(5 * 60_000),
            )
        }
        _ => {
            return Err(CliError::Usage(
                "--kind must be probe|runtime|message; arbitrary transport mutations are closed"
                    .into(),
            ))
        }
    };
    let actor = AuthenticatedActor {
        company_id: company_id.clone(),
        actor_id: node_id.clone(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: std::collections::BTreeSet::from(["fabric_submit".into()]),
        session_id: format!("node-daemon:{}:{}", lease.daemon_id, lease.generation),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now.saturating_add(30_000),
    };
    let operation = harness_fabric::RoutedOperation {
        id: operation_id.clone(),
        company_id,
        kind: wire_kind.into(),
        source_authority: harness_fabric::OperationSourceAuthority::Node,
        source_node_id: Some(node_id),
        target_node_id,
        source_gateway_generation: Some(session.gateway_generation),
        source_node_daemon_id: Some(lease.daemon_id),
        source_node_daemon_generation: Some(lease.generation),
        control_plane_generation: session.control_plane_generation,
        source_execution_space_id,
        target_execution_space_id: Some(target_execution_space_id),
        actor: actor.clone(),
        actor_runtime_generation: None,
        authorization_context: std::collections::BTreeMap::from([(
            "capability".into(),
            match kind.as_str() {
                "probe" => "remote-probe",
                "runtime" => "remote-runtime",
                "message" => "remote-message",
                _ => unreachable!(),
            }
            .into(),
        )]),
        idempotency_key,
        ordering_key,
        correlation_id: format!("route:{operation_id}"),
        causation_id: None,
        expected_target_revision,
        body_schema: body_schema.into(),
        body_digest: harness_fabric::json_digest(&body).map_err(fabric_error)?,
        body,
        priority,
        created_at_unix_ms: now,
        expires_at_unix_ms,
        protocol_version: harness_fabric::FABRIC_PROTOCOL_VERSION,
        schema_version: harness_fabric::FABRIC_SCHEMA_VERSION.into(),
        canonicalization_version: harness_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
    };
    operation.closed_body().map_err(fabric_error)?;
    if kind == "runtime" {
        let envelope = super::remote_fabric::resolved_runtime_command_from_operation(&operation)
            .map_err(fabric_error)?;
        if envelope.expires_unix_ms <= now {
            return Err(CliError::Usage(
                "remote RuntimeCommand expired before local durable queueing".into(),
            ));
        }
    } else if kind == "message" {
        super::remote_fabric::resolved_message_from_operation(&operation).map_err(fabric_error)?;
    }
    let (queued, replayed) = local
        .prepare_outbox(&session, &actor, &operation, now)
        .map_err(fabric_error)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "queued_operation": queued,
            "replayed": replayed,
        }))?
    );
    Ok(())
}

fn control_plane_command(resolved: &ResolvedStore, args: &[String]) -> CliResult<()> {
    match args.first().map(String::as_str) {
        Some("backup") => {
            let company_id = required(args, "--company")?;
            let output = required_path(args, "--output")?;
            let layout =
                RemoteFabricStoreLayout::open(firm_home(resolved, args)?).map_err(fabric_error)?;
            let manifest = layout
                .open_control_plane(&company_id)
                .map_err(fabric_error)?
                .create_backup(&output)
                .map_err(fabric_error)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            return Ok(());
        }
        Some("restore") => {
            let company_id = required(args, "--company")?;
            let backup = required_path(args, "--backup")?;
            let layout =
                RemoteFabricStoreLayout::open(firm_home(resolved, args)?).map_err(fabric_error)?;
            let target = layout
                .control_plane_root(&company_id)
                .map_err(fabric_error)?;
            let manifest = harness_fabric::FabricStore::restore_backup(&backup, &target)
                .map_err(fabric_error)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            return Ok(());
        }
        Some("serve") => {}
        _ => {
            return Err(CliError::Usage(
                "fabric control-plane requires serve|backup|restore; backup --company <id> --output <new-dir>; restore --company <id> --backup <dir> [--firm-home <dir>]".into(),
            ))
        }
    }
    let company_id = required(args, "--company")?;
    let gateway_addr = required(args, "--gateway-addr")?;
    let instance_id = value(args, "--instance-id")
        .unwrap_or_else(|| format!("control-plane:{}", std::process::id()));
    let firm_home = firm_home(resolved, args)?;
    let layout = RemoteFabricStoreLayout::open(&firm_home).map_err(fabric_error)?;
    let store = layout
        .open_control_plane(&company_id)
        .map_err(fabric_error)?;
    let artifact_key = required_key_file(args, "--artifact-key-file")?;
    let capability_key = required_key_file(args, "--capability-key-file")?;
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(&company_id, artifact_key);
    let control = ControlPlane::new(&company_id, &instance_id, &store, &keys, capability_key);
    let now = now_unix_ms().map_err(fabric_error)?;
    let prior_revision = store
        .snapshot()
        .map_err(fabric_error)?
        .control_plane_leases
        .get(&company_id)
        .map_or(0, |lease| lease.revision);
    let lease = control
        .acquire_lease(
            &format!("control-plane-lease:{instance_id}"),
            prior_revision,
            now,
        )
        .map_err(fabric_error)?;
    let generation = lease.control_plane_generation;
    let tls = ControlPlaneTlsFiles {
        server_certificate_chain_pem: required_path(args, "--server-cert")?,
        server_private_key_pem: required_path(args, "--server-key")?,
        node_ca_pem: required_path(args, "--node-ca")?,
    };
    tls.validate().map_err(fabric_error)?;
    let http_addr = required(args, "--http-addr")?;
    let http_socket = http_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|_| CliError::Usage("--http-addr must be an explicit socket address".into()))?;
    if !http_socket.ip().is_loopback() {
        return Err(CliError::Usage(
            "Host REST is bearer-authenticated and must bind loopback; expose it only through a trusted TLS reverse proxy"
                .into(),
        ));
    }
    let host_token = required_secret_file(args, "--host-token-file")?;
    if host_token.len() < 32 {
        return Err(CliError::Usage(
            "--host-token must contain at least 32 characters".into(),
        ));
    }
    let trusted_origin =
        value(args, "--trusted-origin").unwrap_or_else(|| format!("http://{http_addr}"));
    let ca = harness_fabric::pki::FabricCaMaterial {
        certificate_pem: std::fs::read_to_string(required_path(args, "--ca-cert")?)?,
        private_key_pem: required_secret_file(args, "--ca-key")?,
    };
    let listener = std::net::TcpListener::bind(&gateway_addr)?;
    let stop = Arc::new(AtomicBool::new(false));
    let http_stop = stop.clone();
    let http_root = layout
        .control_plane_root(&company_id)
        .map_err(fabric_error)?;
    let http_company = company_id.clone();
    let http_instance = instance_id.clone();
    std::thread::spawn(move || {
        if let Err(error) = serve_host_http(
            &http_addr,
            &trusted_origin,
            &host_token,
            &http_company,
            &http_instance,
            generation,
            &http_root,
            artifact_key,
            capability_key,
            &ca,
            http_stop,
        ) {
            eprintln!("Remote Fabric Host REST stopped: {error}");
        }
    });
    let heartbeat_stop = stop.clone();
    let heartbeat_root = layout
        .control_plane_root(&company_id)
        .map_err(fabric_error)?;
    let heartbeat_company = company_id.clone();
    let heartbeat_instance = instance_id.clone();
    std::thread::spawn(move || {
        let store = harness_fabric::FabricStore::open(heartbeat_root)
            .expect("reopen Control Plane Store for heartbeat");
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(&heartbeat_company, artifact_key);
        let control = ControlPlane::new(
            &heartbeat_company,
            &heartbeat_instance,
            &store,
            &keys,
            capability_key,
        );
        let mut revision = lease.revision;
        while !heartbeat_stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(10));
            let now = now_unix_ms().expect("Control Plane heartbeat clock");
            match control.heartbeat_lease(generation, revision, now) {
                Ok(next) => revision = next.revision,
                Err(error) => {
                    eprintln!("Remote Fabric Control Plane lease lost: {error}");
                    heartbeat_stop.store(true, Ordering::SeqCst);
                }
            }
        }
    });
    println!(
        "Remote Fabric Control Plane company={company_id} generation={generation} host=http://{} gateway=wss://{gateway_addr}{}",
        required(args, "--http-addr")?,
        harness_fabric::transport::FABRIC_GATEWAY_PATH
    );
    let max_connections = value(args, "--max-connections")
        .map(|raw| raw.parse::<u64>())
        .transpose()
        .map_err(|_| CliError::Usage("--max-connections must be an integer".into()))?;
    let accepted = AtomicU64::new(0);
    for incoming in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let tcp = incoming?;
        let tls = tls.clone();
        let root = layout
            .control_plane_root(&company_id)
            .map_err(fabric_error)?;
        let company = company_id.clone();
        let instance = instance_id.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), FabricError> {
                let (mut socket, peer) = accept_control_plane_mtls(tcp, &tls)?;
                let store = harness_fabric::FabricStore::open(root)?;
                let keys = InMemoryArtifactKeyBackend::default();
                keys.insert(&company, artifact_key);
                let control = ControlPlane::new(&company, &instance, &store, &keys, capability_key);
                serve_control_plane_session(&mut socket, &peer, &control, generation)
            })();
            if let Err(error) = result {
                eprintln!("Remote Fabric gateway session ended: {error}");
            }
        });
        let count = accepted.fetch_add(1, Ordering::SeqCst).saturating_add(1);
        if max_connections.is_some_and(|limit| count >= limit) {
            break;
        }
    }
    stop.store(true, Ordering::SeqCst);
    Ok(())
}

fn node_gateway_command(
    wave4c_store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    if args.first().map(String::as_str) != Some("serve") {
        return Err(CliError::Usage(
            "fabric node-gateway serve --company <id> --control-plane <wss-url> --client-cert <path> --client-key <path> --control-plane-ca <path> --certificate-serial <serial> --public-key-fingerprint <sha256>".into(),
        ));
    }
    let company_id = required(args, "--company")?;
    let node_id = super::read_local_node_id()?;
    let now = now_unix_ms().map_err(fabric_error)?;
    let daemon = wave4c_store
        .latest_node_daemon_lease(&node_id)?
        .filter(|lease| {
            lease.status == harness_core::NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > now
        })
        .ok_or_else(|| {
            CliError::Usage(
                "NodeGateway requires the exact current active Wave4C NodeDaemonLease".into(),
            )
        })?;
    let firm_home = firm_home(resolved, args)?;
    let layout = RemoteFabricStoreLayout::open(firm_home).map_err(fabric_error)?;
    let local = layout
        .open_node_local(&company_id, &node_id)
        .map_err(fabric_error)?;
    let schema_digest = required(args, "--schema-bundle-digest")?;
    let credentials = resolve_node_credentials(args, &company_id, &node_id)?;
    let hello = NodeHello {
        company_id: company_id.clone(),
        node_id: node_id.clone(),
        instance_id: value(args, "--instance-id")
            .unwrap_or_else(|| format!("gateway:{}", std::process::id())),
        node_daemon_id: daemon.daemon_id.clone(),
        node_daemon_generation: daemon.generation,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: schema_digest,
        features: std::collections::BTreeSet::from([
            "durable-routing".into(),
            "remote-runtime".into(),
            "remote-message".into(),
            "artifact-transfer".into(),
        ]),
        build_sha: super::build_git_rev().to_string(),
        last_persisted_route_seq: local
            .snapshot()
            .map_err(fabric_error)?
            .inboxes
            .values()
            .map(|inbox| inbox.route_seq)
            .max()
            .unwrap_or(0),
        unresolved_operation_ids: local.unresolved_operation_ids().map_err(fabric_error)?,
        certificate_serial: credentials.certificate_serial.clone(),
        public_key_fingerprint: credentials.public_key_fingerprint.clone(),
    };
    let config = NodeFabricConfig {
        company_id,
        node_id,
        control_plane_url: required(args, "--control-plane")?,
        reconnect_floor_ms: 250,
        reconnect_ceiling_ms: 10_000,
    };
    let mut gateway = match &credentials.tls {
        ResolvedNodeTls::Files(tls) => NodeGatewayConnection::connect(&config, tls, hello),
        ResolvedNodeTls::Material(tls) => {
            NodeGatewayConnection::connect_with_material(&config, tls, hello)
        }
    }
    .map_err(fabric_error)?;
    local
        .bind_gateway_session(&gateway.session)
        .map_err(fabric_error)?;
    println!(
        "Remote Fabric NodeGateway connected node={} gateway_generation={} control_plane_generation={}",
        gateway.session.node_id,
        gateway.session.gateway_generation,
        gateway.session.control_plane_generation
    );
    let once = args.iter().any(|arg| arg == "--once");
    let mut application = Wave4cApplication {
        probe: ProbeApplication,
        firm_home: layout.firm_home().to_path_buf(),
        node_id: gateway.session.node_id.clone(),
        daemon_id: gateway.session.node_daemon_id.clone(),
        daemon_generation: gateway.session.node_daemon_generation,
    };
    loop {
        // A real two-machine mTLS heartbeat includes a durable lease CAS.
        // HeartbeatAck is followed by zero or more routed-operation frames,
        // but v1 has no batch-end frame. Every read therefore needs the same
        // bounded LAN timeout: a shorter idle poll can leave a delayed routed
        // frame in the socket and misread it as the next HeartbeatAck.
        gateway
            .set_read_timeout(Some(GATEWAY_FRAME_READ_TIMEOUT))
            .map_err(fabric_error)?;
        gateway.heartbeat().map_err(fabric_error)?;
        loop {
            match gateway.apply_next(&local, &mut application) {
                Ok(receipt) => println!(
                    "Remote Fabric applied operation={} effect={:?}",
                    receipt.operation_id, receipt.application_effect
                ),
                Err(error) if error.code == FabricErrorCode::TargetOffline && error.retryable => {
                    break
                }
                Err(error) => return Err(fabric_error(error)),
            }
        }
        for mut operation in local.pending_outbox_operations().map_err(fabric_error)? {
            let receipts = gateway
                .reconcile_operations(&local, BTreeSet::from([operation.id.clone()]))
                .map_err(fabric_error)?;
            if !receipts.is_empty() {
                // FabricStore already owns route truth. Accepted operations
                // are reconciled on later heartbeats until a generation-fenced
                // target terminal receipt arrives; they are never resubmitted.
                continue;
            }
            if operation.source_gateway_generation != Some(gateway.session.gateway_generation)
                || operation.source_node_daemon_id.as_deref()
                    != Some(gateway.session.node_daemon_id.as_str())
                || operation.source_node_daemon_generation
                    != Some(gateway.session.node_daemon_generation)
                || operation.control_plane_generation != gateway.session.control_plane_generation
            {
                operation = local
                    .rebind_unaccepted_outbox(&gateway.session, &operation.id, &receipts)
                    .map_err(fabric_error)?;
            }
            let actor = operation.actor.clone();
            let receipt = gateway
                .submit_operation(&local, &actor, operation)
                .map_err(fabric_error)?;
            println!(
                "Remote Fabric submitted operation={} receipt={:?}",
                receipt.operation_id, receipt.kind
            );
        }
        if once {
            gateway.close().map_err(fabric_error)?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

struct Wave4cApplication {
    probe: ProbeApplication,
    firm_home: PathBuf,
    node_id: String,
    daemon_id: String,
    daemon_generation: u64,
}

impl NodeApplication for Wave4cApplication {
    fn apply(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<(String, serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
        match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::Probe(_)
            | harness_fabric::ClosedOperationBody::ReconcileProbe(_) => self.probe.apply(operation),
            harness_fabric::ClosedOperationBody::RuntimeCommand(_) => {
                let envelope =
                    super::remote_fabric::resolved_runtime_command_from_operation(operation)?;
                if envelope.target_node_id != self.node_id
                    || envelope.target_node_daemon_id != self.daemon_id
                    || envelope.target_node_daemon_generation != self.daemon_generation
                {
                    return Err(FabricError::none(
                        FabricErrorCode::NodeStaleGeneration,
                        "RuntimeCommand is not fenced to this exact NodeGateway/NodeDaemon parent",
                    ));
                }
                let (result, effect) = super::remote_fabric::dispatch_resolved_runtime_command(
                    &self.firm_home,
                    operation,
                    &envelope,
                )?;
                Ok((
                    "agentfirm.remote_fabric.runtime_command_result.v1".into(),
                    result,
                    effect,
                ))
            }
            harness_fabric::ClosedOperationBody::Message(_) => {
                let message = super::remote_fabric::resolved_message_from_operation(operation)?;
                let execution_space_id = operation
                    .target_execution_space_id
                    .as_deref()
                    .ok_or_else(|| {
                        FabricError::none(
                            FabricErrorCode::TargetNotPlaced,
                            "Message route has no target Execution Space",
                        )
                    })?;
                let space =
                    super::execution_space::context_for_id(&self.firm_home, execution_space_id)
                        .map_err(|error| {
                            FabricError::none(
                                FabricErrorCode::StoreUnavailable,
                                format!("Execution Space registry failed: {error}"),
                            )
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::TargetNotPlaced,
                                "Message target Execution Space is not registered on this Node",
                            )
                        })?;
                let store = HarnessStore::new(space.store_root);
                let context = harness_core::agentfirm_api::MutationContext {
                    execution_space_id: execution_space_id.into(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: self.daemon_id.clone(),
                    },
                    authority_actor: None,
                    command_name: "remote_message_persist".into(),
                    idempotency_key: operation.id.clone(),
                    expected_version: 0,
                    request_fingerprint: Some(harness_fabric::json_digest(operation).map_err(
                        |error| {
                            FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
                        },
                    )?),
                };
                let persisted = store
                    .persist_remote_message(
                        &context,
                        operation,
                        message,
                        &self.node_id,
                        &self.daemon_id,
                        self.daemon_generation,
                    )
                    .map_err(|error| {
                        FabricError::none(FabricErrorCode::UnauthorizedActor, error.to_string())
                    })?;
                Ok((
                    "agentfirm.remote_fabric.message_persisted.v1".into(),
                    serde_json::json!({
                        "message_id": persisted.projection.id,
                        "canonical_event_id": persisted.event.id,
                        "replayed": persisted.replayed,
                    }),
                    harness_fabric::EffectCertainty::Applied,
                ))
            }
            _ => Err(FabricError::none(
                FabricErrorCode::FeatureIncompatible,
                "Node application adapter does not own this routed reference kind",
            )),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn serve_host_http(
    addr: &str,
    trusted_origin: &str,
    host_token: &str,
    company_id: &str,
    instance_id: &str,
    generation: u64,
    store_root: &Path,
    artifact_key: [u8; 32],
    capability_key: [u8; 32],
    ca: &harness_fabric::pki::FabricCaMaterial,
    stop: Arc<AtomicBool>,
) -> CliResult<()> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    while !stop.load(Ordering::SeqCst) {
        let (stream, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let request = match read_http_request(stream, trusted_origin, host_token) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("Remote Fabric Host REST request rejected: {error}");
                continue;
            }
        };
        let store = harness_fabric::FabricStore::open(store_root).map_err(fabric_error)?;
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(company_id, artifact_key);
        let control = ControlPlane::new(company_id, instance_id, &store, &keys, capability_key);
        handle_host_http(
            request,
            trusted_origin,
            host_token,
            &control,
            generation,
            ca,
        )?;
    }
    Ok(())
}

struct HttpRequest {
    stream: TcpStream,
    method: String,
    target: String,
    headers: std::collections::BTreeMap<String, String>,
    body: Vec<u8>,
}

const STANDARD_FABRIC_HTTP_BODY_LIMIT: usize = 1024 * 1024;
const MAX_FABRIC_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
// Artifact completion uses a closed JSON envelope with hexadecimal content.
// Reserve bounded space for the signed capability and JSON framing without
// widening any other Host REST endpoint beyond the normal 1 MiB limit.
const ARTIFACT_COMPLETE_HTTP_BODY_LIMIT: usize = MAX_FABRIC_ARTIFACT_BYTES * 2 + 256 * 1024;

fn is_artifact_complete_path(path: &str) -> bool {
    path.strip_prefix("/v1/fabric/artifacts/")
        .and_then(|rest| rest.strip_suffix("/complete"))
        .is_some_and(|artifact_id| !artifact_id.is_empty() && !artifact_id.contains('/'))
}

fn fabric_http_body_limit(method: &str, target: &str) -> usize {
    let path = target.split('?').next().unwrap_or_default();
    if method == "POST" && is_artifact_complete_path(path) {
        ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
    } else {
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    }
}

fn authorized_fabric_http_body_limit(
    method: &str,
    target: &str,
    headers: &std::collections::BTreeMap<String, String>,
    trusted_origin: &str,
    host_token: &str,
) -> usize {
    let requested_limit = fabric_http_body_limit(method, target);
    let large_body_authorized = requested_limit > STANDARD_FABRIC_HTTP_BODY_LIMIT
        && headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|presented| constant_time_secret_eq(presented, host_token))
        && headers
            .get("origin")
            .is_none_or(|origin| origin == trusted_origin);
    if large_body_authorized {
        requested_limit
    } else {
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    }
}

fn read_http_request(
    mut stream: TcpStream,
    trusted_origin: &str,
    host_token: &str,
) -> CliResult<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    if !matches!(method.as_str(), "GET" | "POST" | "OPTIONS") || !target.starts_with("/v1/fabric/")
    {
        write_http_json(
            &mut stream,
            "404 Not Found",
            &serde_json::json!({"ok":false,"error":"unknown_fabric_endpoint"}),
            None,
        )?;
        return Err(CliError::Usage("unknown Remote Fabric endpoint".into()));
    }
    let mut headers = std::collections::BTreeMap::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| CliError::Usage("malformed HTTP header".into()))?;
        let name = name.trim().to_ascii_lowercase();
        if headers.insert(name.clone(), value.trim().into()).is_some() {
            return Err(CliError::Usage("duplicate HTTP header".into()));
        }
        if name == "content-length" {
            content_length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| CliError::Usage("invalid Content-Length".into()))?;
        }
    }
    let body_limit =
        authorized_fabric_http_body_limit(&method, &target, &headers, trusted_origin, host_token);
    if content_length > body_limit {
        return Err(CliError::Usage(format!(
            "Remote Fabric REST body exceeds endpoint limit of {body_limit} bytes"
        )));
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(HttpRequest {
        stream,
        method,
        target,
        headers,
        body,
    })
}

fn handle_host_http<K: harness_fabric::ArtifactKeyBackend>(
    mut request: HttpRequest,
    trusted_origin: &str,
    host_token: &str,
    control: &ControlPlane<'_, K>,
    generation: u64,
    ca: &harness_fabric::pki::FabricCaMaterial,
) -> CliResult<()> {
    let path = request.target.split('?').next().unwrap_or_default();
    if request.method == "OPTIONS" {
        let origin = request.headers.get("origin").map(String::as_str);
        if origin != Some(trusted_origin) {
            write_http_json(
                &mut request.stream,
                "403 Forbidden",
                &serde_json::json!({"ok":false,"error":"untrusted_origin"}),
                None,
            )?;
        } else {
            write_http_json(
                &mut request.stream,
                "200 OK",
                &serde_json::json!({"ok":true}),
                origin,
            )?;
        }
        return Ok(());
    }
    let origin = request.headers.get("origin").map(String::as_str);
    if origin.is_some_and(|origin| origin != trusted_origin) {
        return respond_fabric_error(
            &mut request.stream,
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "browser origin is not the configured trusted Control Plane origin",
            ),
            None,
        );
    }
    let node_enroll = request.method == "POST" && path == "/v1/fabric/nodes/enroll";
    let actor = if node_enroll {
        None
    } else {
        let presented = request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "));
        if !presented.is_some_and(|presented| constant_time_secret_eq(presented, host_token))
            || request.headers.keys().any(|name| {
                matches!(
                    name.as_str(),
                    "x-agentfirm-actor-id" | "x-agentfirm-actor-kind" | "x-agentfirm-authority-id"
                )
            })
        {
            return respond_fabric_error(
                &mut request.stream,
                FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    "Company-issued Host credential is missing or request attempted identity selection",
                ),
                origin,
            );
        }
        Some(AuthenticatedActor {
            company_id: control.company_id().into(),
            actor_id: "company-host:http".into(),
            actor_kind: harness_fabric::ActorKind::Human,
            role_bindings: std::collections::BTreeSet::from([
                "company_host".into(),
                "artifact_write".into(),
                "artifact_read".into(),
            ]),
            session_id: format!("host-http:{generation}"),
            issued_at_unix_ms: now_unix_ms().map_err(fabric_error)?,
            expires_at_unix_ms: now_unix_ms().map_err(fabric_error)?.saturating_add(30_000),
        })
    };
    let body = if request.body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&request.body) {
            Ok(value) => value,
            Err(error) => {
                return respond_fabric_error(
                    &mut request.stream,
                    FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()),
                    origin,
                )
            }
        }
    };
    let now = now_unix_ms().map_err(fabric_error)?;
    let result = route_host_http(
        &request.method,
        path,
        &request.target,
        &body,
        actor.as_ref(),
        control,
        generation,
        ca,
        now,
        host_token,
    );
    match result {
        Ok(value) => write_http_json(&mut request.stream, "200 OK", &value, origin),
        Err(error) => respond_fabric_error(&mut request.stream, error, origin),
    }
}

#[allow(clippy::too_many_arguments)]
fn route_host_http<K: harness_fabric::ArtifactKeyBackend>(
    method: &str,
    path: &str,
    target: &str,
    body: &serde_json::Value,
    actor: Option<&AuthenticatedActor>,
    control: &ControlPlane<'_, K>,
    generation: u64,
    ca: &harness_fabric::pki::FabricCaMaterial,
    now: u64,
    host_token: &str,
) -> Result<serde_json::Value, FabricError> {
    let required_actor = || {
        actor.ok_or_else(|| {
            FabricError::none(FabricErrorCode::UnauthorizedActor, "Host actor is required")
        })
    };
    if method == "POST" && path == "/v1/fabric/enrollments" {
        reject_unknown_json_fields(
            body,
            &[
                "enrollment_id",
                "requested_name",
                "allowed_capabilities",
                "expires_at_unix_ms",
            ],
        )?;
        let actor = required_actor()?;
        let enrollment_id = json_string(body, "enrollment_id")?;
        let requested_name = json_string(body, "requested_name")?;
        let capabilities = json_string_set(body, "allowed_capabilities")?;
        let expires_at = body
            .get("expires_at_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| now.saturating_add(10 * 60 * 1000));
        let raw_token = format!(
            "enroll-{}",
            harness_fabric::sha256_hex(format!("{host_token}:{enrollment_id}:{now}").as_bytes())
        );
        let enrollment = control.create_enrollment(
            actor,
            generation,
            &enrollment_id,
            &raw_token,
            &requested_name,
            capabilities,
            expires_at,
            now,
        )?;
        return Ok(serde_json::json!({"enrollment":enrollment,"raw_token":raw_token}));
    }
    if method == "POST" && path == "/v1/fabric/nodes/enroll" {
        reject_unknown_json_fields(
            body,
            &[
                "raw_token",
                "node_id",
                "display_name",
                "csr_pem",
                "schema_bundle_digest",
            ],
        )?;
        let raw_token = json_string(body, "raw_token")?;
        let node_id = json_string(body, "node_id")?;
        let display_name = json_string(body, "display_name")?;
        let csr_pem = json_string(body, "csr_pem")?;
        let schema_digest = json_string(body, "schema_bundle_digest")?;
        harness_fabric::pki::verify_node_csr(&csr_pem, control.company_id(), &node_id)?;
        let issued = harness_fabric::pki::issue_node_certificate(
            ca,
            &csr_pem,
            control.company_id(),
            &node_id,
            now,
        )?;
        let (node, certificate) = control.consume_enrollment_csr(
            generation,
            &raw_token,
            &node_id,
            &display_name,
            &csr_pem,
            &issued.serial,
            issued.expires_at_unix_ms,
            &schema_digest,
            now,
        )?;
        return Ok(serde_json::json!({
            "node":node,
            "certificate":certificate,
            "client_certificate_pem":issued.certificate_pem,
            "company_ca_pem":ca.certificate_pem,
        }));
    }
    let state = control.store().snapshot()?;
    if method == "GET" && path == "/v1/fabric/nodes" {
        let limit = query_value(target, "limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50)
            .clamp(1, 200);
        let cursor = query_value(target, "cursor");
        let status = query_value(target, "status");
        let mut nodes = state.nodes.values().cloned().collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.id.cmp(&right.id));
        let cursor_node_id = cursor
            .as_ref()
            .map(|cursor| {
                nodes
                    .iter()
                    .find(|node| {
                        fabric_node_cursor(control.company_id(), &node.id, state.revision)
                            == *cursor
                    })
                    .map(|node| node.id.clone())
                    .ok_or_else(|| {
                        FabricError::none(
                            FabricErrorCode::ExpectedRevisionConflict,
                            "Fabric node cursor is invalid or belongs to an older snapshot",
                        )
                    })
            })
            .transpose()?;
        nodes.retain(|node| {
            cursor_node_id
                .as_ref()
                .is_none_or(|cursor_node_id| node.id > *cursor_node_id)
                && status.as_ref().is_none_or(|status| {
                    format!("{:?}", node.administrative_status).to_ascii_lowercase() == *status
                })
        });
        let page = nodes.into_iter().take(limit).collect::<Vec<_>>();
        let next_cursor = page
            .last()
            .map(|node| fabric_node_cursor(control.company_id(), &node.id, state.revision));
        let diagnostics = harness_fabric::diagnostics::inspect_fabric(
            control.store(),
            control.company_id(),
            now,
        )?;
        return Ok(serde_json::json!({
            "nodes":page,
            "next_cursor":next_cursor,
            "diagnostics":diagnostics,
        }));
    }
    if method == "GET" {
        if let Some(artifact_id) = path
            .strip_prefix("/v1/fabric/artifacts/")
            .and_then(|rest| rest.strip_suffix("/download-capability"))
        {
            let node_id = query_value(target, "node_id").ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "download-capability requires node_id",
                )
            })?;
            let capability = control.issue_download_capability(
                required_actor()?,
                generation,
                artifact_id,
                &node_id,
                now,
            )?;
            return Ok(serde_json::json!({"download_capability":capability}));
        }
        if let Some(node_id) = path.strip_prefix("/v1/fabric/nodes/") {
            let node = state.nodes.get(node_id).cloned().ok_or_else(|| {
                FabricError::none(FabricErrorCode::TargetNotPlaced, "Node does not exist")
            })?;
            let lease = state.gateway_leases.get(node_id);
            let diagnostic = harness_fabric::diagnostics::inspect_fabric(
                control.store(),
                control.company_id(),
                now,
            )?
            .nodes
            .into_iter()
            .find(|diagnostic| diagnostic.node_id == node_id);
            return Ok(serde_json::json!({
                "node":node,
                "gateway_lease":lease,
                "connection_status":node.connection_status(lease, generation, now),
                "diagnostic":diagnostic,
            }));
        }
        if let Some(operation_id) = path.strip_prefix("/v1/fabric/operations/") {
            let operation = state.operations.get(operation_id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "routed operation does not exist",
                )
            })?;
            let attempts = state
                .attempts
                .values()
                .filter(|attempt| attempt.operation_id == operation_id)
                .cloned()
                .collect::<Vec<_>>();
            let receipts = state
                .receipts
                .values()
                .filter(|receipt| receipt.operation_id == operation_id)
                .cloned()
                .collect::<Vec<_>>();
            return Ok(
                serde_json::json!({"operation":operation,"attempts":attempts,"receipts":receipts}),
            );
        }
    }
    if method == "POST" {
        if let Some(node_id) = path
            .strip_prefix("/v1/fabric/nodes/")
            .and_then(|rest| rest.strip_suffix("/drain"))
        {
            reject_unknown_json_fields(body, &["expected_revision"])?;
            let revision = json_u64(body, "expected_revision")?;
            let node = control.set_node_administrative_status(
                required_actor()?,
                generation,
                node_id,
                revision,
                NodeAdministrativeStatus::Draining,
                now,
            )?;
            return Ok(serde_json::json!({"node":node}));
        }
        if let Some(node_id) = path
            .strip_prefix("/v1/fabric/nodes/")
            .and_then(|rest| rest.strip_suffix("/revoke"))
        {
            reject_unknown_json_fields(body, &["expected_revision", "reason"])?;
            let revision = json_u64(body, "expected_revision")?;
            let reason = json_string(body, "reason")?;
            let node = control.revoke_node(
                required_actor()?,
                generation,
                node_id,
                revision,
                &reason,
                now,
            )?;
            return Ok(serde_json::json!({"node":node}));
        }
        if path == "/v1/fabric/artifacts/initiate" {
            reject_unknown_json_fields(
                body,
                &[
                    "artifact_id",
                    "source_node_id",
                    "operation_id",
                    "media_type",
                    "size_bytes",
                    "sha256",
                    "classification",
                    "authorized_readers",
                ],
            )?;
            let classification = match json_string(body, "classification")?.as_str() {
                "company_internal" => ArtifactClassification::CompanyInternal,
                "sensitive" => ArtifactClassification::Sensitive,
                _ => {
                    return Err(FabricError::none(
                        FabricErrorCode::ArtifactInvalid,
                        "classification must be company_internal|sensitive",
                    ))
                }
            };
            let (manifest, capability) = control.initiate_artifact(
                required_actor()?,
                generation,
                &json_string(body, "artifact_id")?,
                &json_string(body, "source_node_id")?,
                body.get("operation_id").and_then(serde_json::Value::as_str),
                &json_string(body, "media_type")?,
                json_u64(body, "size_bytes")?,
                &json_string(body, "sha256")?,
                classification,
                json_string_set(body, "authorized_readers")?,
                now,
            )?;
            return Ok(serde_json::json!({"manifest":manifest,"upload_capability":capability}));
        }
        if let Some(artifact_id) = path
            .strip_prefix("/v1/fabric/artifacts/")
            .and_then(|rest| rest.strip_suffix("/complete"))
        {
            reject_unknown_json_fields(body, &["capability", "bytes_hex"])?;
            let capability: harness_fabric::ArtifactCapability =
                serde_json::from_value(body.get("capability").cloned().ok_or_else(|| {
                    FabricError::none(FabricErrorCode::CapabilityInvalid, "capability is required")
                })?)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::CapabilityInvalid, error.to_string())
                })?;
            if capability.artifact_id != artifact_id {
                return Err(FabricError::none(
                    FabricErrorCode::CapabilityInvalid,
                    "artifact path and capability disagree",
                ));
            }
            let bytes = decode_hex(&json_string(body, "bytes_hex")?)?;
            let manifest = control.complete_artifact(generation, &capability, &bytes, now)?;
            return Ok(serde_json::json!({"manifest":manifest}));
        }
    }
    Err(FabricError::none(
        FabricErrorCode::InvalidPayload,
        "unknown Remote Fabric Host REST endpoint",
    ))
}

fn fabric_node_cursor(company_id: &str, node_id: &str, snapshot_revision: u64) -> String {
    harness_fabric::sha256_hex(format!(
        "agentfirm.remote-fabric.node-cursor.v1\n{company_id}\n{node_id}\n{snapshot_revision}"
    ))
}

fn respond_fabric_error(
    stream: &mut TcpStream,
    error: FabricError,
    origin: Option<&str>,
) -> CliResult<()> {
    let status = match error.code {
        FabricErrorCode::UnauthorizedActor | FabricErrorCode::WrongCompany => "403 Forbidden",
        FabricErrorCode::TargetNotPlaced | FabricErrorCode::OperationUnknown => "404 Not Found",
        FabricErrorCode::ExpectedRevisionConflict | FabricErrorCode::IdempotencyConflict => {
            "409 Conflict"
        }
        FabricErrorCode::StoreUnavailable => "503 Service Unavailable",
        _ => "400 Bad Request",
    };
    write_http_json(
        stream,
        status,
        &serde_json::json!({"ok":false,"error":error}),
        origin,
    )
}

fn write_http_json(
    stream: &mut TcpStream,
    status: &str,
    value: &serde_json::Value,
    origin: Option<&str>,
) -> CliResult<()> {
    let body = serde_json::to_vec(value)?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        body.len()
    )?;
    if let Some(origin) = origin {
        write!(
            stream,
            "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, If-Match\r\n"
        )?;
    }
    write!(stream, "Connection: close\r\n\r\n")?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

fn json_string(value: &serde_json::Value, key: &str) -> Result<String, FabricError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} is required"),
            )
        })
}

fn reject_unknown_json_fields(
    value: &serde_json::Value,
    allowed: &[&str],
) -> Result<(), FabricError> {
    let object = value.as_object().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "Remote Fabric mutation body must be a JSON object",
        )
    })?;
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("unknown Remote Fabric mutation field: {field}"),
        ));
    }
    Ok(())
}

fn constant_time_secret_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn json_u64(value: &serde_json::Value, key: &str) -> Result<u64, FabricError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} is required"),
            )
        })
}

fn json_string_set(
    value: &serde_json::Value,
    key: &str,
) -> Result<std::collections::BTreeSet<String>, FabricError> {
    let values = value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} must be an array"),
            )
        })?;
    let result = values
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect::<Option<std::collections::BTreeSet<_>>>()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} must contain only strings"),
            )
        })?;
    if result.is_empty() {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("{key} cannot be empty"),
        ));
    }
    Ok(result)
}

fn query_value(target: &str, key: &str) -> Option<String> {
    target.split('?').nth(1)?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_string())
    })
}

fn decode_hex(raw: &str) -> Result<Vec<u8>, FabricError> {
    if !raw.len().is_multiple_of(2) || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FabricError::none(
            FabricErrorCode::ArtifactInvalid,
            "bytes_hex must contain an even number of hexadecimal characters",
        ));
    }
    (0..raw.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&raw[index..index + 2], 16).map_err(|_| {
                FabricError::none(FabricErrorCode::ArtifactInvalid, "bytes_hex is invalid")
            })
        })
        .collect()
}

enum ResolvedNodeTls {
    Files(NodeTlsIdentityFiles),
    Material(NodeTlsIdentityMaterial),
}

struct ResolvedNodeCredentials {
    tls: ResolvedNodeTls,
    certificate_serial: String,
    public_key_fingerprint: String,
}

fn resolve_node_credentials(
    args: &[String],
    company_id: &str,
    node_id: &str,
) -> CliResult<ResolvedNodeCredentials> {
    match value(args, "--credential-backend")
        .unwrap_or_else(|| "file".into())
        .as_str()
    {
        "file" => Ok(ResolvedNodeCredentials {
            tls: ResolvedNodeTls::Files(NodeTlsIdentityFiles {
                client_certificate_chain_pem: required_path(args, "--client-cert")?,
                client_private_key_pem: required_path(args, "--client-key")?,
                control_plane_ca_pem: required_path(args, "--control-plane-ca")?,
            }),
            certificate_serial: required(args, "--certificate-serial")?,
            public_key_fingerprint: required(args, "--public-key-fingerprint")?,
        }),
        "macos-keychain" => {
            let service = required(args, "--keychain-service")?;
            if service.trim().is_empty() || service.len() > 128 {
                return Err(CliError::Usage(
                    "--keychain-service must be a bounded non-empty service name".into(),
                ));
            }
            // Validate the public enrolled identity before touching Keychain.
            // Besides failing closed, this prevents avoidable ACL prompts when
            // an incomplete login-agent command is installed.
            let certificate_serial = required(args, "--certificate-serial")?;
            let public_key_fingerprint = required(args, "--public-key-fingerprint")?;
            let prefix = format!("{company_id}:{node_id}");
            Ok(ResolvedNodeCredentials {
                tls: ResolvedNodeTls::Material(NodeTlsIdentityMaterial {
                    client_certificate_chain_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:client-certificate"),
                    )?
                    .into_bytes(),
                    client_private_key_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:client-private-key"),
                    )?
                    .into_bytes(),
                    control_plane_ca_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:control-plane-ca"),
                    )?
                    .into_bytes(),
                }),
                // Serial and fingerprint are public certificate identity, not
                // secret key material. Requiring five separate Keychain ACL
                // prompts made a login LaunchAgent repeatedly block on user
                // interaction. Keep only the three PEM materials in Keychain;
                // the explicit public values are still checked by the
                // generation-fenced mTLS welcome against the enrolled Node.
                certificate_serial,
                public_key_fingerprint,
            })
        }
        _ => Err(CliError::Usage(
            "--credential-backend must be file|macos-keychain".into(),
        )),
    }
}

#[cfg(target_os = "macos")]
fn keychain_secret(service: &str, account: &str) -> CliResult<String> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-w", "-s", service, "-a", account])
        .output()?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "macOS Keychain item is unavailable for service={service} account={account}"
        )));
    }
    let secret = String::from_utf8(output.stdout)
        .map_err(|_| CliError::Usage("macOS Keychain item is not valid UTF-8".into()))?;
    let secret = secret.trim().to_string();
    if secret.is_empty() {
        return Err(CliError::Usage(format!(
            "macOS Keychain item is empty for service={service} account={account}"
        )));
    }
    Ok(secret)
}

#[cfg(not(target_os = "macos"))]
fn keychain_secret(_service: &str, _account: &str) -> CliResult<String> {
    Err(CliError::Usage(
        "macos-keychain credential backend is available only on macOS".into(),
    ))
}

pub(crate) fn firm_home(resolved: &ResolvedStore, args: &[String]) -> CliResult<PathBuf> {
    if let Some(path) = value(args, "--firm-home") {
        return Ok(PathBuf::from(path));
    }
    if let Some(space) = &resolved.execution_space_context {
        return space
            .store_root
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or_else(|| CliError::Usage("cannot derive FIRM_HOME from Execution Space".into()));
    }
    super::execution_space::firm_home().map_err(|error| CliError::Usage(error.to_string()))
}

fn required(args: &[String], name: &str) -> CliResult<String> {
    value(args, name).ok_or_else(|| CliError::Usage(format!("missing required {name}")))
}

fn required_path(args: &[String], name: &str) -> CliResult<PathBuf> {
    Ok(PathBuf::from(required(args, name)?))
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn required_key_file(args: &[String], name: &str) -> CliResult<[u8; 32]> {
    let raw = required_secret_file(args, name)?;
    let raw = raw.trim();
    if raw.len() != 64 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Usage(format!(
            "{name} must contain exactly 64 hexadecimal characters"
        )));
    }
    let mut key = [0u8; 32];
    for (index, output) in key.iter_mut().enumerate() {
        *output = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
            .map_err(|_| CliError::Usage(format!("{name} is invalid")))?;
    }
    Ok(key)
}

fn required_secret_file(args: &[String], name: &str) -> CliResult<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = required_path(args, name)?;
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(CliError::Usage(format!(
            "{name} must be a regular non-symlink file with no group/other permissions"
        )));
    }
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

fn fabric_error(error: FabricError) -> CliError {
    CliError::Usage(format!("REMOTE_FABRIC_{:?}: {}", error.code, error.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_gateway_frame_timeout_is_bounded_within_the_lease() {
        assert_eq!(GATEWAY_FRAME_READ_TIMEOUT, Duration::from_secs(5));
        assert!(GATEWAY_FRAME_READ_TIMEOUT < Duration::from_secs(30));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_credentials_require_public_identity_before_acl_access() {
        let args = vec![
            "--credential-backend".into(),
            "macos-keychain".into(),
            "--keychain-service".into(),
            "agentfirm.test.must-not-be-read".into(),
        ];
        let error = match resolve_node_credentials(&args, "company-test", "node-test") {
            Ok(_) => panic!("incomplete enrolled identity must fail before Keychain access"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("--certificate-serial"));
    }

    #[test]
    fn artifact_complete_body_limit_matches_the_frozen_64_mib_contract_only() {
        assert_eq!(
            fabric_http_body_limit("POST", "/v1/fabric/artifacts/artifact-a/complete"),
            ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
        );
        assert!(ARTIFACT_COMPLETE_HTTP_BODY_LIMIT > MAX_FABRIC_ARTIFACT_BYTES * 2);
        assert_eq!(
            fabric_http_body_limit("POST", "/v1/fabric/artifacts/initiate"),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
        assert_eq!(
            fabric_http_body_limit("GET", "/v1/fabric/artifacts/artifact-a/complete"),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
        assert_eq!(
            fabric_http_body_limit("POST", "/v1/fabric/artifacts/a/nested/complete"),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
    }

    #[test]
    fn host_rest_secret_and_mutation_shapes_fail_closed() {
        assert!(constant_time_secret_eq("host-secret-a", "host-secret-a"));
        assert!(!constant_time_secret_eq("host-secret-a", "host-secret-b"));
        assert!(!constant_time_secret_eq("short", "longer"));
        reject_unknown_json_fields(
            &serde_json::json!({"expected_revision": 1}),
            &["expected_revision"],
        )
        .expect("closed mutation shape");
        assert_eq!(
            reject_unknown_json_fields(
                &serde_json::json!({"expected_revision": 1, "actor_id": "browser"}),
                &["expected_revision"],
            )
            .expect_err("browser identity field fails closed")
            .code,
            FabricErrorCode::InvalidPayload
        );
        let artifact_path = "/v1/fabric/artifacts/artifact-a/complete";
        let trusted_origin = "https://company.example";
        let host_token = "host-secret-a";
        let mut headers = std::collections::BTreeMap::new();
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            STANDARD_FABRIC_HTTP_BODY_LIMIT,
            "unauthenticated local callers never receive the large allocation budget"
        );
        headers.insert("authorization".into(), "Bearer host-secret-a".into());
        headers.insert("origin".into(), trusted_origin.into());
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
        );
        headers.insert("origin".into(), "https://malicious.example".into());
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
    }

    #[test]
    fn host_rest_enrollment_uses_csr_possession_and_one_atomic_consumption() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-fabric-http-enroll-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("remove prior test root");
        }
        std::fs::create_dir(&root).expect("create test root");
        let store = harness_fabric::FabricStore::open(&root).expect("FabricStore");
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert("company-test", [4; 32]);
        let control = ControlPlane::new("company-test", "control-test", &store, &keys, [5; 32]);
        let now = now_unix_ms().expect("clock");
        let lease = control
            .acquire_lease("lease-test", 0, now)
            .expect("Control Plane lease");
        let ca = harness_fabric::pki::generate_ca("company-test").expect("Company CA");
        let actor = AuthenticatedActor {
            company_id: "company-test".into(),
            actor_id: "company-host:http".into(),
            actor_kind: harness_fabric::ActorKind::Human,
            role_bindings: std::collections::BTreeSet::from(["company_host".into()]),
            session_id: "host-test".into(),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + 60_000,
        };
        let created = route_host_http(
            "POST",
            "/v1/fabric/enrollments",
            "/v1/fabric/enrollments",
            &serde_json::json!({
                "enrollment_id":"enrollment-node-a",
                "requested_name":"Node A",
                "allowed_capabilities":["durable-routing","artifact-transfer"]
            }),
            Some(&actor),
            &control,
            lease.control_plane_generation,
            &ca,
            now + 1,
            "host-token-00000000000000000000000000000000",
        )
        .expect("create enrollment");
        let raw_token = created["raw_token"].as_str().expect("one-time token");
        let csr =
            harness_fabric::pki::generate_node_csr("company-test", "node-a").expect("Node CSR");
        let enrolled = route_host_http(
            "POST",
            "/v1/fabric/nodes/enroll",
            "/v1/fabric/nodes/enroll",
            &serde_json::json!({
                "raw_token":raw_token,
                "node_id":"node-a",
                "display_name":"Node A",
                "csr_pem":csr.csr_pem,
                "schema_bundle_digest":"schema-test"
            }),
            None,
            &control,
            lease.control_plane_generation,
            &ca,
            now + 2,
            "host-token-00000000000000000000000000000000",
        )
        .expect("consume CSR enrollment");
        assert_eq!(enrolled["node"]["id"], "node-a");
        assert_eq!(
            enrolled["node"]["public_key_fingerprint"],
            csr.public_key_fingerprint
        );
        let listed = route_host_http(
            "GET",
            "/v1/fabric/nodes",
            "/v1/fabric/nodes?limit=1",
            &serde_json::Value::Null,
            Some(&actor),
            &control,
            lease.control_plane_generation,
            &ca,
            now + 2,
            "host-token-00000000000000000000000000000000",
        )
        .expect("list Nodes with opaque snapshot cursor");
        let cursor = listed["next_cursor"].as_str().expect("next cursor");
        assert_ne!(cursor, "node-a");
        assert_eq!(cursor.len(), 64);
        assert_eq!(
            route_host_http(
                "GET",
                "/v1/fabric/nodes",
                "/v1/fabric/nodes?cursor=browser-selected-node-a",
                &serde_json::Value::Null,
                Some(&actor),
                &control,
                lease.control_plane_generation,
                &ca,
                now + 2,
                "host-token-00000000000000000000000000000000",
            )
            .expect_err("raw or forged Node cursor fails closed")
            .code,
            FabricErrorCode::ExpectedRevisionConflict
        );
        let before = store.snapshot().expect("before replay");
        let replay = route_host_http(
            "POST",
            "/v1/fabric/nodes/enroll",
            "/v1/fabric/nodes/enroll",
            &serde_json::json!({
                "raw_token":raw_token,
                "node_id":"node-a",
                "display_name":"Node A",
                "csr_pem":csr.csr_pem,
                "schema_bundle_digest":"schema-test"
            }),
            None,
            &control,
            lease.control_plane_generation,
            &ca,
            now + 3,
            "host-token-00000000000000000000000000000000",
        )
        .expect_err("one-use token cannot replay");
        assert_eq!(replay.code, FabricErrorCode::EnrollmentConsumed);
        assert_eq!(store.snapshot().expect("after replay"), before);
        std::fs::remove_dir_all(root).expect("remove test root");
    }
}
