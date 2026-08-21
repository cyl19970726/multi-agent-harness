use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_collaboration_control_plane_http<K: harness_fabric::ArtifactKeyBackend>(
    method: &str,
    path: &str,
    target: &str,
    headers: &std::collections::BTreeMap<String, String>,
    body: &[u8],
    control: &ControlPlane<'_, K>,
    generation: u64,
    collaboration_root: &Path,
) -> Result<serde_json::Value, FabricError> {
    if headers.keys().any(|name| {
        matches!(
            name.as_str(),
            "x-agentfirm-actor-id"
                | "x-agentfirm-actor-kind"
                | "x-agentfirm-authority-id"
                | "x-agentfirm-authority-kind"
        )
    }) {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "collaboration request headers cannot select actor or authority identity",
        ));
    }
    let credential = crate::resolve_agentfirm_http_credential(
        headers.get("x-agentfirm-token").map(String::as_str),
    )
    .map_err(|message| FabricError::none(FabricErrorCode::UnauthorizedActor, message))?;
    let store = HarnessStore::new(collaboration_root);
    if method == "GET" && path == "/v1/collaboration/delegations" {
        let query_value = |key: &str| {
            target.split_once('?').and_then(|(_, query)| {
                query.split('&').find_map(|part| {
                    let (candidate, value) = part.split_once('=')?;
                    (candidate == key).then_some(value)
                })
            })
        };
        let state = target
            .split_once('?')
            .and_then(|(_, query)| {
                query.split('&').find_map(|part| {
                    let (key, value) = part.split_once('=')?;
                    (key == "state").then_some(value)
                })
            })
            .map(|state| match state {
                "proposed" => Ok(harness_core::collaboration::DelegationState::Proposed),
                "awaiting_target_decision" => {
                    Ok(harness_core::collaboration::DelegationState::AwaitingTargetDecision)
                }
                "provisioning_target_work" => {
                    Ok(harness_core::collaboration::DelegationState::ProvisioningTargetWork)
                }
                "active" => Ok(harness_core::collaboration::DelegationState::Active),
                "result_available" => {
                    Ok(harness_core::collaboration::DelegationState::ResultAvailable)
                }
                "cancellation_requested" => {
                    Ok(harness_core::collaboration::DelegationState::CancellationRequested)
                }
                "terminal" => Ok(harness_core::collaboration::DelegationState::Terminal),
                _ => Err(FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "unknown Delegation state filter",
                )),
            })
            .transpose()?;
        let filter = harness_store::CollaborationDelegationFilter {
            source_team_id: None,
            target_team_id: None,
            node_id: None,
            state,
        };
        let cursor_secret = headers.get("x-agentfirm-token").ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "collaboration cursor requires authenticated credential",
            )
        })?;
        let cursor = query_value("cursor")
            .map(|value| decode_collaboration_cursor(value, cursor_secret))
            .transpose()?;
        let limit = query_value("limit")
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "collaboration limit is invalid",
                )
            })?
            .unwrap_or(100);
        let page = store
            .list_collaboration_delegations_for_actor(
                control.company_id(),
                &credential.actor,
                &filter,
                cursor,
                limit,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        let next_cursor = page
            .next_cursor
            .as_ref()
            .map(|value| encode_collaboration_cursor(value, cursor_secret))
            .transpose()?;
        return Ok(serde_json::json!({
            "items": page.items,
            "as_of_store_sequence": page.as_of_store_sequence,
            "next_cursor": next_cursor,
        }));
    }
    if method == "POST" && path == "/v1/collaboration/inbound-policies" {
        let idempotency_key = headers
            .get("idempotency-key")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "Idempotency-Key is required",
                )
            })?;
        let expected_revision = headers
            .get("if-match")
            .and_then(|value| value.trim_matches('"').parse::<u64>().ok())
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "If-Match exact policy revision is required",
                )
            })?;
        let request = serde_json::from_slice::<CollaborationInboundPolicyHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        if request.policy_id.trim().is_empty()
            || request.target_team_id.trim().is_empty()
            || request.source_team_id.trim().is_empty()
            || request.allowed_outcome_classes.is_empty()
            || request.max_active_delegations == 0
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "inbound policy requires an authenticated AgentMember target Host and bounded policy scope",
            ));
        }
        let teams = store.teams().map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("target AgentTeam lookup failed: {error}"),
            )
        })?;
        let target_team = teams
            .iter()
            .rev()
            .find(|team| team.id == request.target_team_id)
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "target AgentTeam does not exist",
                )
            })?;
        let resolved_target_host = harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: target_team.host_agent_id.clone(),
        };
        if credential.actor != resolved_target_host {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "only the server-resolved exact target Host may author inbound policy",
            ));
        }
        let now = now_unix_ms()?;
        let policy = harness_core::collaboration::DelegationInboundPolicy {
            id: request.policy_id,
            company_id: control.company_id().into(),
            target_team_id: request.target_team_id,
            source_team_id: request.source_team_id,
            mode: request.mode,
            allowed_outcome_classes: request.allowed_outcome_classes,
            max_active_delegations: request.max_active_delegations,
            created_by_target_host: credential.actor.clone(),
            revision: expected_revision.saturating_add(1),
            created_at: format!("unix-ms:{now}"),
            revoked_at: None,
        };
        let written = store
            .put_collaboration_inbound_policy(
                &harness_store::CollaborationMutationContext {
                    company_id: control.company_id().into(),
                    authenticated_actor: credential.actor.clone(),
                    command_name: "delegation_inbound_policy.put".into(),
                    idempotency_key: idempotency_key.clone(),
                    expected_revision,
                    occurred_at: format!("unix-ms:{now}"),
                },
                &policy,
                &resolved_target_host,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        return Ok(serde_json::json!({
            "policy": written.projection,
            "replayed": written.replayed,
        }));
    }
    let suffix = path
        .strip_prefix("/v1/collaboration/delegations/")
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                "unknown collaboration endpoint",
            )
        })?;
    if method == "GET" && suffix.ends_with("/publications") {
        let delegation_id = suffix
            .strip_suffix("/publications")
            .filter(|value| !value.is_empty() && !value.contains('/'))
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "publication list path is malformed",
                )
            })?;
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        let source_host = store
            .collaboration_source_work_attestation(
                control.company_id(),
                &delegation.source_work_attestation_id,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .map(|attestation| attestation.source_host_ref);
        if credential.actor != delegation.source_owner_ref
            && source_host.as_ref() != Some(&credential.actor)
            && credential.actor != delegation.target_host_ref
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "publication projection requires the exact source owner/Host or target Host",
            ));
        }
        let publications = store
            .collaboration_publications(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "delegation_revision": delegation.revision,
            "items": publications,
        }));
    }
    if method == "GET" && !suffix.contains('/') {
        let delegation = store
            .collaboration_delegation(control.company_id(), suffix)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        if !collaboration_actor_can_read_delegation(
            &store,
            control.company_id(),
            &credential.actor,
            &delegation,
        )? {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation projection requires the exact source owner/Host or target Host",
            ));
        }
        return Ok(serde_json::json!({"delegation":delegation}));
    }
    let idempotency_key = headers
        .get("idempotency-key")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "Idempotency-Key is required",
            )
        })?;
    let expected_revision = headers
        .get("if-match")
        .and_then(|value| value.trim_matches('"').parse::<u64>().ok())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "If-Match exact Delegation revision is required",
            )
        })?;
    let now = now_unix_ms()?;
    let control_actor = AuthenticatedActor {
        company_id: control.company_id().into(),
        actor_id: format!("control-plane:{generation}"),
        actor_kind: harness_fabric::ActorKind::Service,
        // This is the server's exact Control Plane generation, not the
        // browser credential. It owns transport submission while the
        // resolved business actor remains separately frozen in the closed
        // collaboration envelope.
        role_bindings: BTreeSet::from(["company_control_plane".into(), "fabric_submit".into()]),
        session_id: format!("control-plane:{generation}"),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now.saturating_add(30_000),
    };
    let path_parts = suffix.split('/').collect::<Vec<_>>();
    if method == "POST"
        && path_parts.len() == 3
        && path_parts[1] == "artifacts"
        && path_parts[2] == "initiate"
    {
        let delegation_id = path_parts[0];
        let request = serde_json::from_slice::<CollaborationArtifactInitiateHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        if delegation.revision != expected_revision
            || !matches!(
                delegation.state,
                harness_core::collaboration::DelegationState::Active
                    | harness_core::collaboration::DelegationState::ResultAvailable
            )
            || credential.actor != delegation.target_host_ref
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host may initiate an artifact for current active Delegation",
            ));
        }
        let target_work_ref = delegation.target_work_ref.as_ref().ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "active Delegation has no exact target Work",
            )
        })?;
        let attestation = store
            .collaboration_source_work_attestation(
                control.company_id(),
                &delegation.source_work_attestation_id,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "Delegation lacks source Host attestation",
                )
            })?;
        let writer = AuthenticatedActor {
            company_id: control.company_id().into(),
            actor_id: credential.actor.id,
            actor_kind: harness_fabric::ActorKind::AgentMember,
            role_bindings: BTreeSet::from(["artifact_write".into()]),
            session_id: format!("collaboration-artifact-init:{idempotency_key}"),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(30_000),
        };
        if let Some(existing) = control.artifact_manifest(&request.artifact_id)? {
            let exact = existing.company_id == control.company_id()
                && existing.source_node_id == delegation.target_placement.node_id
                && existing.source_team_id.as_deref()
                    == Some(delegation.target_placement.team_id.as_str())
                && existing.source_work_id.as_deref() == Some(target_work_ref.work_id.as_str())
                && existing.operation_id == request.operation_id
                && existing.media_type == request.media_type
                && existing.size_bytes == request.size_bytes
                && existing.sha256 == request.sha256
                && existing.classification == request.classification
                && existing.initiator == writer.actor_id
                && existing.authorized_readers
                    == BTreeSet::from([attestation.source_host_ref.id.clone()]);
            if !exact {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "artifact identity was reused with a different collaboration scope or payload",
                ));
            }
            let (manifest, upload_capability) = control.replay_collaboration_upload_capability(
                &writer,
                generation,
                &request.artifact_id,
                now,
            )?;
            return Ok(serde_json::json!({
                "delegation_id": delegation_id,
                "manifest": manifest,
                "upload_capability": upload_capability,
                "replayed": true,
            }));
        }
        let (manifest, upload_capability) = control.initiate_collaboration_artifact(
            &writer,
            generation,
            &request.artifact_id,
            &delegation.target_placement.node_id,
            &delegation.target_placement.team_id,
            &target_work_ref.work_id,
            request.operation_id.as_deref(),
            &request.media_type,
            request.size_bytes,
            &request.sha256,
            request.classification,
            BTreeSet::from([attestation.source_host_ref.id]),
            now,
        )?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "manifest": manifest,
            "upload_capability": upload_capability,
        }));
    }
    if method == "POST"
        && path_parts.len() == 4
        && path_parts[1] == "artifacts"
        && path_parts[3] == "grant"
    {
        let delegation_id = path_parts[0];
        let artifact_id = path_parts[2];
        let request = serde_json::from_slice::<CollaborationArtifactGrantHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        return grant_collaboration_artifact(
            &store,
            control,
            generation,
            delegation_id,
            artifact_id,
            &request,
            &credential.actor,
            &control_actor,
            idempotency_key,
            expected_revision,
            now,
        );
    }
    if method == "POST"
        && path_parts.len() == 4
        && path_parts[1] == "artifacts"
        && path_parts[3] == "retention"
    {
        let delegation_id = path_parts[0];
        let artifact_id = path_parts[2];
        let request = serde_json::from_slice::<CollaborationArtifactRetentionHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        const MAX_COLLABORATION_RETENTION_MS: u64 = 365 * 24 * 60 * 60 * 1_000;
        if request.retention_duration_ms == 0
            || request.retention_duration_ms > MAX_COLLABORATION_RETENTION_MS
        {
            return Err(FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "retention duration must be positive and within the server one-year policy",
            ));
        }
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        if delegation.revision != expected_revision
            || credential.actor != delegation.target_host_ref
            || delegation.state != harness_core::collaboration::DelegationState::Terminal
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "retention requires exact target Host and terminal Delegation revision",
            ));
        }
        let manifest = control.artifact_manifest(artifact_id)?.ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "artifact manifest does not exist",
            )
        })?;
        if manifest.company_id != control.company_id()
            || manifest.source_node_id != delegation.target_placement.node_id
            || manifest.source_team_id.as_deref()
                != Some(delegation.target_placement.team_id.as_str())
            || manifest.source_work_id.as_deref()
                != delegation
                    .target_work_ref
                    .as_ref()
                    .map(|work| work.work_id.as_str())
            || manifest.deleted_at_unix_ms.is_some()
        {
            return Err(FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "artifact retention scope disagrees with the exact terminal Delegation",
            ));
        }
        let artifact_import = store
            .collaboration_artifact_import(control.company_id(), artifact_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .filter(|import| {
                import.delegation_id == delegation_id
                    && import.source_node_id == delegation.source_node_id
                    && import.source_team_id == delegation.source_team_id
                    && import.source_work_ref == delegation.source_work_ref
                    && import.artifact_digest == manifest.sha256
                    && import.size_bytes == manifest.size_bytes
            })
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::RecoveryRequired,
                    "artifact retention requires one exact canonical source-owned ArtifactImport",
                )
            })?;
        let retain_until = harness_core::collaboration::CollaborationRetentionAnchor {
            terminal_transport_at_unix_ms: manifest.completed_at_unix_ms,
            terminal_delegation_at_unix_ms: unix_ms_timestamp(&delegation.updated_at),
            source_import_completed_at_unix_ms: Some(artifact_import.imported_at_unix_ms),
        }
        .retain_until_unix_ms(request.retention_duration_ms)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "retention cannot start until transport, Delegation, and source durable import are all terminal",
            )
        })?;
        let writer = AuthenticatedActor {
            company_id: control.company_id().into(),
            actor_id: credential.actor.id,
            actor_kind: harness_fabric::ActorKind::AgentMember,
            role_bindings: BTreeSet::from(["artifact_write".into()]),
            session_id: format!("collaboration-artifact-retention:{idempotency_key}"),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(30_000),
        };
        let manifest = control.schedule_collaboration_artifact_retention(
            &writer,
            generation,
            artifact_id,
            request.expected_artifact_revision,
            retain_until,
            now,
        )?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "manifest": manifest,
            "retention_start": retain_until.saturating_sub(request.retention_duration_ms),
        }));
    }
    if method == "POST" && suffix.ends_with("/cancellation-requests") {
        let delegation_id = suffix
            .strip_suffix("/cancellation-requests")
            .filter(|value| !value.is_empty() && !value.contains('/'))
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "cancellation request path is malformed",
                )
            })?;
        let request = serde_json::from_slice::<CollaborationCancellationRequestHttpRequest>(body)
            .map_err(|error| {
            FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
        })?;
        if request.reason.trim().is_empty()
            || request.target_execution_space_id.trim().is_empty()
            || request.expires_unix_ms <= now
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "cancellation request requires reason, exact target Execution Space, and future expiry",
            ));
        }
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        let attestation = store
            .collaboration_source_work_attestation(
                control.company_id(),
                &delegation.source_work_attestation_id,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "Delegation lacks its server-authored source Work attestation",
                )
            })?;
        let authority = harness_store::ResolvedCollaborationAuthority {
            source_host: attestation.source_host_ref,
            source_work_owner: attestation.source_owner_ref,
            target_host: delegation.target_host_ref.clone(),
            target_placement: delegation.target_placement.clone(),
            source_work_application_service: attestation.work_application_service_ref,
            source_gateway_generation: attestation.source_gateway_generation,
        };
        let cancellation = harness_core::collaboration::DelegationCancellationRequest {
            id: format!("delegation-cancellation-request:{idempotency_key}"),
            delegation_id: delegation_id.into(),
            expected_delegation_revision: expected_revision,
            requested_by: credential.actor.clone(),
            reason: request.reason,
            state: harness_core::collaboration::CancellationRequestState::Pending,
            target_host_decision_ref: None,
            revision: 1,
            created_at: format!("unix-ms:{now}"),
            updated_at: format!("unix-ms:{now}"),
        };
        let mutation_context = harness_store::CollaborationMutationContext {
            company_id: control.company_id().into(),
            authenticated_actor: credential.actor.clone(),
            command_name: "delegation_cancel_request".into(),
            idempotency_key: idempotency_key.clone(),
            expected_revision,
            occurred_at: format!("unix-ms:{now}"),
        };
        let pending = store
            .request_delegation_cancellation(&mutation_context, &cancellation, &authority)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
            })?;
        let business = store
            .delegation_cancel_request_operation(
                &harness_store::CollaborationMutationContext {
                    expected_revision: pending.projection.revision,
                    command_name: "delegation_cancel_request_route".into(),
                    ..mutation_context
                },
                &cancellation,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
            })?;
        let routed_actor = AuthenticatedActor {
            expires_at_unix_ms: request.expires_unix_ms,
            ..control_actor.clone()
        };
        let routed = harness_store::route_collaboration_business_operation(
            &business,
            &harness_store::CollaborationFabricRouteContext {
                authenticated_actor: routed_actor.clone(),
                resolved_business_actor: credential.actor,
                source: harness_store::CollaborationFabricSource::ControlPlane,
                control_plane_generation: generation,
                target_execution_space_id: Some(request.target_execution_space_id),
                created_at_unix_ms: now,
                expires_at_unix_ms: request.expires_unix_ms,
            },
        )?;
        let (_, _, receipt, replayed) =
            control.accept_control_plane_operation(generation, &routed_actor, routed, now)?;
        return Ok(serde_json::json!({
            "delegation": pending.projection,
            "cancellation_request": cancellation,
            "operation_id": receipt.operation_id,
            "receipt": receipt,
            "replayed": pending.replayed || replayed,
        }));
    }
    let cancellation_parts = path_parts;
    if method == "POST"
        && cancellation_parts.len() == 4
        && cancellation_parts[1] == "cancellation-requests"
        && matches!(cancellation_parts[3], "accept" | "reject")
    {
        let delegation_id = cancellation_parts[0];
        let request_id = cancellation_parts[2];
        let action = cancellation_parts[3];
        let request = serde_json::from_slice::<CollaborationCancellationDecisionHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        if request.reason.trim().is_empty()
            || request.native_work_event_ref.trim().is_empty()
            || request.target_execution_space_id.trim().is_empty()
            || request.expires_unix_ms <= now
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "cancellation decision requires reason, native Work event, exact target Execution Space, and future expiry",
            ));
        }
        let decision = harness_core::collaboration::DelegationCancellationDecision {
            id: format!("delegation-cancellation-decision:{idempotency_key}"),
            cancellation_request_id: request_id.into(),
            expected_request_revision: 1,
            decision: if action == "accept" {
                harness_core::collaboration::CancellationDecisionKind::Accept
            } else {
                harness_core::collaboration::CancellationDecisionKind::Reject
            },
            decided_by_target_host: credential.actor.clone(),
            native_work_event_ref: request.native_work_event_ref,
            reason: request.reason,
            created_at: format!("unix-ms:{now}"),
        };
        let business = store
            .delegation_cancel_decide_operation(
                &harness_store::CollaborationMutationContext {
                    company_id: control.company_id().into(),
                    authenticated_actor: credential.actor.clone(),
                    command_name: "delegation_cancel_decide".into(),
                    idempotency_key: idempotency_key.clone(),
                    expected_revision,
                    occurred_at: format!("unix-ms:{now}"),
                },
                delegation_id,
                request_id,
                &decision,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
            })?;
        let routed_actor = AuthenticatedActor {
            expires_at_unix_ms: request.expires_unix_ms,
            ..control_actor.clone()
        };
        let routed = harness_store::route_collaboration_business_operation(
            &business,
            &harness_store::CollaborationFabricRouteContext {
                authenticated_actor: routed_actor.clone(),
                resolved_business_actor: credential.actor,
                source: harness_store::CollaborationFabricSource::ControlPlane,
                control_plane_generation: generation,
                target_execution_space_id: Some(request.target_execution_space_id),
                created_at_unix_ms: now,
                expires_at_unix_ms: request.expires_unix_ms,
            },
        )?;
        let (_, _, receipt, replayed) =
            control.accept_control_plane_operation(generation, &routed_actor, routed, now)?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "cancellation_request_id": request_id,
            "operation_id": receipt.operation_id,
            "receipt": receipt,
            "replayed": replayed,
        }));
    }
    let (delegation_id, action) = suffix.rsplit_once('/').ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "collaboration mutation path is malformed",
        )
    })?;
    if method != "POST" || !matches!(action, "accept" | "reject") {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "unknown collaboration mutation",
        ));
    }
    let request = serde_json::from_slice::<CollaborationDecisionHttpRequest>(body)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    if request.reason.trim().is_empty()
        || request.target_execution_space_id.trim().is_empty()
        || request.expires_unix_ms <= now
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "decision requires reason, exact target Execution Space, and future expiry",
        ));
    }
    let decision = harness_core::collaboration::DelegationDecision {
        id: format!("delegation-decision:{idempotency_key}"),
        delegation_id: delegation_id.into(),
        expected_delegation_revision: expected_revision,
        decision: if action == "accept" {
            harness_core::collaboration::DelegationDecisionKind::Accept
        } else {
            harness_core::collaboration::DelegationDecisionKind::Reject
        },
        decided_by_target_host: credential.actor.clone(),
        reason: request.reason,
        created_at: format!("unix-ms:{now}"),
    };
    let business = store
        .delegation_decide_operation(
            &harness_store::CollaborationMutationContext {
                company_id: control.company_id().into(),
                authenticated_actor: credential.actor.clone(),
                command_name: "delegation_decide".into(),
                idempotency_key: idempotency_key.clone(),
                expected_revision,
                occurred_at: format!("unix-ms:{now}"),
            },
            delegation_id,
            &decision,
        )
        .map_err(|error| {
            FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
        })?;
    let control_actor = AuthenticatedActor {
        expires_at_unix_ms: request.expires_unix_ms,
        ..control_actor
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: control_actor.clone(),
            resolved_business_actor: credential.actor,
            source: harness_store::CollaborationFabricSource::ControlPlane,
            control_plane_generation: generation,
            target_execution_space_id: Some(request.target_execution_space_id),
            created_at_unix_ms: now,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    let (_, _, receipt, replayed) =
        control.accept_control_plane_operation(generation, &control_actor, routed, now)?;
    Ok(serde_json::json!({
        "delegation_id": delegation_id,
        "operation_id": receipt.operation_id,
        "receipt": receipt,
        "replayed": replayed,
    }))
}
