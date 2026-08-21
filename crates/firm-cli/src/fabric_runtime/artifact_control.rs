use super::*;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationDecisionHttpRequest {
    pub(super) reason: String,
    pub(super) target_execution_space_id: String,
    pub(super) expires_unix_ms: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationInboundPolicyHttpRequest {
    pub(super) policy_id: String,
    pub(super) target_team_id: String,
    pub(super) source_team_id: String,
    pub(super) mode: harness_core::collaboration::DelegationInboundMode,
    pub(super) allowed_outcome_classes: Vec<String>,
    pub(super) max_active_delegations: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationCancellationRequestHttpRequest {
    pub(super) reason: String,
    pub(super) target_execution_space_id: String,
    pub(super) expires_unix_ms: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationCancellationDecisionHttpRequest {
    pub(super) reason: String,
    pub(super) native_work_event_ref: String,
    pub(super) target_execution_space_id: String,
    pub(super) expires_unix_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationArtifactGrantHttpRequest {
    pub(super) target_execution_space_id: String,
    pub(super) expires_unix_ms: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationArtifactInitiateHttpRequest {
    pub(super) artifact_id: String,
    pub(super) operation_id: Option<String>,
    pub(super) media_type: String,
    pub(super) size_bytes: u64,
    pub(super) sha256: String,
    pub(super) classification: harness_fabric::ArtifactClassification,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationArtifactRetentionHttpRequest {
    pub(super) expected_artifact_revision: u64,
    pub(super) retention_duration_ms: u64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CollaborationArtifactGrantEnvelope {
    pub(super) delegation_id: String,
    pub(super) delegation: harness_core::collaboration::WorkDelegationV1,
    pub(super) source_work_attestation: harness_core::collaboration::SourceWorkAttestation,
    pub(super) manifest: harness_fabric::RemoteArtifactManifest,
    pub(super) read_capability: harness_fabric::ArtifactCapability,
    pub(super) source_placement: harness_core::collaboration::TargetPlacementRef,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_frozen_artifact_grant_replay(
    operations: &std::collections::BTreeMap<String, RoutedOperation>,
    receipts: &std::collections::BTreeMap<String, RouteReceipt>,
    company_id: &str,
    delegation_id: &str,
    artifact_id: &str,
    idempotency_key: &str,
    expected_revision: u64,
    target_execution_space_id: &str,
    expires_unix_ms: u64,
    credential_actor: &harness_core::agentfirm_api::ActorRef,
) -> Result<Option<RouteReceipt>, FabricError> {
    let operation_id = format!("route-artifact-grant:{delegation_id}:{artifact_id}");
    let Some(existing) = operations.get(&operation_id) else {
        return Ok(None);
    };
    let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
        existing.closed_body()?
    else {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "artifact grant identity belongs to another operation kind",
        ));
    };
    let payload: CollaborationArtifactGrantEnvelope =
        serde_json::from_value(reference.payload.clone()).map_err(|error| {
            FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
        })?;
    let credential_kind = match credential_actor.kind {
        harness_core::agentfirm_api::ActorKind::Human => "human",
        harness_core::agentfirm_api::ActorKind::AgentMember => "agent_member",
        harness_core::agentfirm_api::ActorKind::External
        | harness_core::agentfirm_api::ActorKind::Service => "service",
    };
    let exact = existing.company_id == company_id
        && existing.idempotency_key == idempotency_key
        && existing.expected_target_revision == Some(expected_revision)
        && existing.target_execution_space_id.as_deref() == Some(target_execution_space_id)
        && existing.actor.expires_at_unix_ms == expires_unix_ms
        && reference.business_kind == "artifact_grant"
        && reference.business_actor_kind == credential_kind
        && reference.business_actor_id == credential_actor.id
        && payload.delegation_id == delegation_id
        && payload.delegation.id == delegation_id
        && payload.delegation.revision == expected_revision
        && payload.delegation.target_host_ref == *credential_actor
        && payload.manifest.id == artifact_id;
    if !exact {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "same artifact grant identity was reused with a different request",
        ));
    }
    receipts
        .values()
        .filter(|receipt| {
            receipt.operation_id == operation_id
                && receipt.kind == ReceiptKind::ControlPlaneAccepted
        })
        .min_by_key(|receipt| (receipt.created_at_unix_ms, receipt.id.as_str()))
        .cloned()
        .map(Some)
        .ok_or_else(|| {
            FabricError::unknown(
                operation_id,
                "accepted artifact grant has no durable ControlPlane receipt",
            )
        })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn grant_collaboration_artifact<K: harness_fabric::ArtifactKeyBackend>(
    store: &HarnessStore,
    control: &ControlPlane<'_, K>,
    generation: u64,
    delegation_id: &str,
    artifact_id: &str,
    request: &CollaborationArtifactGrantHttpRequest,
    credential_actor: &harness_core::agentfirm_api::ActorRef,
    control_actor: &AuthenticatedActor,
    idempotency_key: &str,
    expected_revision: u64,
    now: u64,
) -> Result<serde_json::Value, FabricError> {
    if request.target_execution_space_id.trim().is_empty() || request.expires_unix_ms <= now {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "artifact grant requires exact source Execution Space and future expiry",
        ));
    }
    let operation_id = format!("route-artifact-grant:{delegation_id}:{artifact_id}");
    let frozen_replay = std::cell::RefCell::new(None::<RouteReceipt>);
    store.with_collaboration_authority_fence(
        |locked_store| {
            // Replay resolution and mutable collaboration validation share the
            // same writer fence. Two identical requests therefore cannot both
            // miss the frozen operation before one mints a time-dependent
            // one-use capability and commits the canonical Fabric route.
            let fabric_state = control.store().snapshot()?;
            if let Some(receipt) = resolve_frozen_artifact_grant_replay(
                &fabric_state.operations,
                &fabric_state.receipts,
                control.company_id(),
                delegation_id,
                artifact_id,
                idempotency_key,
                expected_revision,
                &request.target_execution_space_id,
                request.expires_unix_ms,
                credential_actor,
            )? {
                frozen_replay.replace(Some(receipt));
                return Ok(());
            }
            let delegation = locked_store
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
            let attestation = locked_store
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
            let policy = locked_store
                .collaboration_inbound_policy(
                    control.company_id(),
                    &delegation.inbound_policy_snapshot.policy_id,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::UnauthorizedActor,
                        "artifact grant inbound policy is missing",
                    )
                })?;
            let manifest = control.artifact_manifest(artifact_id)?.ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ArtifactInvalid,
                    "artifact manifest does not exist",
                )
            })?;
            validate_current_artifact_grant_authority(
                control.company_id(),
                &delegation,
                &attestation,
                &policy,
                &manifest,
                credential_actor,
                expected_revision,
            )
        },
        || {
            if let Some(receipt) = frozen_replay.borrow_mut().take() {
                return Ok(serde_json::json!({
                    "delegation_id": delegation_id,
                    "artifact_id": artifact_id,
                    "operation_id": operation_id,
                    "receipt": receipt,
                    "replayed": true,
                }));
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
                        "Delegation lacks source Host attestation",
                    )
                })?;
            let manifest = control.artifact_manifest(artifact_id)?.ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ArtifactInvalid,
                    "artifact manifest does not exist",
                )
            })?;
            let grantor = AuthenticatedActor {
                company_id: control.company_id().into(),
                actor_id: credential_actor.id.clone(),
                actor_kind: match credential_actor.kind {
                    harness_core::agentfirm_api::ActorKind::Human => {
                        harness_fabric::ActorKind::Human
                    }
                    harness_core::agentfirm_api::ActorKind::AgentMember => {
                        harness_fabric::ActorKind::AgentMember
                    }
                    harness_core::agentfirm_api::ActorKind::External
                    | harness_core::agentfirm_api::ActorKind::Service => {
                        harness_fabric::ActorKind::Service
                    }
                },
                role_bindings: BTreeSet::from(["artifact_write".into()]),
                session_id: format!("collaboration-artifact:{idempotency_key}"),
                issued_at_unix_ms: now,
                expires_at_unix_ms: request.expires_unix_ms,
            };
            let capability = control.issue_delegated_download_capability(
                &grantor,
                generation,
                artifact_id,
                &attestation.source_host_ref.id,
                &delegation.source_node_id,
                now,
            )?;
            let business = store
                .artifact_grant_operation(
                    &harness_store::CollaborationMutationContext {
                        company_id: control.company_id().into(),
                        authenticated_actor: credential_actor.clone(),
                        command_name: "artifact_grant".into(),
                        idempotency_key: idempotency_key.into(),
                        expected_revision,
                        occurred_at: format!("unix-ms:{now}"),
                    },
                    delegation_id,
                    &manifest,
                    &capability,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::UnauthorizedActor, error.to_string())
                })?;
            let routed_actor = AuthenticatedActor {
                expires_at_unix_ms: request.expires_unix_ms,
                ..control_actor.clone()
            };
            let routed = harness_store::route_collaboration_business_operation(
                &business,
                &harness_store::CollaborationFabricRouteContext {
                    authenticated_actor: routed_actor.clone(),
                    resolved_business_actor: credential_actor.clone(),
                    source: harness_store::CollaborationFabricSource::ControlPlane,
                    control_plane_generation: generation,
                    target_execution_space_id: Some(request.target_execution_space_id.clone()),
                    created_at_unix_ms: now,
                    expires_at_unix_ms: request.expires_unix_ms,
                },
            )?;
            let (_, _, receipt, replayed) =
                control.accept_control_plane_operation(generation, &routed_actor, routed, now)?;
            Ok(serde_json::json!({
                "delegation_id": delegation_id,
                "artifact_id": artifact_id,
                "operation_id": receipt.operation_id,
                "receipt": receipt,
                "replayed": replayed,
            }))
        },
    )
}

pub(super) fn validate_artifact_grant_authority(
    payload: &CollaborationArtifactGrantEnvelope,
    capability: &harness_fabric::ArtifactCapability,
    reference: &harness_fabric::CollaborationBusinessReference,
    operation: &harness_fabric::RoutedOperation,
    source_node_id: &str,
) -> Result<(), FabricError> {
    let actor_matches = payload.delegation.target_host_ref.id == reference.business_actor_id
        && matches!(
            (
                &payload.delegation.target_host_ref.kind,
                reference.business_actor_kind.as_str()
            ),
            (harness_core::agentfirm_api::ActorKind::Human, "human")
                | (
                    harness_core::agentfirm_api::ActorKind::AgentMember,
                    "agent_member"
                )
        );
    if &payload.read_capability != capability
        || capability.node_id != source_node_id
        || capability.company_id != operation.company_id
        || capability.purpose != harness_fabric::ArtifactCapabilityPurpose::Download
        || payload.delegation.id != payload.delegation_id
        || payload.delegation.company_id != operation.company_id
        || payload.delegation.source_node_id != source_node_id
        || payload.delegation.source_work_attestation_id != payload.source_work_attestation.id
        || payload.source_work_attestation.company_id != operation.company_id
        || payload.source_work_attestation.source_work_ref != payload.delegation.source_work_ref
        || payload.source_work_attestation.source_owner_ref != payload.delegation.source_owner_ref
        || payload.source_work_attestation.source_host_ref.id != capability.issued_to
        || !actor_matches
        || payload.source_placement.team_id != payload.delegation.source_team_id
        || payload.source_placement.team_revision
            != payload.delegation.source_work_ref.team_revision
        || payload.source_placement.node_id != source_node_id
        || payload.source_placement.placement_generation
            != payload.delegation.source_work_ref.placement_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "artifact capability or frozen central collaboration authority changed or is outside the authenticated source Node",
        ));
    }
    Ok(())
}

pub(super) fn unix_ms_timestamp(value: &str) -> Option<u64> {
    value.strip_prefix("unix-ms:")?.parse().ok()
}

pub(super) fn collaboration_actor_can_read_delegation(
    store: &HarnessStore,
    company_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    delegation: &harness_core::collaboration::WorkDelegationV1,
) -> Result<bool, FabricError> {
    let source_host = store
        .collaboration_source_work_attestation(company_id, &delegation.source_work_attestation_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .map(|attestation| attestation.source_host_ref);
    Ok(actor == &delegation.source_owner_ref
        || source_host.as_ref() == Some(actor)
        || actor == &delegation.target_host_ref)
}

pub(crate) fn encode_collaboration_cursor(
    cursor: &harness_store::CollaborationScopedCursor,
    secret: &str,
) -> Result<String, FabricError> {
    let bytes = serde_json::to_vec(cursor)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let payload = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "collaboration cursor signing key is invalid",
        )
    })?;
    mac.update(payload.as_bytes());
    let signature = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("v1.{payload}.{signature}"))
}

pub(crate) fn decode_collaboration_cursor(
    encoded: &str,
    secret: &str,
) -> Result<harness_store::CollaborationScopedCursor, FabricError> {
    let mut parts = encoded.split('.');
    let (Some("v1"), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "collaboration cursor is malformed",
        ));
    };
    let signature_bytes = (0..signature.len())
        .step_by(2)
        .map(|offset| {
            signature
                .get(offset..offset + 2)
                .ok_or(())
                .and_then(|value| u8::from_str_radix(value, 16).map_err(|_| ()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "collaboration cursor signature is invalid",
            )
        })?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "collaboration cursor signing key is invalid",
        )
    })?;
    mac.update(payload.as_bytes());
    if mac.verify_slice(&signature_bytes).is_err() || payload.len() % 2 != 0 {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "collaboration cursor signature is invalid",
        ));
    }
    let bytes = (0..payload.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&payload[offset..offset + 2], 16).map_err(|_| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "collaboration cursor encoding is invalid",
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::from_slice(&bytes)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))
}
