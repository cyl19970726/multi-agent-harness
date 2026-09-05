use super::*;

impl MultiTeamDaemon {
    /// Accept new control connections and advance every partial command once.
    /// Per-client framing and I/O failures never escape into the daemon loop.
    pub(super) fn poll_control_socket(
        self: &Arc<Self>,
        listener: &UnixListener,
        pending: &mut Vec<PendingControlConnection>,
        workers: &mut Vec<std::thread::JoinHandle<CliResult<()>>>,
    ) {
        let mut worker_index = 0;
        while worker_index < workers.len() {
            if workers[worker_index].is_finished() {
                let worker = workers.swap_remove(worker_index);
                self.observe_control_worker_result(worker.join(), "");
            } else {
                worker_index += 1;
            }
        }

        loop {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    if let Err(error) = stream.set_nonblocking(true) {
                        eprintln!("[node-daemon] cannot configure client socket: {error}");
                        continue;
                    }
                    pending.push(PendingControlConnection {
                        stream,
                        bytes: Vec::new(),
                        accepted_at: Instant::now(),
                    });
                }
                Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    eprintln!("[node-daemon] socket accept error: {error}");
                    break;
                }
            }
        }

        let mut index = 0;
        while index < pending.len() {
            let state = Self::read_control_command(&mut pending[index]);
            match state {
                ControlReadState::Pending => index += 1,
                ControlReadState::Closed => {
                    pending.swap_remove(index);
                }
                ControlReadState::Ready(command) => {
                    let mut connection = pending.swap_remove(index);
                    let command = command.trim().to_string();
                    let command_name = serde_json::from_str::<serde_json::Value>(&command)
                        .ok()
                        .and_then(|value| value["cmd"].as_str().map(str::to_string));
                    // Stop is admitted on the reserved control lane, but its
                    // answer is the drain result rather than the acceptance.
                    // Hold the client socket until `serve_loop` knows whether
                    // this generation actually drained (#584).
                    if command_name.as_deref() == Some("stop") {
                        match self.accept_stop_command(&mut connection.stream, command.as_str()) {
                            Ok(Some(daemon_generation)) => self
                                .deferred_stop_responses
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .push(DeferredStopResponse {
                                    stream: connection.stream,
                                    daemon_generation,
                                }),
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("[node-daemon] control client error: {error}")
                            }
                        }
                        continue;
                    }
                    // Status is deliberately a reserved control-lane command.
                    // It is a bounded registry read and must remain reachable
                    // even when every mutation worker is waiting on a
                    // provider-native effect.
                    if command_name.as_deref() == Some("status") {
                        if let Err(error) =
                            self.handle_control_command(&mut connection.stream, command.as_str())
                        {
                            eprintln!("[node-daemon] control client error: {error}");
                        }
                        continue;
                    }

                    let worker_limit = self.max_concurrency.saturating_mul(4).clamp(8, 64);
                    if workers.len() >= worker_limit {
                        let response = serde_json::json!({
                            "ok": false,
                            "error": "NODE_DAEMON_CONTROL_BUSY: bounded mutation workers are occupied; reconcile or retry the pre-effect request",
                            "retryable": true,
                        });
                        if let Err(error) =
                            Self::write_control_response(&mut connection.stream, &response)
                        {
                            eprintln!("[node-daemon] control client error: {error}");
                        }
                        continue;
                    }
                    let daemon = Arc::clone(self);
                    workers.push(std::thread::spawn(move || {
                        daemon.handle_control_command(&mut connection.stream, command.as_str())
                    }));
                }
                ControlReadState::Invalid(error) => {
                    let mut connection = pending.swap_remove(index);
                    let response = serde_json::json!({"ok": false, "error": error});
                    if let Err(write_error) =
                        Self::write_control_response(&mut connection.stream, &response)
                    {
                        eprintln!("[node-daemon] control client error: {write_error}");
                    }
                }
            }
        }
    }

    fn read_control_command(connection: &mut PendingControlConnection) -> ControlReadState {
        const MAX_CONTROL_BYTES: usize = 64 * 1024;
        let mut chunk = [0_u8; 4096];
        loop {
            match connection.stream.read(&mut chunk) {
                Ok(0) if connection.bytes.is_empty() => return ControlReadState::Closed,
                Ok(0) => {
                    return ControlReadState::Invalid(
                        "control command must be one newline-terminated JSON object",
                    )
                }
                Ok(count) => {
                    connection.bytes.extend_from_slice(&chunk[..count]);
                    if connection.bytes.len() > MAX_CONTROL_BYTES {
                        return ControlReadState::Invalid("control command exceeds 64 KiB");
                    }
                    if let Some(newline) = connection.bytes.iter().position(|byte| *byte == b'\n') {
                        return match String::from_utf8(connection.bytes[..newline].to_vec()) {
                            Ok(command) => ControlReadState::Ready(command),
                            Err(_) => {
                                ControlReadState::Invalid("control command is not valid UTF-8")
                            }
                        };
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if connection.accepted_at.elapsed() >= Duration::from_secs(1) {
                        return ControlReadState::Invalid(
                            "control command must be one newline-terminated JSON object",
                        );
                    }
                    return ControlReadState::Pending;
                }
                Err(_) => return ControlReadState::Closed,
            }
        }
    }

    pub(super) fn write_control_response(
        stream: &mut UnixStream,
        response: &serde_json::Value,
    ) -> CliResult<()> {
        // Accepted control sockets are nonblocking while the daemon collects
        // one complete request. A JSON response may be larger than the
        // socket's immediately available send buffer: `writeln!` can then
        // leave a valid prefix on the wire before returning WouldBlock, and
        // dropping the connection makes the client parse that prefix as an
        // EOF-truncated object. Keep the accepted AF_UNIX socket nonblocking
        // (switching it back to blocking is not portable on macOS) and drain
        // the complete bounded frame against an explicit deadline.
        let mut frame = serde_json::to_vec(response)
            .map_err(|error| control_response_delivery_error(std::io::Error::other(error)))?;
        frame.push(b'\n');
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut written = 0;
        while written < frame.len() {
            match stream.write(&frame[written..]) {
                Ok(0) => {
                    return Err(control_response_delivery_error(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "NodeDaemon control client closed before the response was complete",
                    )))
                }
                Ok(count) => written += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(control_response_delivery_error(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "NodeDaemon control response exceeded its 10s write deadline",
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(control_response_delivery_error(error)),
            }
        }
        Ok(())
    }

    fn managed_team_run_authority(
        &self,
        execution_space_id: &str,
        run_id: &str,
    ) -> CliResult<Option<(u64, String, u64)>> {
        Ok(self
            .contexts
            .lock()
            .map_err(|error| CliError::Usage(format!("context lock poisoned: {error}")))?
            .iter()
            .find(|context| {
                context.execution_space_id == execution_space_id && context.run_id == run_id
            })
            .map(|context| {
                (
                    context.daemon_generation,
                    context.supervisor_id.clone(),
                    context.supervisor_generation,
                )
            }))
    }

    fn already_managed_start_response(
        &self,
        execution_space_id: &str,
        run_id: &str,
    ) -> CliResult<Option<serde_json::Value>> {
        Ok(self
            .managed_team_run_authority(execution_space_id, run_id)?
            .map(
                |(daemon_generation, supervisor_id, supervisor_generation)| {
                    serde_json::json!({
                        "ok": true,
                        "already_managed": true,
                        "reused": true,
                        "daemon_id": self.daemon_id,
                        "daemon_generation": daemon_generation,
                        "execution_space_id": execution_space_id,
                        "run_id": run_id,
                        "supervisor_id": supervisor_id,
                        "supervisor_generation": supervisor_generation,
                    })
                },
            ))
    }

    /// Handle a single control socket command.
    pub(super) fn handle_control_command(
        &self,
        stream: &mut UnixStream,
        cmd_line: &str,
    ) -> CliResult<()> {
        let cmd: serde_json::Value = match serde_json::from_str(cmd_line) {
            Ok(v) => v,
            Err(e) => {
                let response = serde_json::json!({
                    "ok": false,
                    "error": format!("invalid json: {e}"),
                });
                Self::write_control_response(stream, &response)?;
                return Ok(());
            }
        };

        let cmd_name = cmd["cmd"].as_str().unwrap_or("");
        match cmd_name {
            #[cfg(test)]
            "test_block" => {
                let delay_ms = cmd["delay_ms"].as_u64().unwrap_or(250).min(2_000);
                std::thread::sleep(Duration::from_millis(delay_ms));
                Self::write_control_response(
                    stream,
                    &serde_json::json!({"ok": true, "delay_ms": delay_ms}),
                )?;
            }
            #[cfg(test)]
            "test_fail" => {
                return Err(CliError::Usage(
                    "TEST_ACCEPTED_CONTROL_FAILURE: simulated unresolved accepted command".into(),
                ));
            }
            "register_native_session_wake" => {
                let authority = cmd["authority"].as_str().unwrap_or("").trim();
                let token = cmd["token"].as_str().unwrap_or("").trim();
                let agent_member_id = cmd["agent_member_id"].as_str().unwrap_or("").trim();
                let expected_daemon_instance_id = cmd["expected_daemon_instance_id"]
                    .as_str()
                    .unwrap_or("")
                    .trim();
                let serve_instance_id = cmd["serve_instance_id"].as_str().unwrap_or("").trim();
                if !self.install_native_session_wake_endpoint(
                    authority,
                    token,
                    agent_member_id,
                    expected_daemon_instance_id,
                    serve_instance_id,
                ) {
                    Self::write_control_response(
                        stream,
                        &serde_json::json!({
                            "ok": false,
                            "error": "live provider activity sink registration requires the exact current daemon instance, a selected AgentMember, a loopback authority, and bounded callback capabilities"
                        }),
                    )?;
                    return Ok(());
                }
                Self::write_control_response(
                    stream,
                    &serde_json::json!({"ok": true, "registered": true}),
                )?;
            }
            "read_native_session" => {
                let request: crate::provider_event_api::PersistedSessionReadRequest =
                    match serde_json::from_value(cmd["request"].clone()) {
                        Ok(value) => value,
                        Err(error) => {
                            Self::write_control_response(
                                stream,
                                &serde_json::json!({
                                    "ok": false,
                                    "error": format!("INVALID_NATIVE_SESSION_READ: {error}")
                                }),
                            )?;
                            return Ok(());
                        }
                    };
                match crate::provider_event_api::read_persisted_session_for_daemon(
                    &self.firm_home,
                    &self.node_id,
                    &self.daemon_id,
                    request.node_daemon_generation,
                    Some(&self.instance_id),
                    &request,
                ) {
                    Ok(response) => Self::write_control_response(
                        stream,
                        &serde_json::json!({"ok": true, "response": response}),
                    )?,
                    Err(error) => Self::write_control_response(
                        stream,
                        &serde_json::json!({"ok": false, "error": error.to_string()}),
                    )?,
                }
            }
            "runtime" => {
                let envelope: harness_core::agentfirm_api::ControlCommandEnvelope =
                    match serde_json::from_value(cmd["envelope"].clone()) {
                        Ok(value) => value,
                        Err(error) => {
                            Self::write_control_response(
                                stream,
                                &serde_json::json!({
                                    "ok": false,
                                    "error": format!("INVALID_RUNTIME_COMMAND: {error}")
                                }),
                            )?;
                            return Ok(());
                        }
                    };
                if envelope.target_node_id != self.node_id
                    || envelope.target_node_daemon_id != self.daemon_id
                {
                    Self::write_control_response(
                        stream,
                        &serde_json::json!({
                            "ok": false,
                            "error": "NODE_DAEMON_GENERATION_FENCED: command targets another daemon"
                        }),
                    )?;
                    return Ok(());
                }
                let space = match crate::execution_space::context_for_id(
                    &self.firm_home,
                    &envelope.execution_space_id,
                )
                .map_err(|error| CliError::Usage(error.to_string()))?
                {
                    Some(space) => space,
                    None => {
                        Self::write_control_response(
                            stream,
                            &serde_json::json!({
                                "ok": false,
                                "error": "EXECUTION_SPACE_SCOPE_MISMATCH: Execution Space not registered"
                            }),
                        )?;
                        return Ok(());
                    }
                };
                let command_fingerprint =
                    harness_store::runtime_command_envelope_fingerprint(&envelope)?;
                let store = HarnessStore::new(&space.store_root);
                if let Err(error) = store.validate_runtime_command(&envelope, current_unix_ms_u64())
                {
                    Self::write_control_response(
                        stream,
                        &serde_json::json!({"ok": false, "error": error.to_string()}),
                    )?;
                    return Ok(());
                }
                let mutation = harness_core::agentfirm_api::MutationContext {
                    execution_space_id: envelope.execution_space_id.clone(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: self.daemon_id.clone(),
                    },
                    authority_actor: Some(envelope.authenticated_actor.clone()),
                    command_name: format!("runtime.{:?}", envelope.command).to_lowercase(),
                    idempotency_key: envelope.idempotency_key.clone(),
                    expected_version: envelope.expected_version,
                    request_fingerprint: Some(command_fingerprint),
                };
                let accepted_at = format!("unix-ms:{}", current_unix_ms_u64());
                let admission = match store.prepare_runtime_command(
                    &mutation,
                    &envelope,
                    current_unix_ms_u64(),
                    &accepted_at,
                ) {
                    Ok(admission) => admission,
                    Err(error) => {
                        Self::write_control_response(
                            stream,
                            &serde_json::json!({"ok": false, "error": error.to_string()}),
                        )?;
                        return Ok(());
                    }
                };
                if admission.replayed {
                    Self::write_control_response(
                        stream,
                        &serde_json::json!({"ok": true, "result": admission.projection.result, "replayed": true}),
                    )?;
                    return Ok(());
                }
                let effect_mutation = harness_core::agentfirm_api::MutationContext {
                    command_name: format!("{}.effect", mutation.command_name),
                    idempotency_key: format!("{}:effect", mutation.idempotency_key),
                    expected_version: envelope.expected_version,
                    ..mutation.clone()
                };
                // Set only after a provider-native process/thread may have
                // started. A later Store/registry failure is then Unknown,
                // never falsely reported as NotApplied.
                let mut provider_effect_started = false;
                let result = (|| -> CliResult<serde_json::Value> {
                    match envelope.command {
                    harness_core::agentfirm_api::RuntimeCommandKind::AuthorMessage => {
                        if envelope.required_capability != "message.author" {
                            Err(CliError::Usage(
                                "CAPABILITY_DENIED: author requires message.author".into(),
                            ))
                        } else {
                            serde_json::from_value::<harness_core::agentfirm_api::MessageDraft>(
                                envelope.payload["draft"].clone(),
                            )
                            .map_err(|error| {
                                CliError::Usage(format!("INVALID_RUNTIME_COMMAND: {error}"))
                            })
                            .and_then(|draft| {
                                if let Some(team_run_id) = draft.team_run_id.as_deref() {
                                    ensure_team_message_fabric(
                                        &store,
                                        team_run_id,
                                        &envelope.execution_space_id,
                                        &self.daemon_id,
                                        envelope.target_node_daemon_generation,
                                    )?;
                                }
                                let (sender_agent_member_id, sender_session_id) = if envelope
                                    .authenticated_actor
                                    .kind
                                    == harness_core::agentfirm_api::ActorKind::AgentMember
                                {
                                    let current = store
                                        .fabric_agent_sessions(&envelope.execution_space_id)
                                        .map_err(|error| CliError::Usage(error.to_string()))?
                                        .into_iter()
                                        .filter(|session| {
                                            session.agent_member_id
                                                == envelope.authenticated_actor.id
                                                && session.node_id == self.node_id
                                                && session.node_daemon_id == self.daemon_id
                                                && session.node_daemon_generation
                                                    == envelope.target_node_daemon_generation
                                                && session.lifecycle
                                                    != harness_core::agentfirm_api::AgentSessionStatus::Closed
                                        })
                                        .collect::<Vec<_>>();
                                    match current.as_slice() {
                                        [session] => (
                                            Some(envelope.authenticated_actor.id.clone()),
                                            Some(session.id.clone()),
                                        ),
                                        [] => {
                                            let team_run_id = draft.team_run_id.as_deref().ok_or_else(|| {
                                                CliError::Usage("AGENT_SESSION_AMBIGUOUS: sessionless AgentMember author requires an exact external Host TeamRun".into())
                                            })?;
                                            let run = crate::latest_team_run(&store, team_run_id)?;
                                            let exact_host = store.exact_team_run_host_actor(team_run_id)?;
                                            if run.host_control_mode
                                                != harness_core::HostControlMode::ExternalInteractive
                                                || exact_host.id != envelope.authenticated_actor.id
                                            {
                                                return Err(CliError::Usage(
                                                    "AGENT_SESSION_AMBIGUOUS: message author requires one exact current local session or the exact sessionless external Host identity".into(),
                                                ));
                                            }
                                            // External Host authoring is an authenticated
                                            // coordination-plane effect. It preserves the
                                            // AgentMember actor but never fabricates a sender
                                            // AgentSession or provider receipt.
                                            (None, None)
                                        }
                                        _ => {
                                            return Err(CliError::Usage(
                                                "AGENT_SESSION_AMBIGUOUS: message author has multiple current local sessions".into(),
                                            ));
                                        }
                                    }
                                } else {
                                    (None, None)
                                };
                                // The immutable transfer contract hashes the
                                // exact Message body bytes. Hashing a wrapper
                                // JSON object here makes a valid source-authored
                                // Message unverifiable on the target Node.
                                let body_digest = format!(
                                    "sha256:{}",
                                    harness_fabric::sha256_hex(draft.body.as_bytes())
                                );
                                let fingerprint = harness_store::canonical_json_fingerprint(
                                    &serde_json::json!({
                                        "sender_actor_ref": envelope.authenticated_actor,
                                        "sender_agent_member_id": sender_agent_member_id,
                                        "sender_session_id": sender_session_id,
                                        "address_kind": draft.address_kind,
                                        "target_ref": draft.target_ref,
                                        "recipients": draft.recipients,
                                        "team_id": draft.team_id,
                                        "team_run_id": draft.team_run_id,
                                        "work_id": draft.work_id,
                                        "collaboration_scope": draft.collaboration_scope,
                                        "kind": draft.kind,
                                        "body": draft.body,
                                        "body_digest": body_digest,
                                        "correlation_id": draft.correlation_id,
                                        "causation_id": draft.causation_id,
                                        "response_intent": draft.response_intent,
                                        "evidence_refs": draft.evidence_refs,
                                        "schema_version": draft.schema_version,
                                        "idempotency_key": envelope.idempotency_key,
                                    }),
                                );
                                let admission_authority = if let Some(value) = envelope
                                    .payload
                                    .get("message_admission_authority")
                                    .filter(|value| !value.is_null())
                                {
                                    Some(serde_json::from_value::<
                                        harness_core::collaboration::MessageAdmissionAuthority,
                                    >(value.clone()).map_err(|error| {
                                        CliError::Usage(format!(
                                            "INVALID_MESSAGE_ADMISSION_AUTHORITY: {error}"
                                        ))
                                    })?)
                                } else {
                                    // Serialized migration compatibility for
                                    // already-prepared WorkDelegation commands.
                                    envelope
                                        .payload
                                        .get("delegation_authority")
                                        .filter(|value| !value.is_null())
                                        .map(|value| {
                                            serde_json::from_value::<
                                                harness_core::collaboration::CollaborationMessageAuthority,
                                            >(value.clone())
                                            .map(harness_core::collaboration::MessageAdmissionAuthority::WorkDelegation)
                                            .map_err(|error| {
                                                CliError::Usage(format!(
                                                    "INVALID_COLLABORATION_MESSAGE_AUTHORITY: {error}"
                                                ))
                                            })
                                        })
                                        .transpose()?
                                };
                                store
                                    .author_message_with_admission_authority(
                                        &effect_mutation,
                                        harness_core::agentfirm_api::Message {
                                            id: format!("message:{}", envelope.idempotency_key),
                                            source_execution_space_id: envelope.execution_space_id.clone(),
                                            source_node_id: self.node_id.clone(),
                                            source_node_daemon_id: self.daemon_id.clone(),
                                            source_authority_generation: envelope.target_node_daemon_generation,
                                            sender_actor_ref: envelope.authenticated_actor.clone(),
                                            sender_agent_member_id,
                                            sender_session_id,
                                            address_kind: draft.address_kind,
                                            target_ref: draft.target_ref,
                                            recipients: draft.recipients,
                                            team_id: draft.team_id,
                                            team_run_id: draft.team_run_id,
                                            work_id: draft.work_id,
                                            collaboration_scope: draft.collaboration_scope,
                                            kind: draft.kind,
                                            body: draft.body,
                                            body_digest,
                                            correlation_id: draft.correlation_id,
                                            causation_id: draft.causation_id,
                                            response_intent: draft.response_intent,
                                            evidence_refs: draft.evidence_refs,
                                            content_fingerprint: fingerprint,
                                            schema_version: draft.schema_version,
                                            idempotency_key: envelope.idempotency_key.clone(),
                                            created_at: accepted_at.clone(),
                                        },
                                        admission_authority.as_ref(),
                                    )
                                    .map_err(|error| CliError::Usage(error.to_string()))
                                    .and_then(|result| {
                                        serde_json::to_value(result.projection)
                                            .map_err(CliError::Json)
                                    })
                            })
                        }
                    }
                    harness_core::agentfirm_api::RuntimeCommandKind::StartSession => {
                        if envelope.required_capability != "agent_session.start" {
                            Err(CliError::Usage(
                                "CAPABILITY_DENIED: start requires agent_session.start".into(),
                            ))
                        } else {
                            serde_json::from_value::<harness_core::agentfirm_api::AgentSession>(
                                envelope.payload["session"].clone(),
                            )
                            .map_err(|error| {
                                CliError::Usage(format!("INVALID_RUNTIME_COMMAND: {error}"))
                            })
                            .and_then(|mut session| {
                                let display_name = store
                                    .fabric_agent_identities(&envelope.execution_space_id)
                                    .map_err(|error| CliError::Usage(error.to_string()))?
                                    .into_iter()
                                    .find(|identity| identity.id == session.agent_member_id)
                                    .map(|identity| identity.display_name)
                                    .ok_or_else(|| {
                                        CliError::Usage("AGENT_IDENTITY_NOT_FOUND".into())
                                    })?;
                                let opened = crate::provider_adapter::open_node_session(
                                    &session,
                                    &space.store_root,
                                    &display_name,
                                )
                                .map_err(CliError::Usage)?;
                                provider_effect_started = true;
                                let runtime_provider = opened.runtime.provider().to_string();
                                let runtime_native_session_id =
                                    opened.runtime.native_session_id().to_string();
                                session.native_session_ref =
                                    Some(opened.native_session_ref.clone());
                                let session_id = session.id.clone();
                                let result = store
                                    .create_agent_session(&effect_mutation, session)
                                    .map_err(|error| CliError::Usage(error.to_string()))?;
                                let mut runtimes = self.session_runtimes.lock().map_err(|_| {
                                    CliError::Usage(
                                        "RUNTIME_COMMAND_RECOVERY_REQUIRED: provider runtime registry poisoned"
                                            .into(),
                                    )
                                })?;
                                if runtimes.insert(session_id, opened.runtime).is_some() {
                                    return Err(CliError::Usage(
                                        "RUNTIME_COMMAND_RECOVERY_REQUIRED: duplicate provider runtime handle"
                                            .into(),
                                    ));
                                }
                                serde_json::to_value(serde_json::json!({
                                    "session": result.projection,
                                    "provider": opened.permission_mapping.provider,
                                    "runtime_provider": runtime_provider,
                                    "runtime_native_session_id": runtime_native_session_id,
                                    "permission": opened.permission_mapping,
                                    "native_session": opened.native_session_ref,
                                }))
                                .map_err(CliError::Json)
                            })
                        }
                    }
                    harness_core::agentfirm_api::RuntimeCommandKind::StopSession
                    | harness_core::agentfirm_api::RuntimeCommandKind::ResumeSession => {
                        let required = if matches!(
                            envelope.command,
                            harness_core::agentfirm_api::RuntimeCommandKind::StopSession
                        ) {
                            "agent_session.stop"
                        } else {
                            "agent_session.resume"
                        };
                        if envelope.required_capability != required {
                            Err(CliError::Usage(format!(
                                "CAPABILITY_DENIED: command requires {required}"
                            )))
                        } else {
                            let session_id = envelope.payload["session_id"]
                                .as_str()
                                .ok_or_else(|| {
                                    CliError::Usage(
                                        "INVALID_RUNTIME_COMMAND: session_id is required".into(),
                                    )
                                })?;
                            let stopping = matches!(
                                envelope.command,
                                harness_core::agentfirm_api::RuntimeCommandKind::StopSession
                            );
                            let next = if stopping {
                                harness_core::agentfirm_api::AgentSessionStatus::Closed
                            } else {
                                harness_core::agentfirm_api::AgentSessionStatus::Cold
                            };
                            let session = store
                                .fabric_agent_sessions(&envelope.execution_space_id)
                                .map_err(|error| CliError::Usage(error.to_string()))?
                                .into_iter()
                                .find(|session| session.id == session_id)
                                .ok_or_else(|| CliError::Usage("AGENT_SESSION_NOT_FOUND".into()))?;
                            let capabilities = crate::provider_adapter::node_session_capabilities(
                                &session.provider_kind,
                            )
                            .ok_or_else(|| {
                                CliError::Usage(format!(
                                    "PROVIDER_CAPABILITY_UNPROVABLE: {}",
                                    session.provider_kind
                                ))
                            })?;
                            if (stopping && !capabilities.stop)
                                || (!stopping && !capabilities.resume)
                            {
                                return Err(CliError::Usage(format!(
                                    "PROVIDER_RUNTIME_UNSUPPORTED: {} cannot {:?} through the NodeDaemon session adapter",
                                    session.provider_kind, envelope.command
                                )));
                            }
                            let has_runtime = self
                                .session_runtimes
                                .lock()
                                .map_err(|_| {
                                    CliError::Usage(
                                        "RUNTIME_COMMAND_RECOVERY_REQUIRED: provider runtime registry poisoned"
                                            .into(),
                                    )
                                })?
                                .contains_key(session_id);
                            if stopping && !has_runtime {
                                return Err(CliError::Usage(
                                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: NodeDaemon has no exact provider handle for StopSession"
                                        .into(),
                                ));
                            }
                            if !stopping && has_runtime {
                                // Resume against the exact live NodeDaemon
                                // handle is a truthful, idempotent liveness
                                // confirmation. It must not reopen the native
                                // provider thread or rewrite lifecycle state.
                                return serde_json::to_value(session).map_err(CliError::Json);
                            }
                            let mut resumed_runtime = None;
                            if !stopping && !has_runtime {
                                let display_name = store
                                    .fabric_agent_identities(&envelope.execution_space_id)
                                    .map_err(|error| CliError::Usage(error.to_string()))?
                                    .into_iter()
                                    .find(|identity| identity.id == session.agent_member_id)
                                    .map(|identity| identity.display_name)
                                    .ok_or_else(|| {
                                        CliError::Usage("AGENT_IDENTITY_NOT_FOUND".into())
                                    })?;
                                let opened = crate::provider_adapter::open_node_session(
                                    &session,
                                    &space.store_root,
                                    &display_name,
                                )
                                .map_err(CliError::Usage)?;
                                provider_effect_started = true;
                                resumed_runtime = Some(opened.runtime);
                            }
                            let session_mutation = harness_core::agentfirm_api::MutationContext {
                                expected_version: session.version,
                                ..effect_mutation.clone()
                            };
                            let transitioned = store
                                .transition_agent_session(
                                    &session_mutation,
                                    session_id,
                                    next,
                                    &format!("unix-ms:{}", current_unix_ms_u64()),
                                )
                                .map_err(|error| CliError::Usage(error.to_string()))?;
                            if stopping {
                                self.session_runtimes
                                    .lock()
                                    .map_err(|_| {
                                        CliError::Usage(
                                            "RUNTIME_COMMAND_RECOVERY_REQUIRED: provider runtime registry poisoned"
                                                .into(),
                                        )
                                    })?
                                    .remove(session_id);
                            } else if let Some(runtime) = resumed_runtime {
                                let replaced = self
                                    .session_runtimes
                                    .lock()
                                    .map_err(|_| {
                                        CliError::Usage(
                                            "RUNTIME_COMMAND_RECOVERY_REQUIRED: provider runtime registry poisoned"
                                                .into(),
                                        )
                                    })?
                                    .insert(session_id.to_string(), runtime);
                                if replaced.is_some() {
                                    return Err(CliError::Usage(
                                        "RUNTIME_COMMAND_RECOVERY_REQUIRED: concurrent provider runtime appeared during ResumeSession"
                                            .into(),
                                    ));
                                }
                            }
                            serde_json::to_value(transitioned.projection).map_err(CliError::Json)
                        }
                    }
                    harness_core::agentfirm_api::RuntimeCommandKind::DispatchProvider => {
                        if envelope.required_capability != "provider.dispatch" {
                            Err(CliError::Usage(
                                "CAPABILITY_DENIED: dispatch requires provider.dispatch".into(),
                            ))
                        } else {
                            let delivery_id = envelope.payload["delivery_id"]
                                .as_str()
                                .ok_or_else(|| {
                                    CliError::Usage(
                                        "INVALID_RUNTIME_COMMAND: delivery_id is required".into(),
                                    )
                                })?;
                            let claim_id = envelope.payload["claim_id"].as_str().ok_or_else(|| {
                                CliError::Usage(
                                    "INVALID_RUNTIME_COMMAND: claim_id is required".into(),
                                )
                            })?;
                            let requested_mode = serde_json::from_value(
                                envelope.payload["dispatch_mode"].clone(),
                            )
                            .map_err(|error| {
                                CliError::Usage(format!("INVALID_RUNTIME_COMMAND: {error}"))
                            })?;
                            let session_id = envelope.payload["session_id"].as_str().ok_or_else(|| {
                                CliError::Usage("INVALID_RUNTIME_COMMAND: session_id is required".into())
                            })?;
                            let session = store
                                .fabric_agent_sessions(&envelope.execution_space_id)
                                .map_err(|error| CliError::Usage(error.to_string()))?
                                .into_iter()
                                .find(|session| session.id == session_id)
                                .ok_or_else(|| CliError::Usage("AGENT_SESSION_NOT_FOUND".into()))?;
                            crate::provider_adapter::map_permission(
                                &session.provider_kind,
                                session.effective_permission_ceiling,
                            )
                            .map_err(CliError::Usage)?;
                            let dispatch_mode = crate::provider_adapter::effective_delivery_mode(
                                &session.provider_kind,
                                requested_mode,
                                session.lifecycle,
                                false,
                            )
                            .map_err(CliError::Usage)?;
                            store
                                .claim_message_for_provider(
                                    &effect_mutation,
                                    delivery_id,
                                    &self.node_id,
                                    &self.daemon_id,
                                    envelope.target_node_daemon_generation,
                                    claim_id,
                                    dispatch_mode,
                                    &format!("unix-ms:{}", current_unix_ms_u64()),
                                )
                                .map_err(|error| CliError::Usage(error.to_string()))
                                .and_then(|result| {
                                    serde_json::to_value(result.projection).map_err(CliError::Json)
                                })
                        }
                    }
                    harness_core::agentfirm_api::RuntimeCommandKind::CancelProviderTurn => {
                        Err(CliError::Usage(
                            "RUNTIME_COMMAND_UNSUPPORTED: provider adapter has no proven cancel capability"
                                .into(),
                        ))
                    }
                    command => Err(CliError::Usage(format!(
                        "RUNTIME_COMMAND_UNSUPPORTED: semantic command {command:?} is admitted only when an executable runtime adapter binding implements it"
                    ))),
                }
                })();
                let settled_at = format!("unix-ms:{}", current_unix_ms_u64());
                let (status, certainty, settled_result, failure_code) = match &result {
                    Ok(value) => (
                        harness_core::agentfirm_api::RuntimeCommandStatus::Applied,
                        harness_core::agentfirm_api::RuntimeEffectCertainty::Applied,
                        Some(value.clone()),
                        None,
                    ),
                    Err(error) => (
                        if provider_effect_started {
                            harness_core::agentfirm_api::RuntimeCommandStatus::RecoveryRequired
                        } else {
                            harness_core::agentfirm_api::RuntimeCommandStatus::Failed
                        },
                        if provider_effect_started {
                            harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown
                        } else {
                            harness_core::agentfirm_api::RuntimeEffectCertainty::NotApplied
                        },
                        None,
                        Some(error.to_string()),
                    ),
                };
                let settle_context = harness_core::agentfirm_api::MutationContext {
                    command_name: format!("{}.settle", mutation.command_name),
                    idempotency_key: format!("{}:settle", mutation.idempotency_key),
                    expected_version: admission.projection.version,
                    request_fingerprint: None,
                    ..mutation
                };
                if let Err(error) = store.settle_runtime_command(
                    &settle_context,
                    &envelope.id,
                    status,
                    certainty,
                    settled_result,
                    failure_code,
                    &settled_at,
                ) {
                    Self::write_control_response(
                        stream,
                        &serde_json::json!({
                            "ok": false,
                            "error": format!("RUNTIME_COMMAND_RECOVERY_REQUIRED: {error}")
                        }),
                    )?;
                    return Ok(());
                }
                match result {
                    Ok(value) => Self::write_control_response(
                        stream,
                        &serde_json::json!({"ok": true, "result": value}),
                    )?,
                    Err(error) => Self::write_control_response(
                        stream,
                        &serde_json::json!({"ok": false, "error": error.to_string()}),
                    )?,
                }
            }
            "start" => {
                let run_id = cmd["run_id"].as_str().unwrap_or("");
                let execution_space_id = cmd["execution_space_id"].as_str().unwrap_or("");
                if run_id.is_empty() || execution_space_id.is_empty() {
                    let response = serde_json::json!({
                        "ok": false,
                        "error": "execution_space_id and run_id are required"
                    });
                    Self::write_control_response(stream, &response)?;
                    return Ok(());
                }
                if let Some(response) =
                    self.already_managed_start_response(execution_space_id, run_id)?
                {
                    Self::write_control_response(stream, &response)?;
                    return Ok(());
                }
                let space =
                    crate::execution_space::context_for_id(&self.firm_home, execution_space_id)
                        .map_err(|error| {
                            CliError::Usage(format!(
                                "cannot resolve Execution Space {execution_space_id}: {error}"
                            ))
                        })?
                        .ok_or_else(|| {
                            CliError::Usage(format!(
                                "Execution Space not found: {execution_space_id}"
                            ))
                        })?;
                let store = HarnessStore::new(space.store_root.clone());
                match self.start_supervising(space, store.clone(), run_id) {
                    Ok(()) => {
                        let authority =
                            self.managed_team_run_authority(execution_space_id, run_id)?;
                        if let Some((_, supervisor_id, supervisor_generation)) = &authority {
                            if let Err(error) = self.clear_team_run_supervisor_recovery(
                                execution_space_id,
                                &store,
                                run_id,
                                supervisor_id,
                                *supervisor_generation,
                            ) {
                                eprintln!(
                                    "[node-daemon] Supervisor recovery marker settlement deferred for {execution_space_id}/{run_id}: {error}"
                                );
                            }
                        }
                        let (daemon_generation, supervisor_id, supervisor_generation) =
                            authority.ok_or_else(|| {
                                CliError::Usage(format!(
                                    "TEAM_RUN_START_RESULT_UNKNOWN: NodeDaemon started {execution_space_id}/{run_id} without registering its managed authority"
                                ))
                            })?;
                        let response = serde_json::json!({
                            "ok": true,
                            "already_managed": false,
                            "reused": false,
                            "daemon_id": self.daemon_id,
                            "daemon_generation": daemon_generation,
                            "execution_space_id": execution_space_id,
                            "run_id": run_id,
                            "supervisor_id": supervisor_id,
                            "supervisor_generation": supervisor_generation,
                        });
                        Self::write_control_response(stream, &response)?;
                    }
                    Err(e) => {
                        // Recovery discovery and an explicit Start request run
                        // concurrently. The scanner may finish adopting this
                        // exact TeamRun after the fast `already_managed` check
                        // above but before `start_supervising` acquires the
                        // context lock. In that case the requested effect is
                        // already true under this daemon generation, so report
                        // an idempotent reuse instead of turning a successful
                        // recovery into a client-visible rejection.
                        if let Some(response) =
                            self.already_managed_start_response(execution_space_id, run_id)?
                        {
                            Self::write_control_response(stream, &response)?;
                            return Ok(());
                        }
                        self.block_start_failure_if_unresolved(
                            execution_space_id,
                            &store,
                            run_id,
                            &e,
                        );
                        let response = serde_json::json!({
                            "ok": false,
                            "execution_space_id": execution_space_id,
                            "run_id": run_id,
                            "error": e.to_string(),
                        });
                        Self::write_control_response(stream, &response)?;
                    }
                }
            }
            "status" => {
                let runs: Vec<serde_json::Value> = {
                    let contexts = self
                        .contexts
                        .lock()
                        .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
                    contexts
                        .iter()
                        .map(|ctx| {
                            let is_finished =
                                ctx.thread.as_ref().map(|t| t.is_finished()).unwrap_or(true);
                            let serving_status = ctx
                                .serving_status
                                .lock()
                                .unwrap_or_else(|error| error.into_inner());
                            serde_json::json!({
                                "execution_space_id": ctx.execution_space_id,
                                "project_binding_id": ctx.project_binding_id,
                                "run_id": ctx.run_id,
                                "daemon_generation": ctx.daemon_generation,
                                "supervisor_id": ctx.supervisor_id,
                                "supervisor_generation": ctx.supervisor_generation,
                                "status": if is_finished { "finished" } else { serving_status.as_str() },
                                "elapsed_secs": ctx.started_at.elapsed().as_secs(),
                            })
                        })
                        .collect()
                };
                let resp = serde_json::json!({
                    "ok": true,
                    "node_id": self.node_id,
                    "daemon_id": self.daemon_id,
                    "instance_id": self.instance_id,
                    "process_id": std::process::id(),
                    "log_path": crate::daemon_cli::node_daemon_log_path(
                        &self.firm_home,
                        &self.node_id,
                    ),
                    "native_session_wake_sink_registered": !self
                        .native_session_wake_endpoint
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .is_empty(),
                    // Known limit: this reports only the process-local holds.
                    // A durable `team_supervisor_no_progress` hold is not
                    // listed, because surfacing it means reading
                    // `member_actions` and re-fingerprinting every Running run
                    // across every registered Execution Space — whole-Store
                    // scans on the one control lane that must stay answerable
                    // while discovery and reap are busy (#671). Read those
                    // holds from the TeamRun's MemberAction journal instead.
                    "recovery_blocked_runs": self.recovery_blocked_runs
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .iter()
                        .map(|((space, run), hold)| serde_json::json!({
                            "execution_space_id": space,
                            "run_id": run,
                            "lifted_by": match hold {
                                VolatileAdoptionHold::Unconditional => "explicit_start",
                                VolatileAdoptionHold::AtCanonicalState(_) => "canonical_change",
                            },
                        }))
                        .collect::<Vec<_>>(),
                    "runs": runs
                });
                Self::write_control_response(stream, &resp)?;
            }
            _ => {
                let response = serde_json::json!({
                    "ok": false,
                    "error": format!("unknown command: {cmd_name}"),
                });
                Self::write_control_response(stream, &response)?;
            }
        }
        Ok(())
    }
}

impl MultiTeamDaemon {
    /// Validate and admit one `stop` request without answering it.
    ///
    /// `Ok(Some(generation))` means the request was accepted and its response
    /// is owed once the drain result exists; `Ok(None)` means a rejection was
    /// already written and the connection is finished. Stop must never report
    /// success from acceptance alone: `daemon status` went absent while the
    /// exact serve process still spun at ~200% CPU precisely because the
    /// acceptance was the answer (#584).
    pub(super) fn accept_stop_command(
        &self,
        stream: &mut UnixStream,
        cmd_line: &str,
    ) -> CliResult<Option<u64>> {
        let cmd: serde_json::Value = match serde_json::from_str(cmd_line) {
            Ok(value) => value,
            Err(error) => {
                Self::write_control_response(
                    stream,
                    &serde_json::json!({
                        "ok": false,
                        "error": format!("invalid json: {error}"),
                    }),
                )?;
                return Ok(None);
            }
        };
        let execution_space_id = cmd["execution_space_id"].as_str().unwrap_or("");
        let Some(daemon_generation) = cmd["daemon_generation"].as_u64() else {
            Self::write_control_response(
                stream,
                &serde_json::json!({
                    "ok": false,
                    "error": "execution_space_id and daemon_generation are required"
                }),
            )?;
            return Ok(None);
        };
        if execution_space_id.is_empty() {
            Self::write_control_response(
                stream,
                &serde_json::json!({
                    "ok": false,
                    "error": "execution_space_id and daemon_generation are required"
                }),
            )?;
            return Ok(None);
        }
        let space = crate::execution_space::context_for_id(&self.firm_home, execution_space_id)
            .map_err(|error| CliError::Usage(error.to_string()))?
            .ok_or_else(|| {
                CliError::Usage(format!("Execution Space not found: {execution_space_id}"))
            })?;
        let store = HarnessStore::new(space.store_root);
        let now_ms = current_unix_ms_u64();
        let lease = store.latest_node_daemon_lease(&self.node_id)?;
        let authorized = daemon_control_generation_authorized(
            lease.as_ref(),
            &self.daemon_id,
            &self.instance_id,
            daemon_generation,
            now_ms,
        );
        if !authorized {
            Self::write_control_response(
                stream,
                &serde_json::json!({
                    "ok": false,
                    "error": "SUPERVISOR_GENERATION_FENCED: stop does not match this live NodeDaemon generation"
                }),
            )?;
            return Ok(None);
        }
        self.stop_requested.store(true, Ordering::SeqCst);
        Ok(Some(daemon_generation))
    }
}

const CONTROL_RESPONSE_DELIVERY_FAILED: &str = "NODE_DAEMON_RESPONSE_DELIVERY_FAILED:";

fn control_response_delivery_error(error: std::io::Error) -> CliError {
    CliError::Usage(format!("{CONTROL_RESPONSE_DELIVERY_FAILED} {error}"))
}

fn is_control_response_delivery_error(error: &CliError) -> bool {
    matches!(error, CliError::Usage(message) if message.starts_with(CONTROL_RESPONSE_DELIVERY_FAILED))
}

impl MultiTeamDaemon {
    pub(super) fn observe_control_worker_result(
        &self,
        result: std::thread::Result<CliResult<()>>,
        phase: &str,
    ) {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) if is_control_response_delivery_error(&error) => {
                eprintln!(
                    "[node-daemon] accepted control command completed but its response was not delivered {phase}: {error}"
                );
            }
            Ok(Err(error)) => {
                self.control_worker_failed.store(true, Ordering::SeqCst);
                eprintln!("[node-daemon] control worker failed {phase}: {error}");
            }
            Err(_) => {
                self.control_worker_failed.store(true, Ordering::SeqCst);
                eprintln!("[node-daemon] control worker panicked {phase}");
            }
        }
    }
}
