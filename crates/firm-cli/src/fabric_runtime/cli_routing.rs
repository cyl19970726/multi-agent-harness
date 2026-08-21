use super::*;


pub(super) fn route_command(
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
    let node_id = crate::read_local_node_id()?;
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
            let intent = reference.canonical_command_intent.clone();
            if intent.target_execution_space_id != target_execution_space_id {
                return Err(CliError::Usage(
                    "remote RuntimeCommand intent must match the exact target Execution Space; target identity and capability are server-resolved"
                        .into(),
                ));
            }
            (
                harness_fabric::RUNTIME_COMMAND_REFERENCE_KIND,
                harness_fabric::RUNTIME_COMMAND_REFERENCE_SCHEMA,
                value(args, "--source-space"),
                Some(intent.expected_version),
                harness_fabric::OperationPriority::Control,
                intent.expires_unix_ms,
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
    if let Some(existing) = local
        .snapshot()
        .map_err(fabric_error)?
        .outboxes
        .get(&operation_id)
        .cloned()
    {
        let existing_operation = existing.operation.as_ref().ok_or_else(|| {
            fabric_error(FabricError::none(
                FabricErrorCode::OperationUnknown,
                "durable source outbox lost its canonical operation envelope",
            ))
        })?;
        let capability = route_capability(&kind);
        let same_intent = route_queue_intent_matches(
            existing_operation,
            &company_id,
            &operation_id,
            wire_kind,
            &node_id,
            source_execution_space_id.as_deref(),
            &target_node_id,
            &target_execution_space_id,
            &idempotency_key,
            &ordering_key,
            expected_target_revision,
            body_schema,
            &body,
            capability,
        )
        .map_err(fabric_error)?;
        if !same_intent {
            return Err(fabric_error(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "route queue replay changed its durable semantic intent",
            )));
        }
        existing_operation.validate_digest().map_err(fabric_error)?;
        existing_operation.closed_body().map_err(fabric_error)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "queued_operation": existing,
                "replayed": true,
            }))?
        );
        return Ok(());
    }
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
            route_capability(&kind).into(),
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
        let intent = crate::remote_fabric::runtime_intent_from_operation(&operation)
            .map_err(fabric_error)?;
        if intent.expires_unix_ms <= now {
            return Err(CliError::Usage(
                "remote RuntimeCommand expired before local durable queueing".into(),
            ));
        }
    } else if kind == "message" {
        crate::remote_fabric::resolved_message_from_operation(&operation).map_err(fabric_error)?;
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

pub(super) fn route_capability(kind: &str) -> &'static str {
    match kind {
        "probe" => "remote-probe",
        "runtime" => "remote-runtime",
        "message" => "remote-message",
        _ => unreachable!("route kind was closed before capability resolution"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn route_queue_intent_matches(
    existing: &harness_fabric::RoutedOperation,
    company_id: &str,
    operation_id: &str,
    wire_kind: &str,
    source_node_id: &str,
    source_execution_space_id: Option<&str>,
    target_node_id: &str,
    target_execution_space_id: &str,
    idempotency_key: &str,
    ordering_key: &str,
    expected_target_revision: Option<u64>,
    body_schema: &str,
    body: &serde_json::Value,
    capability: &str,
) -> Result<bool, FabricError> {
    Ok(existing.company_id == company_id
        && existing.id == operation_id
        && existing.kind == wire_kind
        && existing.source_authority == harness_fabric::OperationSourceAuthority::Node
        && existing.source_node_id.as_deref() == Some(source_node_id)
        && existing.source_execution_space_id.as_deref() == source_execution_space_id
        && existing.target_node_id == target_node_id
        && existing.target_execution_space_id.as_deref() == Some(target_execution_space_id)
        && existing.idempotency_key == idempotency_key
        && existing.ordering_key == ordering_key
        && existing.expected_target_revision == expected_target_revision
        && existing.body_schema == body_schema
        && existing.body == *body
        && existing.body_digest == harness_fabric::json_digest(body)?
        && existing
            .authorization_context
            .get("capability")
            .map(String::as_str)
            == Some(capability))
}

pub(super) fn control_plane_command(resolved: &ResolvedStore, args: &[String]) -> CliResult<()> {
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
    let store = Arc::new(
        layout
            .open_control_plane(&company_id)
            .map_err(fabric_error)?,
    );
    let collaboration_root = layout
        .collaboration_root(&company_id)
        .map_err(fabric_error)?;
    layout
        .open_collaboration_store(&company_id)
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
    let http_store = store.clone();
    let http_company = company_id.clone();
    let http_instance = instance_id.clone();
    let http_collaboration_root = collaboration_root.clone();
    std::thread::spawn(move || {
        if let Err(error) = serve_host_http(
            &http_addr,
            &trusted_origin,
            &host_token,
            &http_company,
            &http_instance,
            generation,
            http_store,
            artifact_key,
            capability_key,
            &ca,
            http_collaboration_root,
            http_stop,
        ) {
            eprintln!("Remote Fabric Host REST stopped: {error}");
        }
    });
    let heartbeat_stop = stop.clone();
    let heartbeat_store = store.clone();
    let heartbeat_company = company_id.clone();
    let heartbeat_instance = instance_id.clone();
    std::thread::spawn(move || {
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(&heartbeat_company, artifact_key);
        let control = ControlPlane::new(
            &heartbeat_company,
            &heartbeat_instance,
            &heartbeat_store,
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
        let session_store = store.clone();
        let company = company_id.clone();
        let instance = instance_id.clone();
        let collaboration_root = collaboration_root.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), FabricError> {
                let (mut socket, peer) = accept_control_plane_mtls(tcp, &tls)?;
                let keys = InMemoryArtifactKeyBackend::default();
                keys.insert(&company, artifact_key);
                let control =
                    ControlPlane::new(&company, &instance, &session_store, &keys, capability_key);
                let application = Wave6ControlPlaneApplication {
                    collaboration_root,
                    company_id: company.clone(),
                    actor_id: format!("control-plane:{instance}"),
                };
                serve_control_plane_session_with_application(
                    &mut socket,
                    &peer,
                    &control,
                    generation,
                    &application,
                )
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

pub(super) fn node_gateway_command(
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
    let node_id = crate::read_local_node_id()?;
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
    let claimed_schema_digest = required(args, "--schema-bundle-digest")?;
    let schema_digest = remote_fabric_schema_bundle_digest();
    if claimed_schema_digest != schema_digest {
        return Err(CliError::Usage(
            "--schema-bundle-digest must equal the digest of the schema bundle compiled into this exact firm build"
                .into(),
        ));
    }
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
            "cross-team-collaboration".into(),
        ]),
        build_sha: crate::build_git_rev().to_string(),
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
        gateway.heartbeat().map_err(|mut error| {
            error.message = format!("gateway heartbeat failed: {}", error.message);
            fabric_error(error)
        })?;
        loop {
            match gateway.apply_next(&local, &mut application) {
                Ok(receipt) => println!(
                    "Remote Fabric applied operation={} effect={:?}",
                    receipt.operation_id, receipt.application_effect
                ),
                Err(error) if error.code == FabricErrorCode::TargetOffline && error.retryable => {
                    break
                }
                Err(error)
                    if error.code == FabricErrorCode::TargetOffline
                        && error.message == "pending delivery batch is complete" =>
                {
                    break
                }
                Err(mut error) => {
                    error.message = format!("gateway pending delivery failed: {}", error.message);
                    return Err(fabric_error(error));
                }
            }
        }
        for mut operation in local.pending_outbox_operations().map_err(fabric_error)? {
            let now_unix_ms = now_unix_ms().map_err(fabric_error)?;
            if operation.expires_at_unix_ms <= now_unix_ms {
                if let Some(outbox) = local
                    .expire_unaccepted_outbox(&gateway.session, &operation.id, now_unix_ms)
                    .map_err(fabric_error)?
                {
                    println!(
                        "Remote Fabric settled unaccepted expired operation={} truth={}",
                        operation.id,
                        outbox
                            .terminal_receipt_ref
                            .as_deref()
                            .unwrap_or("local:not_applied:operation_expired")
                    );
                    continue;
                }
            }
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
