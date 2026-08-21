use super::*;

impl HttpExchange<'_> {
    #[allow(unused_variables)]
    pub(super) fn handle_trust_routes(&mut self) -> CliResult<bool> {
        let projects = self.projects;
        let mut stream = &mut *self.stream;
        let sse_manager = self.sse_manager.clone();
        let method = self.method.as_str();
        let path = &self.path;
        let path_only = &self.path_only;
        let project_param = &self.project_param;
        let project_id = &self.project_id;
        let store_owned = &self.store;
        let store = store_owned;
        let company_os_path = self.company_os_path;
        let body = &self.body;
        let trust_transport_token = &self.trust_transport_token;
        let trust_idempotency_key = self.trust_idempotency_key.clone();
        let trust_expected_version = self.trust_expected_version;
        let trust_confirmed_action = &self.trust_confirmed_action;
        let trust_identity_override_header = self.trust_identity_override_header;
        let live_provider_activity_token = &self.live_provider_activity_token;
        if method == "POST" && role_actions_api::is_retired_legacy_write_path(&path_only) {
            write_http_json(
                &mut stream,
                "410 Gone",
                &serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": "RETIRED_WRITE_AUTHORITY",
                        "message": "legacy Work and WorkDelegation HTTP writers are retired; use the authenticated AgentFirm role-action or canonical acceptance service"
                    }
                }),
            )?;
            return Ok(true);
        }
        let provider_message_answer =
            path_only
                .strip_prefix("/v1/team-runs/")
                .is_some_and(|rest| {
                    matches!(
                        rest.split('/').collect::<Vec<_>>().as_slice(),
                        [_, "messages", _, "answer"]
                    )
                });
        let retired_message_write = method == "POST"
            && !provider_message_answer
            && (path_only == "/v1/messages"
                || (path_only.starts_with("/v1/team-runs/")
                    && (path_only.ends_with("/messages") || path_only.contains("/messages/")))
                || path_only.starts_with("/v1/message-deliveries/"));
        if retired_message_write {
            write_http_json(
                &mut stream,
                "410 Gone",
                &serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": "RETIRED_WRITE_AUTHORITY",
                        "message": "run-addressed Team message writers are retired; author Message through the source NodeDaemon RuntimeCommand and consume identity-first Delivery"
                    }
                }),
            )?;
            return Ok(true);
        }
        if method == "POST" && path_only == "/v1/collaboration/delegations" {
            if trust_identity_override_header {
                write_http_json(
                    &mut stream,
                    "401 Unauthorized",
                    &serde_json::json!({"ok":false,"error":{"code":"UNAUTHORIZED_ACTOR","message":"request headers cannot select collaboration actor or authority identity"}}),
                )?;
                return Ok(true);
            }
            let credential = match resolve_agentfirm_http_credential(
                trust_transport_token.as_deref(),
            ) {
                Ok(value) => value,
                Err(message) => {
                    write_http_json(
                        &mut stream,
                        "401 Unauthorized",
                        &serde_json::json!({"ok":false,"error":{"code":"UNAUTHORIZED_ACTOR","message":message}}),
                    )?;
                    return Ok(true);
                }
            };
            let Some(idempotency_key) = trust_idempotency_key
                .as_deref()
                .filter(|key| !key.trim().is_empty())
            else {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok":false,"error":{"code":"IDEMPOTENCY_CONFLICT","message":"Idempotency-Key is required"}}),
                )?;
                return Ok(true);
            };
            if trust_expected_version != Some(0) {
                write_http_json(
                    &mut stream,
                    "409 Conflict",
                    &serde_json::json!({"ok":false,"error":{"code":"EXPECTED_REVISION_CONFLICT","message":"new Delegation proposal requires If-Match: 0"}}),
                )?;
                return Ok(true);
            }
            let request = match serde_json::from_slice::<
                fabric_runtime::QueueCollaborationProposalRequest,
            >(&body)
            {
                Ok(value) => value,
                Err(error) => {
                    write_http_json(
                        &mut stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok":false,"error":{"code":"INVALID_PAYLOAD","message":error.to_string()}}),
                    )?;
                    return Ok(true);
                }
            };
            let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
            let local_node_id = read_local_node_id()?;
            match fabric_runtime::queue_collaboration_proposal(
                &store_owned,
                &firm_home,
                &project_id,
                &local_node_id,
                &credential,
                idempotency_key,
                &request,
                current_unix_ms_u64(),
            ) {
                Ok(value) => write_http_json(
                    &mut stream,
                    "202 Accepted",
                    &serde_json::json!({"ok":true,"queued":value}),
                )?,
                Err(error) => {
                    let status = match error.code {
                        harness_fabric::FabricErrorCode::UnauthorizedActor => "403 Forbidden",
                        harness_fabric::FabricErrorCode::ExpectedRevisionConflict => "409 Conflict",
                        _ => "400 Bad Request",
                    };
                    write_http_json(
                        &mut stream,
                        status,
                        &serde_json::json!({"ok":false,"error":{"code":format!("{:?}",error.code).to_ascii_uppercase(),"message":error.message}}),
                    )?;
                }
            }
            return Ok(true);
        }
        let collaboration_publication_delegation = path_only
            .strip_prefix("/v1/collaboration/delegations/")
            .and_then(|suffix| suffix.strip_suffix("/publications"))
            .filter(|delegation_id| !delegation_id.is_empty() && !delegation_id.contains('/'));
        if method == "POST" && collaboration_publication_delegation.is_some() {
            if trust_identity_override_header {
                write_http_json(
                    &mut stream,
                    "401 Unauthorized",
                    &serde_json::json!({"ok":false,"error":{"code":"UNAUTHORIZED_ACTOR","message":"request headers cannot select collaboration actor or authority identity"}}),
                )?;
                return Ok(true);
            }
            let credential = match resolve_agentfirm_http_credential(
                trust_transport_token.as_deref(),
            ) {
                Ok(value) => value,
                Err(message) => {
                    write_http_json(
                        &mut stream,
                        "401 Unauthorized",
                        &serde_json::json!({"ok":false,"error":{"code":"UNAUTHORIZED_ACTOR","message":message}}),
                    )?;
                    return Ok(true);
                }
            };
            let Some(idempotency_key) = trust_idempotency_key
                .as_deref()
                .filter(|key| !key.trim().is_empty())
            else {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok":false,"error":{"code":"IDEMPOTENCY_CONFLICT","message":"Idempotency-Key is required"}}),
                )?;
                return Ok(true);
            };
            let Some(expected_revision) = trust_expected_version.filter(|version| *version > 0)
            else {
                write_http_json(
                    &mut stream,
                    "409 Conflict",
                    &serde_json::json!({"ok":false,"error":{"code":"EXPECTED_REVISION_CONFLICT","message":"If-Match exact Delegation revision is required"}}),
                )?;
                return Ok(true);
            };
            let request = match serde_json::from_slice::<
                fabric_runtime::QueueRemoteFactPublicationRequest,
            >(&body)
            {
                Ok(value) => value,
                Err(error) => {
                    write_http_json(
                        &mut stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok":false,"error":{"code":"INVALID_PAYLOAD","message":error.to_string()}}),
                    )?;
                    return Ok(true);
                }
            };
            if Some(request.delegation_id.as_str()) != collaboration_publication_delegation {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok":false,"error":{"code":"INVALID_PAYLOAD","message":"path and body Delegation identities differ"}}),
                )?;
                return Ok(true);
            }
            let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
            let local_node_id = read_local_node_id()?;
            match fabric_runtime::queue_remote_fact_publication(
                &store_owned,
                &firm_home,
                &project_id,
                &local_node_id,
                &credential,
                idempotency_key,
                expected_revision,
                &request,
                current_unix_ms_u64(),
            ) {
                Ok(value) => write_http_json(
                    &mut stream,
                    "202 Accepted",
                    &serde_json::json!({"ok":true,"queued":value}),
                )?,
                Err(error) => {
                    let status = match error.code {
                        harness_fabric::FabricErrorCode::UnauthorizedActor => "403 Forbidden",
                        harness_fabric::FabricErrorCode::ExpectedRevisionConflict => "409 Conflict",
                        _ => "400 Bad Request",
                    };
                    write_http_json(
                        &mut stream,
                        status,
                        &serde_json::json!({"ok":false,"error":{"code":format!("{:?}",error.code).to_ascii_uppercase(),"message":error.message}}),
                    )?;
                }
            }
            return Ok(true);
        }
        if method == "POST" && path_only == "/v1/agentfirm/runtime-commands" {
            if trust_identity_override_header {
                write_http_json(
                    &mut stream,
                    "401 Unauthorized",
                    &serde_json::json!({"ok": false, "error": {"code": "UNAUTHORIZED_ACTOR", "message": "request headers cannot select AgentFirm actor or authority identity"}}),
                )?;
                return Ok(true);
            }
            let credential = match resolve_agentfirm_http_credential(
                trust_transport_token.as_deref(),
            ) {
                Ok(value) => value,
                Err(message) => {
                    write_http_json(
                        &mut stream,
                        "401 Unauthorized",
                        &serde_json::json!({"ok": false, "error": {"code": "UNAUTHORIZED_ACTOR", "message": message}}),
                    )?;
                    return Ok(true);
                }
            };
            let Some(idempotency_key) = trust_idempotency_key.filter(|key| !key.trim().is_empty())
            else {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok": false, "error": {"code": "IDEMPOTENCY_KEY_REUSED", "message": "Idempotency-Key is required"}}),
                )?;
                return Ok(true);
            };
            let Some(expected_version) = trust_expected_version else {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok": false, "error": {"code": "VERSION_CONFLICT", "message": "If-Match exact expected version is required"}}),
                )?;
                return Ok(true);
            };
            let request = match serde_json::from_slice::<RuntimeCommandHttpRequest>(&body) {
                Ok(value) => value,
                Err(error) => {
                    write_http_json(
                        &mut stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok": false, "error": {"code": "INVALID_STATE_TRANSITION", "message": error.to_string()}}),
                    )?;
                    return Ok(true);
                }
            };
            use harness_core::agentfirm_api::{
                AgentSession, AgentSessionStatus, RuntimeCommandKind,
            };
            let now = now_string();
            let (target_node_id, target_identity_id, server_payload) = match request.command {
                RuntimeCommandKind::AuthorMessage => {
                    let intent = match serde_json::from_value::<RuntimeAuthorMessageIntent>(
                        request.payload,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            write_http_json(
                                &mut stream,
                                "400 Bad Request",
                                &serde_json::json!({"ok":false,"error":{"code":"INVALID_STATE_TRANSITION","message":format!("invalid author_message intent: {error}")}}),
                            )?;
                            return Ok(true);
                        }
                    };
                    let (message_admission_authority, delegation_authority) = {
                        let firm_home =
                            execution_space::firm_home().map_err(execution_space_err)?;
                        let local_node_id = read_local_node_id()?;
                        let scope = intent.draft.collaboration_scope.as_ref();
                        let delegation_scoped = scope
                            .and_then(|scope| scope.delegation_id.as_ref())
                            .is_some();
                        if delegation_scoped {
                            let Some(remote_transfer) = intent.remote_transfer.as_ref() else {
                                write_http_json(
                                    &mut stream,
                                    "400 Bad Request",
                                    &serde_json::json!({"ok":false,"error":{"code":"INVALID_PAYLOAD","message":"Delegation-scoped cross-node Message requires exact remote_transfer route facts"}}),
                                )?;
                                return Ok(true);
                            };
                            match fabric_runtime::resolve_collaboration_message_authority(
                                    &store_owned,
                                    &firm_home,
                                    &project_id,
                                    &local_node_id,
                                    &credential,
                                    &intent.draft,
                                    remote_transfer,
                                ) {
                                    Ok(authority) => (
                                        Some(harness_core::collaboration::MessageAdmissionAuthority::WorkDelegation(authority.clone())),
                                        Some(authority),
                                    ),
                                    Err(error) => {
                                        write_http_json(
                                            &mut stream,
                                            "403 Forbidden",
                                            &serde_json::json!({"ok":false,"error":{"code":format!("{:?}",error.code).to_ascii_uppercase(),"message":error.message}}),
                                        )?;
                                        return Ok(true);
                                    }
                                }
                        } else if scope.is_some() {
                            // Ordinary peer-Team admission runs with or without a
                            // remote route: a same-Space same-Node target is
                            // delivered by the local authoring Store directly.
                            match resolve_peer_team_message_admission_authority(
                                    &store_owned,
                                    &firm_home,
                                    &project_id,
                                    &local_node_id,
                                    &credential.actor,
                                    &intent.draft,
                                    intent.remote_transfer.as_ref(),
                                ) {
                                    Ok(resolved) => (
                                        Some(harness_core::collaboration::MessageAdmissionAuthority::PeerTeam(resolved.authority)),
                                        None,
                                    ),
                                    Err(message) => {
                                        write_http_json(
                                            &mut stream,
                                            "403 Forbidden",
                                            &serde_json::json!({"ok":false,"error":{"code":"UNAUTHORIZED_ACTOR","message":message}}),
                                        )?;
                                        return Ok(true);
                                    }
                                }
                        } else {
                            if intent.remote_transfer.is_some() {
                                write_http_json(
                                    &mut stream,
                                    "400 Bad Request",
                                    &serde_json::json!({"ok":false,"error":{"code":"INVALID_PAYLOAD","message":"remote_transfer requires a CollaborationScope on the Message draft"}}),
                                )?;
                                return Ok(true);
                            }
                            (None, None)
                        }
                    };
                    let target_node_id = intent
                        .draft
                        .team_run_id
                        .as_deref()
                        .and_then(|run_id| {
                            store_owned
                                .team_runs()
                                .ok()?
                                .into_iter()
                                .rev()
                                .find(|run| run.id == run_id)
                                .map(|run| run.execution_node_id)
                        })
                        .or(request.target_node_id.clone())
                        .ok_or_else(|| {
                            CliError::Usage(
                                "INVALID_RUNTIME_COMMAND: Message target Node cannot be resolved"
                                    .into(),
                            )
                        })?;
                    (
                        target_node_id,
                        None,
                        serde_json::json!({
                            "draft": intent.draft,
                            "remote_transfer": intent.remote_transfer,
                            "message_admission_authority": message_admission_authority,
                            "delegation_authority": delegation_authority,
                        }),
                    )
                }
                RuntimeCommandKind::StartSession => {
                    let intent = match serde_json::from_value::<RuntimeStartSessionIntent>(
                        request.payload,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            write_http_json(
                                &mut stream,
                                "400 Bad Request",
                                &serde_json::json!({"ok":false,"error":{"code":"INVALID_STATE_TRANSITION","message":format!("invalid start_session intent: {error}")}}),
                            )?;
                            return Ok(true);
                        }
                    };
                    let identity = store_owned
                        .fabric_agent_identities(&project_id)?
                        .into_iter()
                        .find(|identity| identity.id == intent.agent_member_id)
                        .ok_or_else(|| CliError::Usage("AGENT_IDENTITY_NOT_FOUND".into()))?;
                    // AgentSession placement is machine runtime truth, independent
                    // of TeamMembership. This machine's immutable Node identity
                    // and active project registration are the server-resolved
                    // placement; Team joins/leaves never create or close it.
                    let target_node_id = read_local_node_id()?;
                    if request
                        .target_node_id
                        .as_deref()
                        .is_some_and(|claimed| claimed != target_node_id)
                    {
                        write_http_json(
                            &mut stream,
                            "403 Forbidden",
                            &serde_json::json!({"ok":false,"error":{"code":"UNAUTHORIZED_ACTOR","message":"caller-selected StartSession Node does not match the server-resolved local machine Node"}}),
                        )?;
                        return Ok(true);
                    }
                    let member = store_owned
                        .trust_agent_members(&project_id)?
                        .into_iter()
                        .find(|member| member.id == identity.id)
                        .ok_or_else(|| {
                            CliError::Usage(
                                "SERVER_PROVIDER_PROFILE_UNAVAILABLE: AgentIdentity has no canonical AgentMember profile"
                                    .into(),
                            )
                        })?;
                    let provider_profile_ref = member.provider_profile_ref.ok_or_else(|| {
                        CliError::Usage(
                            "SERVER_PROVIDER_PROFILE_UNAVAILABLE: AgentMember has no frozen provider profile"
                                .into(),
                        )
                    })?;
                    let provider_kind = ["codex", "claude", "kimi", "pi"]
                        .into_iter()
                        .find(|provider| provider_profile_ref.starts_with(provider))
                        .ok_or_else(|| {
                            CliError::Usage(
                                "SERVER_PROVIDER_PROFILE_UNAVAILABLE: provider profile is not a closed supported provider"
                                .into(),
                            )
                        })?;
                    let provider_profile = team_member_provider_profile(provider_kind);
                    let availability =
                        crate::provider_adapter::provider_availability(provider_kind)
                            .map_err(CliError::Usage)?;
                    if !availability.available {
                        return Err(CliError::Usage(format!(
                            "PROVIDER_UNAVAILABLE: {} binary {} is not installed or failed its version probe",
                            availability.provider, availability.binary
                        )));
                    }
                    let session_id = format!(
                        "session:{}:{}",
                        identity.id,
                        harness_store::canonical_json_fingerprint(&serde_json::json!({
                            "identity":identity.id,
                            "node":target_node_id,
                            "key":idempotency_key,
                        }))
                    );
                    // Session identity and its replay fingerprint must not depend
                    // on a newly sampled HTTP wall clock. The durable command
                    // record carries real accepted/settled timestamps; these
                    // projection fields bind the immutable start request.
                    let session_observed_at = format!("runtime-command:{}", idempotency_key);
                    let session = AgentSession {
                        id: session_id.clone(),
                        agent_member_id: identity.id.clone(),
                        node_id: target_node_id.clone(),
                        execution_space_id: project_id.clone(),
                        node_daemon_id: String::new(),
                        node_daemon_generation: 0,
                        provider_kind: provider_kind.to_string(),
                        provider_profile_ref,
                        permission_envelope_ref: format!(
                            "agent-identity:{}:permission:v{}",
                            identity.id, identity.version
                        ),
                        effective_permission_ceiling: identity.permission_ceiling,
                        lifecycle: AgentSessionStatus::Cold,
                        runtime_generation: 1,
                        control_state: agent_session_control_state_for_profile(
                            Some(&provider_profile),
                            "",
                            0,
                            1,
                        ),
                        native_session_ref: None,
                        current_turn_id: None,
                        queued_input_count: 0,
                        version: 1,
                        opened_at: session_observed_at.clone(),
                        last_active_at: session_observed_at,
                        closed_at: None,
                    };
                    (
                        target_node_id,
                        Some(identity.id),
                        serde_json::json!({
                            "session_id": session_id,
                            "session_generation": 1,
                            "session": session,
                        }),
                    )
                }
                RuntimeCommandKind::DispatchProvider => {
                    let intent = match serde_json::from_value::<RuntimeDispatchIntent>(
                        request.payload,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            write_http_json(
                                &mut stream,
                                "400 Bad Request",
                                &serde_json::json!({"ok":false,"error":{"code":"INVALID_STATE_TRANSITION","message":format!("invalid provider dispatch intent: {error}")}}),
                            )?;
                            return Ok(true);
                        }
                    };
                    let session = store_owned
                        .fabric_agent_sessions(&project_id)?
                        .into_iter()
                        .find(|session| session.id == intent.session_id)
                        .ok_or_else(|| CliError::Usage("AGENT_SESSION_NOT_FOUND".into()))?;
                    (
                        session.node_id,
                        Some(session.agent_member_id),
                        serde_json::json!({
                            "session_id":intent.session_id,
                            "session_generation":intent.session_generation,
                            "delivery_id":intent.delivery_id,
                            "claim_id":intent.claim_id,
                            "dispatch_mode":intent.dispatch_mode,
                        }),
                    )
                }
                RuntimeCommandKind::StopSession
                | RuntimeCommandKind::ResumeSession
                | RuntimeCommandKind::CancelProviderTurn => {
                    let intent = match serde_json::from_value::<RuntimeSessionIntent>(
                        request.payload,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            write_http_json(
                                &mut stream,
                                "400 Bad Request",
                                &serde_json::json!({"ok":false,"error":{"code":"INVALID_STATE_TRANSITION","message":format!("invalid session control intent: {error}")}}),
                            )?;
                            return Ok(true);
                        }
                    };
                    let session = store_owned
                        .fabric_agent_sessions(&project_id)?
                        .into_iter()
                        .find(|session| session.id == intent.session_id)
                        .ok_or_else(|| CliError::Usage("AGENT_SESSION_NOT_FOUND".into()))?;
                    (
                        session.node_id,
                        Some(session.agent_member_id),
                        serde_json::json!({
                            "session_id":intent.session_id,
                            "session_generation":intent.session_generation,
                        }),
                    )
                }
                other => {
                    write_http_json(
                        &mut stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok":false,"error":{"code":"RUNTIME_COMMAND_UNSUPPORTED","message":format!("runtime command {other:?} has no reviewed HTTP intent schema")}}),
                    )?;
                    return Ok(true);
                }
            };
            let registered = store_owned
                .latest_node_project_registrations()?
                .into_iter()
                .any(|registration| {
                    registration.node_id == target_node_id
                        && registration.execution_space_id == *project_id
                        && registration.status == NodeProjectRegistrationStatus::Active
                });
            let lease = store_owned.latest_node_daemon_lease(&target_node_id)?;
            let Some(lease) = lease.filter(|lease| {
                registered
                    && lease.status == NodeDaemonLeaseStatus::Active
                    && lease.expires_unix_ms > current_unix_ms_u64()
            }) else {
                write_http_json(
                    &mut stream,
                    "409 Conflict",
                    &serde_json::json!({"ok": false, "error": {"code": "SUPERVISOR_GENERATION_FENCED", "message": "target Node has no current daemon registered in this Execution Space"}}),
                )?;
                return Ok(true);
            };
            let mut authority_candidates = vec![credential.actor.clone()];
            authority_candidates.extend(credential.authority_actors.iter().cloned());
            let authenticated_actor = if let Some(identity_id) = target_identity_id.as_deref() {
                let mut resolved = None;
                for actor in authority_candidates {
                    if runtime_control_actor_is_authorized(
                        &actor,
                        identity_id,
                        &target_node_id,
                        &lease.daemon_id,
                    )? {
                        resolved = Some(actor);
                        break;
                    }
                }
                let Some(actor) = resolved else {
                    write_http_json(
                        &mut stream,
                        "403 Forbidden",
                        &serde_json::json!({"ok":false,"error":{"code":"UNAUTHORIZED_ACTOR","message":"credential is not exact self or exact machine Operator/NodeDaemon for the target AgentSession; Team Host authority is Team-scoped"}}),
                    )?;
                    return Ok(true);
                };
                actor
            } else {
                credential.actor.clone()
            };
            let server_payload = if request.command == RuntimeCommandKind::StartSession {
                let mut payload = server_payload;
                if let Some(session) = payload
                    .get_mut("session")
                    .and_then(|value| value.as_object_mut())
                {
                    session.insert("node_daemon_id".into(), serde_json::json!(lease.daemon_id));
                    session.insert(
                        "node_daemon_generation".into(),
                        serde_json::json!(lease.generation),
                    );
                    let provider = session
                        .get("provider_kind")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("codex");
                    let runtime_generation = session
                        .get("runtime_generation")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(1);
                    let profile = team_member_provider_profile(provider);
                    session.insert(
                        "control_state".into(),
                        serde_json::to_value(agent_session_control_state_for_profile(
                            Some(&profile),
                            &lease.daemon_id,
                            lease.generation,
                            runtime_generation,
                        ))?,
                    );
                }
                payload
            } else {
                server_payload
            };
            // Resolve an exact replay from the immutable accepted envelope before
            // rebuilding a fence from mutable session state. Open/Stop/Reopen are
            // expected to change native-session or residency fields; deriving a
            // second envelope after that postcondition would turn an identical
            // caller retry into a false idempotency conflict. Hostile reuse still
            // fails because every caller-authored semantic field must match the
            // original accepted envelope byte-for-byte.
            if request.command != RuntimeCommandKind::AuthorMessage {
                let command_id = format!("runtime-command:{idempotency_key}");
                let original = store_owned
                    .canonical_operations_for_space(&project_id)?
                    .into_iter()
                    .find(|operation| {
                        operation.event.aggregate_kind == "runtime_command"
                            && operation.event.aggregate_id == command_id
                            && operation.event.transition == "accepted"
                    })
                    .map(|operation| {
                        serde_json::from_value::<
                                harness_core::agentfirm_api::ControlCommandEnvelope,
                            >(operation.event.payload)
                            .map_err(CliError::Json)
                    })
                    .transpose()?;
                if let Some(original) = original {
                    let exact_intent = original.command == request.command
                        && original.expires_unix_ms == request.expires_unix_ms
                        && original.payload == server_payload
                        && original.expected_version == expected_version
                        && original.authenticated_actor == authenticated_actor
                        && original.idempotency_key == idempotency_key
                        && request
                            .target_node_id
                            .as_deref()
                            .is_none_or(|node_id| node_id == original.target_node_id);
                    if !exact_intent {
                        write_http_json(
                            &mut stream,
                            "409 Conflict",
                            &serde_json::json!({
                                "ok": false,
                                "error": "IDEMPOTENCY_KEY_REUSED: RuntimeCommand key was reused with different caller semantics",
                            }),
                        )?;
                        return Ok(true);
                    }
                    let record = store_owned
                        .runtime_commands(&project_id)?
                        .into_iter()
                        .find(|record| record.id == command_id)
                        .ok_or_else(|| {
                            CliError::Usage(format!(
                                "RUNTIME_COMMAND_RECOVERY_REQUIRED: accepted command {command_id} has no readable projection"
                            ))
                        })?;
                    if record.status == harness_core::agentfirm_api::RuntimeCommandStatus::Applied
                        && record.effect_certainty
                            == harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
                    {
                        write_http_json(
                            &mut stream,
                            "200 OK",
                            &serde_json::json!({
                                "ok": true,
                                "result": record.result,
                                "replayed": true,
                            }),
                        )?;
                    } else {
                        write_http_json(
                            &mut stream,
                            "409 Conflict",
                            &serde_json::json!({
                                "ok": false,
                                "error": format!(
                                    "RUNTIME_COMMAND_RECOVERY_REQUIRED: exact replay is {:?}/{:?}/{:?}",
                                    record.status,
                                    record.effect_certainty,
                                    record.postcondition_status,
                                ),
                                "replayed": true,
                            }),
                        )?;
                    }
                    return Ok(true);
                }
            }
            let command_target_session = if request.command == RuntimeCommandKind::AuthorMessage {
                None
            } else if request.command == RuntimeCommandKind::StartSession {
                Some(
                    serde_json::from_value::<AgentSession>(
                        server_payload
                            .get("session")
                            .cloned()
                            .ok_or_else(|| CliError::Usage("INVALID_RUNTIME_COMMAND: StartSession payload lost server-bound session".into()))?,
                    )
                    .map_err(|error| {
                        CliError::Usage(format!(
                            "INVALID_RUNTIME_COMMAND: server-bound StartSession session is invalid: {error}"
                        ))
                    })?,
                )
            } else {
                let session_id = server_payload
                    .get("session_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        CliError::Usage(
                            "INVALID_RUNTIME_COMMAND: runtime control lacks session_id".into(),
                        )
                    })?;
                Some(
                    store_owned
                        .fabric_agent_sessions(&project_id)?
                        .into_iter()
                        .find(|session| session.id == session_id)
                        .ok_or_else(|| CliError::Usage("AGENT_SESSION_NOT_FOUND".into()))?,
                )
            };
            let command_binding = command_target_session
                .as_ref()
                .map(runtime_command_binding_for_session)
                .unwrap_or_default();
            let command_precondition = command_target_session
                .as_ref()
                .map(
                    |session| harness_core::agentfirm_api::RuntimeCommandPrecondition {
                        expected_session_version: Some(session.version),
                        expected_residency: Some(session.control_state.runtime_residency),
                        expected_activity: Some(session.control_state.activity),
                        expected_execution_driver: Some(session.control_state.execution_driver),
                        ..Default::default()
                    },
                )
                .unwrap_or_default();
            let envelope = harness_core::agentfirm_api::ControlCommandEnvelope {
                id: format!("runtime-command:{}", idempotency_key),
                execution_space_id: project_id.clone(),
                target_node_id,
                target_node_daemon_id: lease.daemon_id,
                target_node_daemon_generation: lease.generation,
                authenticated_actor,
                command: request.command,
                required_capability: runtime_command_capability(request.command).to_string(),
                idempotency_key,
                expected_version,
                expires_unix_ms: request.expires_unix_ms,
                binding: command_binding,
                precondition: command_precondition,
                postcondition: runtime_command_postcondition_for(request.command),
                payload_fingerprint: harness_store::canonical_json_fingerprint(&server_payload),
                payload: server_payload,
                issued_at: now,
            };
            let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
            match supervisor_daemon::runtime_command_via_socket(
                &firm_home,
                &envelope.target_node_id,
                &envelope,
            ) {
                Ok(response) => {
                    if response.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                        write_http_json(&mut stream, "409 Conflict", &response)?;
                        return Ok(true);
                    }
                    if envelope.command == RuntimeCommandKind::AuthorMessage {
                        if let Some(remote_transfer) = envelope
                            .payload
                            .get("remote_transfer")
                            .filter(|value| !value.is_null())
                        {
                            let request = serde_json::from_value::<
                                fabric_runtime::QueueCollaborationMessageRequest,
                            >(remote_transfer.clone())
                            .map_err(|error| {
                                CliError::Usage(format!(
                                    "INVALID_COLLABORATION_MESSAGE_TRANSFER: {error}"
                                ))
                            })?;
                            let message = serde_json::from_value::<
                                harness_core::agentfirm_api::Message,
                            >(
                                response.get("result").cloned().ok_or_else(|| {
                                    CliError::Usage("COLLABORATION_MESSAGE_RESULT_MISSING".into())
                                })?,
                            )?;
                            let message_admission_authority = if let Some(value) = envelope
                                .payload
                                .get("message_admission_authority")
                                .filter(|value| !value.is_null())
                            {
                                serde_json::from_value::<
                                    harness_core::collaboration::MessageAdmissionAuthority,
                                >(value.clone())?
                            } else {
                                // Compatibility is deliberately read-only and
                                // bounded to an already prepared WorkDelegation
                                // command. New peer-Team commands must carry the
                                // tagged canonical authority field.
                                harness_core::collaboration::MessageAdmissionAuthority::WorkDelegation(
                                    serde_json::from_value(
                                        envelope
                                            .payload
                                            .get("delegation_authority")
                                            .cloned()
                                            .filter(|value| !value.is_null())
                                            .ok_or_else(|| {
                                                CliError::Usage(
                                                    "COLLABORATION_MESSAGE_AUTHORITY_MISSING".into(),
                                                )
                                            })?,
                                    )?,
                                )
                            };
                            match fabric_runtime::queue_collaboration_message(
                                &firm_home,
                                &project_id,
                                &envelope.target_node_id,
                                &credential.actor,
                                &envelope.idempotency_key,
                                &message,
                                &request,
                                message_admission_authority,
                                current_unix_ms_u64(),
                            ) {
                                Ok(queued) => write_http_json(
                                    &mut stream,
                                    "202 Accepted",
                                    &serde_json::json!({
                                        "ok": true,
                                        "message": message,
                                        "remote_transfer": queued,
                                    }),
                                )?,
                                Err(error) => write_http_json(
                                    &mut stream,
                                    "409 Conflict",
                                    &serde_json::json!({"ok":false,"error":{"code":format!("{:?}",error.code).to_ascii_uppercase(),"message":error.message}}),
                                )?,
                            }
                        } else {
                            write_http_json(&mut stream, "200 OK", &response)?;
                        }
                    } else {
                        write_http_json(&mut stream, "200 OK", &response)?;
                    }
                }
                Err(error) => write_http_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &serde_json::json!({"ok": false, "error": {"code": "NODE_DAEMON_UNAVAILABLE", "message": error.to_string()}}),
                )?,
            }
            return Ok(true);
        }
        if method == "POST" && agentfirm_api::is_http_mutation_path(&path_only) {
            if trust_identity_override_header {
                write_http_json(
                    &mut stream,
                    "401 Unauthorized",
                    &serde_json::json!({
                        "ok": false,
                        "error": {
                            "code": "UNAUTHORIZED_ACTOR",
                            "message": "request headers cannot select AgentFirm actor or authority identity"
                        }
                    }),
                )?;
                return Ok(true);
            }
            let credential =
                match resolve_agentfirm_http_credential(trust_transport_token.as_deref()) {
                    Ok(credential) => credential,
                    Err(message) => {
                        write_http_json(
                            &mut stream,
                            "401 Unauthorized",
                            &serde_json::json!({
                                "ok": false,
                                    "error": {"code": "UNAUTHORIZED_ACTOR", "message": message}
                            }),
                        )?;
                        return Ok(true);
                    }
                };
            let Some(idempotency_key) = trust_idempotency_key.filter(|key| !key.trim().is_empty())
            else {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({
                        "ok": false,
                        "error": {"code": "IDEMPOTENCY_KEY_REUSED", "message": "Idempotency-Key is required"}
                    }),
                )?;
                return Ok(true);
            };
            let Some(expected_version) = trust_expected_version else {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({
                        "ok": false,
                        "error": {"code": "VERSION_CONFLICT", "message": "If-Match exact expected version is required"}
                    }),
                )?;
                return Ok(true);
            };
            let auth = agentfirm_api::AuthenticatedMutation {
                execution_space_id: project_id.clone(),
                actor: credential.actor,
                authorized_authority_actors: credential.authority_actors,
                idempotency_key,
                expected_version,
                request_fingerprint: None,
            };
            if let Some((team_run_id, message_id)) =
                agentfirm_api::provider_answer_route(&path_only)
            {
                match answer_provider_message_value(
                    &store_owned,
                    team_run_id,
                    message_id,
                    &serde_json::from_slice(&body).map_err(CliError::Json)?,
                    &auth.actor,
                    "http_token",
                ) {
                    Ok(result) => write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({"ok": true, "result": result}),
                    )?,
                    Err(error) => {
                        let message = error.to_string();
                        let status = if message.contains("UNAUTHORIZED_ACTOR") {
                            "403 Forbidden"
                        } else {
                            "409 Conflict"
                        };
                        write_http_json(
                            &mut stream,
                            status,
                            &serde_json::json!({"ok": false, "error": {"code": "PROVIDER_INTERACTION_ANSWER_REJECTED", "message": message}}),
                        )?;
                    }
                }
                return Ok(true);
            }
            if role_actions_api::is_http_mutation_path(&path_only) {
                let role_store = match projects.scoped_store_for_project(
                    &store_owned,
                    &project_id,
                    project_param.as_deref(),
                ) {
                    Ok(store) => store,
                    Err(error) => {
                        let detail = error.to_string();
                        write_http_json(
                            &mut stream,
                            "404 Not Found",
                            &serde_json::json!({
                                "ok": false,
                                "error": {"code": "PROJECT_BINDING_NOT_FOUND", "message": detail}
                            }),
                        )?;
                        return Ok(true);
                    }
                };
                // A managed MemberRun Close is one authorize-only Role intent
                // followed by an exact Supervisor-owned runtime transaction. The
                // provider receipt and Session Detached/Idle postcondition must be
                // durable before Closed/Stopped is projected or HTTP returns 200.
                let close_member_run_id = path_only
                    .strip_prefix("/v1/agentfirm/member-runs/")
                    .and_then(|rest| rest.strip_suffix("/close"))
                    .filter(|member_run_id| {
                        !member_run_id.is_empty() && !member_run_id.contains('/')
                    })
                    .map(str::to_string);
                let interrupt_member_run_id = path_only
                    .strip_prefix("/v1/agentfirm/member-runs/")
                    .and_then(|rest| rest.strip_suffix("/interrupt"))
                    .filter(|member_run_id| {
                        !member_run_id.is_empty() && !member_run_id.contains('/')
                    })
                    .map(str::to_string);
                let reopen_member_run_id = path_only
                    .strip_prefix("/v1/agentfirm/member-runs/")
                    .and_then(|rest| rest.strip_suffix("/reopen"))
                    .filter(|member_run_id| {
                        !member_run_id.is_empty() && !member_run_id.contains('/')
                    })
                    .map(str::to_string);
                if interrupt_member_run_id.is_some() {
                    match role_actions_api::authorize_member_interrupt(
                        &role_store,
                        &auth,
                        &path_only,
                        &body,
                    ) {
                        Ok(permit) => {
                            let control_body = serde_json::json!({
                                "reason": permit.reason,
                                "requested_by": permit.requested_by,
                            });
                            match interrupt_team_member_value(
                                &role_store,
                                &permit.team_run_id,
                                &permit.member_run_id,
                                &control_body,
                            ) {
                                Ok(result) => write_http_json(
                                    &mut stream,
                                    "200 OK",
                                    &serde_json::json!({
                                        "ok": true,
                                        "action_protocol_version": "agentfirm.role_actions.v1",
                                        "projection": result,
                                    }),
                                )?,
                                Err(error) => write_http_json(
                                    &mut stream,
                                    "409 Conflict",
                                    &serde_json::json!({
                                        "ok": false,
                                        "error": {
                                            "code": "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                                            "message": error.to_string(),
                                        }
                                    }),
                                )?,
                            }
                        }
                        Err(StoreError::Conflict(encoded)) => {
                            let error = serde_json::from_str::<serde_json::Value>(&encoded)
                                .unwrap_or_else(|_| {
                                    serde_json::json!({
                                        "code": "INVALID_STATE_TRANSITION",
                                        "message": encoded,
                                    })
                                });
                            write_http_json(
                                &mut stream,
                                "409 Conflict",
                                &serde_json::json!({"ok": false, "error": error}),
                            )?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                    return Ok(true);
                }
                if close_member_run_id.is_some() {
                    let permit = match role_actions_api::authorize_member_close(
                        &role_store,
                        &auth,
                        &path_only,
                        &body,
                        trust_confirmed_action.as_deref(),
                    ) {
                        Ok(permit) => permit,
                        Err(StoreError::Conflict(encoded)) => {
                            let error = serde_json::from_str::<serde_json::Value>(&encoded)
                                .unwrap_or_else(|_| {
                                    serde_json::json!({
                                        "code": "INVALID_STATE_TRANSITION",
                                        "message": encoded,
                                    })
                                });
                            write_http_json(
                                &mut stream,
                                "409 Conflict",
                                &serde_json::json!({"ok": false, "error": error}),
                            )?;
                            return Ok(true);
                        }
                        Err(error) => return Err(error.into()),
                    };
                    let member_before = latest_member_runs_in_append_order(&role_store)?
                        .into_iter()
                        .find(|member| member.id == permit.member_run_id)
                        .ok_or_else(|| {
                            CliError::Usage(format!(
                                "member run not found after Close authorization: {}",
                                permit.member_run_id
                            ))
                        })?;
                    if !member_before.is_external_interactive() {
                        let close_result = if managed_member_runtime_close_is_settled(
                            &role_store,
                            &member_before,
                        )? {
                            Ok(serde_json::json!({
                                "member_run_id": member_before.id,
                                "status": "closed",
                                "provider_effect_repeated": false,
                            }))
                        } else {
                            dispatch_live_member_control(
                                &role_store,
                                LiveMemberControlRequest::Close {
                                    team_run_id: permit.team_run_id.clone(),
                                    member_run_id: permit.member_run_id.clone(),
                                    reason: "authenticated Role Action close_member_run"
                                        .to_string(),
                                    requested_by: permit.requested_by.clone(),
                                },
                            )
                        };
                        let close_settled = if managed_member_runtime_close_is_settled(
                            &role_store,
                            &member_before,
                        )? {
                            true
                        } else {
                            await_managed_member_runtime_close_settled(&role_store, &member_before)?
                        };
                        match (close_result, close_settled) {
                            (Ok(provider_projection), true) => {
                                let projection = latest_member_runs_in_append_order(&role_store)?
                                    .into_iter()
                                    .find(|member| member.id == permit.member_run_id)
                                    .ok_or_else(|| {
                                        CliError::Usage(format!(
                                            "member run disappeared after Close: {}",
                                            permit.member_run_id
                                        ))
                                    })?;
                                write_http_json(
                                    &mut stream,
                                    "200 OK",
                                    &serde_json::json!({
                                        "ok": true,
                                        "action_protocol_version": "agentfirm.role_actions.v1",
                                        "projection": projection,
                                        "provider_close": provider_projection,
                                    }),
                                )?;
                            }
                            (Err(error), true) => {
                                let projection = latest_member_runs_in_append_order(&role_store)?
                                    .into_iter()
                                    .find(|member| member.id == permit.member_run_id)
                                    .ok_or_else(|| {
                                        CliError::Usage(format!(
                                            "member run disappeared after reconciled Close: {}",
                                            permit.member_run_id
                                        ))
                                    })?;
                                write_http_json(
                                    &mut stream,
                                    "200 OK",
                                    &serde_json::json!({
                                        "ok": true,
                                        "action_protocol_version": "agentfirm.role_actions.v1",
                                        "projection": projection,
                                        "runtime_close_reconciled": true,
                                        "runtime_close_note": "the live-control call returned an uncertain receipt, but the exact durable CloseMember + Detached/Idle + Stopped postcondition was observed before response",
                                        "provider_close_error": error.to_string(),
                                    }),
                                )?;
                            }
                            (Ok(_), false) => write_http_json(
                                &mut stream,
                                "409 Conflict",
                                &serde_json::json!({
                                    "ok": false,
                                    "error": {
                                        "code": "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                                        "message": "provider Close returned without the exact durable CloseMember + Detached/Idle + Stopped postcondition",
                                    }
                                }),
                            )?,
                            (Err(error), false) => write_http_json(
                                &mut stream,
                                "409 Conflict",
                                &serde_json::json!({
                                    "ok": false,
                                    "error": {
                                        "code": "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                                        "message": error.to_string(),
                                    }
                                }),
                            )?,
                        }
                        return Ok(true);
                    }
                    // external_interactive has no Harness-owned provider runtime;
                    // its Close remains the coordination-only Role Action below.
                }
                match role_actions_api::execute(
                    &role_store,
                    auth,
                    &path_only,
                    &body,
                    trust_confirmed_action.as_deref(),
                ) {
                    Ok(result) => {
                        if let Some(member_run_id) = reopen_member_run_id {
                            let reopened = latest_member_runs_in_append_order(&role_store)?
                                .into_iter()
                                .find(|member| member.id == member_run_id)
                                .ok_or_else(|| {
                                    CliError::Usage(format!(
                                        "member run not found after Reopen: {member_run_id}"
                                    ))
                                })?;
                            let activation = (|| -> CliResult<()> {
                                if reopened_member_requires_supervisor_start(
                                    &role_store,
                                    &reopened.team_run_id,
                                    &reopened.id,
                                )? {
                                    delegate_team_run_to_node_daemon_in_space(
                                        &role_store,
                                        &project_id,
                                        &reopened.team_run_id,
                                        TEAM_RUN_START_DEFAULT_CONCURRENCY,
                                    )?;
                                }
                                Ok(())
                            })();
                            match activation {
                                Ok(()) => write_http_json(&mut stream, "200 OK", &result)?,
                                Err(error) => {
                                    if let Some(observed) =
                                        await_managed_member_runtime_reopen_settled(
                                            &role_store,
                                            &reopened,
                                        )?
                                    {
                                        let mut reconciled = serde_json::to_value(&result)?;
                                        reconciled["projection"] = serde_json::to_value(observed)?;
                                        reconciled["runtime_activation_reconciled"] =
                                            serde_json::json!(true);
                                        reconciled["runtime_activation_note"] = serde_json::json!(
                                            "the direct NodeDaemon dispatch returned a transient error, but the exact higher-generation Attached runtime postcondition was observed before response"
                                        );
                                        write_http_json(&mut stream, "200 OK", &reconciled)?;
                                    } else {
                                        write_http_json(
                                            &mut stream,
                                            "409 Conflict",
                                            &serde_json::json!({
                                                "ok": false,
                                                "error": {
                                                    "code": "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                                                    "message": error.to_string(),
                                                }
                                            }),
                                        )?;
                                    }
                                }
                            }
                        } else {
                            write_http_json(&mut stream, "200 OK", &result)?;
                        }
                    }
                    Err(StoreError::Conflict(encoded)) => {
                        let error = serde_json::from_str::<serde_json::Value>(&encoded).unwrap_or_else(
                            |_| serde_json::json!({"code": "INVALID_STATE_TRANSITION", "message": encoded}),
                        );
                        write_http_json(
                            &mut stream,
                            "409 Conflict",
                            &serde_json::json!({"ok": false, "error": error}),
                        )?;
                    }
                    Err(error) => return Err(error.into()),
                }
                return Ok(true);
            }
            let command = match serde_json::from_slice::<agentfirm_api::TrustCommand>(&body) {
                Ok(command) if command.matches_http_route(&path_only) => command,
                Ok(_) => {
                    write_http_json(
                        &mut stream,
                        "400 Bad Request",
                        &serde_json::json!({
                            "ok": false, "error": {"code": "INVALID_STATE_TRANSITION", "message": "command payload does not match the exact endpoint"}
                        }),
                    )?;
                    return Ok(true);
                }
                Err(error) => {
                    write_http_json(
                        &mut stream,
                        "400 Bad Request",
                        &serde_json::json!({
                            "ok": false, "error": {"code": "INVALID_STATE_TRANSITION", "message": error.to_string()}
                        }),
                    )?;
                    return Ok(true);
                }
            };
            match agentfirm_api::execute(&store_owned, auth, command) {
                Ok(result) => write_http_json(&mut stream, "200 OK", &result)?,
                Err(StoreError::Conflict(encoded)) => {
                    let error = serde_json::from_str::<serde_json::Value>(&encoded).unwrap_or_else(
                        |_| serde_json::json!({"code": "INVALID_STATE_TRANSITION", "message": encoded}),
                    );
                    write_http_json(
                        &mut stream,
                        "409 Conflict",
                        &serde_json::json!({"ok": false, "error": error}),
                    )?;
                }
                Err(error) => return Err(error.into()),
            }
            return Ok(true);
        }
        if retired_http_path(&path_only) {
            write_http_json(
                &mut stream,
                "410 Gone",
                &serde_json::json!({
                    "ok": false,
                    "error": "retired_coordination_surface",
                    "detail": "This Goal/GoalPhase/Task Graph API was retired. Current coordination uses /v1/teams and /v1/team-runs; historical rows are export-only through `harness legacy-goal-task export|verify`."
                }),
            )?;
            return Ok(true);
        }
        Ok(false)
    }
}
