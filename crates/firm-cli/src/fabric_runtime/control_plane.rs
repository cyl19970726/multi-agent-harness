use super::*;

pub(super) const GATEWAY_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct Wave6ControlPlaneApplication {
    pub(super) collaboration_root: PathBuf,
    pub(super) company_id: String,
    pub(super) actor_id: String,
}

pub(super) fn current_inbound_policy_matches_delegation(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
) -> bool {
    let policy_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "policy_id": policy.id,
        "policy_revision": policy.revision,
        "mode": policy.mode,
        "allowed_outcome_classes": policy.allowed_outcome_classes,
        "max_active_delegations": policy.max_active_delegations,
    }));
    policy.revoked_at.is_none()
        && policy.id == delegation.inbound_policy_snapshot.policy_id
        && policy.company_id == company_id
        && policy.target_team_id == delegation.target_placement.team_id
        && policy.source_team_id == delegation.source_team_id
        && policy.created_by_target_host == delegation.target_host_ref
        && policy.revision == delegation.inbound_policy_snapshot.policy_revision
        && policy_digest == delegation.inbound_policy_snapshot.policy_digest
}

pub(super) fn validate_current_remote_fact_authority(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    publication: &harness_core::collaboration::RemoteFactPublication,
    reference: &harness_fabric::CollaborationBusinessReference,
    operation: &RoutedOperation,
) -> Result<(), FabricError> {
    let business_actor = harness_core::agentfirm_api::ActorRef {
        kind: match reference.business_actor_kind.as_str() {
            "human" => harness_core::agentfirm_api::ActorKind::Human,
            "agent_member" => harness_core::agentfirm_api::ActorKind::AgentMember,
            "service" => harness_core::agentfirm_api::ActorKind::Service,
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    "remote fact business actor kind is not allowed",
                ))
            }
        },
        id: reference.business_actor_id.clone(),
    };
    if !matches!(
        delegation.state,
        harness_core::collaboration::DelegationState::Active
            | harness_core::collaboration::DelegationState::ResultAvailable
    ) || delegation.revision != reference.expected_revision
        || !current_inbound_policy_matches_delegation(company_id, delegation, policy)
        || publication.company_id != company_id
        || publication.delegation_id != delegation.id
        || publication.created_by != business_actor
        || publication.origin_node_id != operation.source_node_id.clone().unwrap_or_default()
        || publication.origin_node_id != delegation.target_placement.node_id
        || publication.origin_team_id != delegation.target_placement.team_id
        || delegation.target_work_ref.as_ref() != Some(&publication.fact_work_ref)
        || publication.delegation_source_work_ref != delegation.source_work_ref
        || reference.target_team_id != delegation.source_team_id
        || reference.target_team_revision != delegation.source_work_ref.team_revision
        || reference.placement_generation != delegation.source_work_ref.placement_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::ExpectedRevisionConflict,
            "remote fact route disagrees with the current Delegation, inbound policy, Work, placement, or actor",
        ));
    }
    Ok(())
}

pub(super) fn validate_current_delegation_proposal_authority(
    company_id: &str,
    store: &HarnessStore,
    request: &harness_store::ProposeDelegationRequest,
    attestation: &harness_core::collaboration::SourceWorkAttestation,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    reference: &harness_fabric::CollaborationBusinessReference,
    operation: &RoutedOperation,
) -> Result<(), FabricError> {
    let attestation_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "id": attestation.id,
        "company_id": attestation.company_id,
        "source_work_ref": attestation.source_work_ref,
        "source_owner_ref": attestation.source_owner_ref,
        "source_host_ref": attestation.source_host_ref,
        "work_application_service_ref": attestation.work_application_service_ref,
        "source_gateway_generation": attestation.source_gateway_generation,
        "issued_at": attestation.issued_at,
    }));
    let exact_actor = reference.business_actor_kind == "agent_member"
        && (reference.business_actor_id == attestation.source_owner_ref.id
            || reference.business_actor_id == attestation.source_host_ref.id);
    let active_count = store
        .collaboration_delegations(company_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .filter(|delegation| {
            delegation.source_team_id == attestation.source_work_ref.team_id
                && delegation.target_placement.team_id == request.target_placement.team_id
                && delegation.state != harness_core::collaboration::DelegationState::Terminal
        })
        .count() as u64;
    if reference.expected_revision != 0
        || request.source_work_attestation_id != attestation.id
        || attestation.company_id != company_id
        || attestation.attestation_digest != attestation_digest
        || operation.source_node_id.as_deref() != Some(attestation.source_work_ref.node_id.as_str())
        || request.target_placement.team_id != reference.target_team_id
        || request.target_placement.team_revision != reference.target_team_revision
        || request.target_placement.placement_generation != reference.placement_generation
        || request.target_placement.node_id != operation.target_node_id
        || policy.company_id != company_id
        || policy.source_team_id != attestation.source_work_ref.team_id
        || policy.target_team_id != request.target_placement.team_id
        || policy.revoked_at.is_some()
        || !policy
            .allowed_outcome_classes
            .iter()
            .any(|class| class == &request.outcome_class)
        || active_count >= policy.max_active_delegations
        || !exact_actor
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "delegation proposal disagrees with the current inbound policy, source Work attestation, placement, actor, or active limit",
        ));
    }
    Ok(())
}

pub(super) fn validate_current_artifact_grant_authority(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    attestation: &harness_core::collaboration::SourceWorkAttestation,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    manifest: &harness_fabric::RemoteArtifactManifest,
    actor: &harness_core::agentfirm_api::ActorRef,
    expected_revision: u64,
) -> Result<(), FabricError> {
    if !matches!(
        delegation.state,
        harness_core::collaboration::DelegationState::Active
            | harness_core::collaboration::DelegationState::ResultAvailable
    ) || delegation.revision != expected_revision
        || !current_inbound_policy_matches_delegation(company_id, delegation, policy)
        || actor != &delegation.target_host_ref
        || attestation.id != delegation.source_work_attestation_id
        || attestation.company_id != company_id
        || attestation.source_work_ref != delegation.source_work_ref
        || manifest.company_id != company_id
        || manifest.source_node_id != delegation.target_placement.node_id
        || manifest.source_team_id.as_deref() != Some(delegation.target_placement.team_id.as_str())
        || manifest.source_work_id.as_deref()
            != delegation
                .target_work_ref
                .as_ref()
                .map(|work| work.work_id.as_str())
        || manifest.completed_at_unix_ms.is_none()
        || manifest.deleted_at_unix_ms.is_some()
        || !manifest
            .authorized_readers
            .contains(&attestation.source_host_ref.id)
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "artifact grant disagrees with the current Delegation, inbound policy, Work, placement, or target Host",
        ));
    }
    Ok(())
}

pub(super) fn validate_current_collaboration_message_authority(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    attestation: &harness_core::collaboration::SourceWorkAttestation,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    authority: &harness_core::collaboration::CollaborationMessageAuthority,
    reference: &harness_fabric::CollaborationBusinessReference,
) -> Result<(), FabricError> {
    let expected_authority_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "company_id": authority.company_id,
        "delegation_id": authority.delegation_id,
        "delegation_revision": authority.delegation_revision,
        "source_work_ref": authority.source_work_ref,
        "target_work_ref": authority.target_work_ref,
        "target_placement": authority.target_placement,
        "source_owner_ref": authority.source_owner_ref,
        "source_host_ref": authority.source_host_ref,
        "target_host_ref": authority.target_host_ref,
        "inbound_policy_snapshot": authority.inbound_policy_snapshot,
    }));
    let exact_actor = reference.business_actor_kind == "agent_member"
        && (reference.business_actor_id == attestation.source_owner_ref.id
            || reference.business_actor_id == attestation.source_host_ref.id);
    let policy_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "policy_id": policy.id,
        "policy_revision": policy.revision,
        "mode": policy.mode,
        "allowed_outcome_classes": policy.allowed_outcome_classes,
        "max_active_delegations": policy.max_active_delegations,
    }));
    if delegation.state != harness_core::collaboration::DelegationState::Active
        || delegation.revision != reference.expected_revision
        || authority.company_id != company_id
        || authority.delegation_id != delegation.id
        || authority.delegation_revision != delegation.revision
        || authority.source_work_ref != delegation.source_work_ref
        || delegation.target_work_ref.as_ref() != Some(&authority.target_work_ref)
        || authority.target_placement != delegation.target_placement
        || attestation.id != delegation.source_work_attestation_id
        || attestation.company_id != company_id
        || attestation.source_work_ref != delegation.source_work_ref
        || attestation.source_owner_ref != delegation.source_owner_ref
        || authority.source_owner_ref != attestation.source_owner_ref
        || authority.source_host_ref != attestation.source_host_ref
        || authority.target_host_ref != delegation.target_host_ref
        || authority.inbound_policy_snapshot != delegation.inbound_policy_snapshot
        || policy.revoked_at.is_some()
        || policy.id != delegation.inbound_policy_snapshot.policy_id
        || policy.company_id != company_id
        || policy.target_team_id != delegation.target_placement.team_id
        || policy.source_team_id != delegation.source_team_id
        || policy.created_by_target_host != delegation.target_host_ref
        || policy.revision != delegation.inbound_policy_snapshot.policy_revision
        || policy_digest != delegation.inbound_policy_snapshot.policy_digest
        || authority.authority_digest != expected_authority_digest
        || reference.target_team_id != delegation.target_placement.team_id
        || reference.target_team_revision != delegation.target_placement.team_revision
        || reference.placement_generation != delegation.target_placement.placement_generation
        || !exact_actor
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "team Message route disagrees with the current accepted Delegation, Work, placement, inbound policy, or source actor",
        ));
    }
    Ok(())
}

impl ControlPlaneReceiptApplication for Wave6ControlPlaneApplication {
    fn admit_and_accept_source(
        &self,
        operation: &RoutedOperation,
        _authenticated_actor: &AuthenticatedActor,
        accept: &mut dyn FnMut() -> Result<
            harness_fabric::gateway_runtime::AcceptedSourceOperation,
            FabricError,
        >,
    ) -> Result<harness_fabric::gateway_runtime::AcceptedSourceOperation, FabricError> {
        if operation.kind != COLLABORATION_BUSINESS_OPERATION_KIND {
            return accept();
        }
        let reference = match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
            _ => return accept(),
        };
        if reference.business_kind == "delegation_propose" {
            let request = serde_json::from_value::<harness_store::ProposeDelegationRequest>(
                reference.payload.get("request").cloned().ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        "delegation proposal route lacks its frozen request",
                    )
                })?,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
            let attestation =
                serde_json::from_value::<harness_core::collaboration::SourceWorkAttestation>(
                    reference
                        .payload
                        .get("source_work_attestation")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::InvalidPayload,
                                "delegation proposal route lacks source Work attestation",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
                })?;
            let policy_id = reference
                .payload
                .get("policy_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        "delegation proposal route lacks policy_id",
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            return store.with_collaboration_authority_fence(
                |locked_store| {
                    let policy = locked_store
                        .collaboration_inbound_policy(&self.company_id, policy_id)
                        .map_err(|error| {
                            FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "delegation proposal inbound policy is missing",
                            )
                        })?;
                    validate_current_delegation_proposal_authority(
                        &self.company_id,
                        locked_store,
                        &request,
                        &attestation,
                        &policy,
                        &reference,
                        operation,
                    )
                },
                accept,
            );
        }
        if reference.business_kind == "team_message_deliver" {
            let authority = serde_json::from_value::<
                harness_core::collaboration::CollaborationMessageAuthority,
            >(
                reference
                    .payload
                    .get("delegation_authority")
                    .cloned()
                    .ok_or_else(|| {
                        FabricError::none(
                            FabricErrorCode::UnauthorizedActor,
                            "team Message route lacks frozen Delegation authority",
                        )
                    })?,
            )
            .map_err(|error| {
                FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    format!("team Message Delegation authority is invalid: {error}"),
                )
            })?;
            let store = HarnessStore::new(&self.collaboration_root);
            return store.with_collaboration_authority_fence(
                |locked_store| {
                    let delegation = locked_store
                        .collaboration_delegation(&self.company_id, &authority.delegation_id)
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "team Message references no central Delegation",
                            )
                        })?;
                    let attestation = locked_store
                        .collaboration_source_work_attestation(
                            &self.company_id,
                            &delegation.source_work_attestation_id,
                        )
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "team Message Delegation lacks source Work attestation",
                            )
                        })?;
                    let policy = locked_store
                        .collaboration_inbound_policy(
                            &self.company_id,
                            &delegation.inbound_policy_snapshot.policy_id,
                        )
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .filter(|policy| policy.revoked_at.is_none())
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "team Message inbound policy is missing or revoked",
                            )
                        })?;
                    validate_current_collaboration_message_authority(
                        &self.company_id,
                        &delegation,
                        &attestation,
                        &policy,
                        &authority,
                        &reference,
                    )
                },
                accept,
            );
        }
        if reference.business_kind == "remote_fact_publish" {
            let publication =
                serde_json::from_value::<harness_core::collaboration::RemoteFactPublication>(
                    reference
                        .payload
                        .get("publication")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::InvalidPayload,
                                "remote fact route lacks the immutable publication",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        format!("remote fact publication is invalid: {error}"),
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            return store.with_collaboration_authority_fence(
                |locked_store| {
                    let delegation = locked_store
                        .collaboration_delegation(&self.company_id, &publication.delegation_id)
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::ExpectedRevisionConflict,
                                "remote fact references no central Delegation",
                            )
                        })?;
                    let policy = locked_store
                        .collaboration_inbound_policy(
                            &self.company_id,
                            &delegation.inbound_policy_snapshot.policy_id,
                        )
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "remote fact inbound policy is missing",
                            )
                        })?;
                    validate_current_remote_fact_authority(
                        &self.company_id,
                        &delegation,
                        &policy,
                        &publication,
                        &reference,
                        operation,
                    )
                },
                accept,
            );
        }
        accept()
    }

    fn fold_source_accepted(
        &self,
        operation: &RoutedOperation,
        receipt: &RouteReceipt,
        observed_at_unix_ms: u64,
    ) -> Result<Vec<harness_fabric::gateway_runtime::ControlPlaneFollowUp>, FabricError> {
        if operation.kind != COLLABORATION_BUSINESS_OPERATION_KIND
            || receipt.kind != ReceiptKind::ControlPlaneAccepted
        {
            return Ok(Vec::new());
        }
        let reference = match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
            _ => return Ok(Vec::new()),
        };
        if matches!(
            reference.business_kind.as_str(),
            "team_message_deliver" | "remote_fact_publish"
        ) {
            // Source authority was already resolved while the canonical
            // collaboration lock was held through the Fabric commit. Never
            // perform a second mutable-authority fold here: cancellation or a
            // policy revision immediately after that linearization point is a
            // later event, not grounds to reject an already accepted route or
            // strand a committed attempt without its response.
            if reference.business_kind == "team_message_deliver" {
                return Ok(Vec::new());
            }
        } else {
            return Ok(Vec::new());
        }
        let publication = serde_json::from_value::<
            harness_core::collaboration::RemoteFactPublication,
        >(reference.payload.get("publication").cloned().ok_or_else(
            || {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "remote fact route lacks the immutable publication",
                )
            },
        )?)
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("remote fact publication is invalid: {error}"),
            )
        })?;
        let store = HarnessStore::new(&self.collaboration_root);
        let delegation = store
            .collaboration_delegation(&self.company_id, &publication.delegation_id)
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("remote fact Delegation lookup failed: {error}"),
                )
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "remote fact references no central Delegation",
                )
            })?;
        let business_actor = harness_core::agentfirm_api::ActorRef {
            kind: match reference.business_actor_kind.as_str() {
                "human" => harness_core::agentfirm_api::ActorKind::Human,
                "agent_member" => harness_core::agentfirm_api::ActorKind::AgentMember,
                "service" => harness_core::agentfirm_api::ActorKind::Service,
                _ => {
                    return Err(FabricError::none(
                        FabricErrorCode::UnauthorizedActor,
                        "remote fact business actor kind is not allowed",
                    ))
                }
            },
            id: reference.business_actor_id.clone(),
        };
        if publication.created_by != business_actor
            || publication.origin_node_id != operation.source_node_id.clone().unwrap_or_default()
            || publication.origin_team_id != delegation.target_placement.team_id
        {
            return Err(FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "remote fact changed actor, source Node/Team, or Delegation revision",
            ));
        }
        let context = harness_store::CollaborationMutationContext {
            company_id: self.company_id.clone(),
            authenticated_actor: business_actor.clone(),
            command_name: "remote_fact_publish".into(),
            idempotency_key: operation.idempotency_key.clone(),
            expected_revision: 0,
            occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
        };
        store
            .publish_remote_fact(
                &context,
                &publication,
                &[business_actor, delegation.target_host_ref.clone()],
                &delegation.target_placement,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("Control Plane remote fact fold failed: {error}"),
                )
            })?;
        if let Some(operational_decision) = publication.operational_decision_ref.as_ref() {
            let attestation = store
                .collaboration_source_work_attestation(
                    &self.company_id,
                    &delegation.source_work_attestation_id,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Work attestation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "accepted remote result has no frozen source Work attestation",
                    )
                })?;
            let result_context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: delegation.target_host_ref.clone(),
                command_name: "delegation.result_available".into(),
                idempotency_key: format!("{}:result-available", operation.idempotency_key),
                expected_revision: reference.expected_revision,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            store
                .mark_delegation_result_available(
                    &result_context,
                    &delegation.id,
                    &publication.id,
                    operational_decision,
                    &harness_store::ResolvedCollaborationAuthority {
                        source_host: attestation.source_host_ref,
                        source_work_owner: attestation.source_owner_ref,
                        target_host: delegation.target_host_ref.clone(),
                        target_placement: delegation.target_placement.clone(),
                        source_work_application_service: attestation.work_application_service_ref,
                        source_gateway_generation: attestation.source_gateway_generation,
                    },
                    &delegation.target_placement,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane result-available fold failed: {error}"),
                    )
                })?;
        }
        Ok(Vec::new())
    }

    fn fold_target_application(
        &self,
        operation: &RoutedOperation,
        result: &TargetApplicationResult,
        receipt: &RouteReceipt,
        observed_at_unix_ms: u64,
    ) -> Result<Vec<harness_fabric::gateway_runtime::ControlPlaneFollowUp>, FabricError> {
        if operation.kind != COLLABORATION_BUSINESS_OPERATION_KIND {
            return Ok(Vec::new());
        }
        if receipt.kind != ReceiptKind::OperationApplied
            || receipt.application_effect != Some(EffectCertainty::Applied)
            || result.effect != EffectCertainty::Applied
        {
            return Ok(Vec::new());
        }
        let reference = match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
            _ => return Ok(Vec::new()),
        };
        if reference.business_kind == "delegation_propose" {
            if result.result_schema != "agentfirm.collaboration.delegation_proposal_validated.v1" {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "delegation proposal applied receipt has an unexpected result schema",
                ));
            }
            let request = serde_json::from_value::<harness_store::ProposeDelegationRequest>(
                reference.payload.get("request").cloned().ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "proposal route lacks the frozen request",
                    )
                })?,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("proposal request is invalid: {error}"),
                )
            })?;
            let attestation =
                serde_json::from_value::<harness_core::collaboration::SourceWorkAttestation>(
                    reference
                        .payload
                        .get("source_work_attestation")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::unknown(
                                operation.id.clone(),
                                "proposal route lacks source Work attestation",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Work attestation is invalid: {error}"),
                    )
                })?;
            let policy_id = reference
                .payload
                .get("policy_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(operation.id.clone(), "proposal route lacks policy_id")
                })?;
            let target_host = serde_json::from_value::<harness_core::agentfirm_api::ActorRef>(
                result
                    .result
                    .get("target_host_ref")
                    .cloned()
                    .ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "proposal validation lacks target Host",
                        )
                    })?,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("target Host result is invalid: {error}"),
                )
            })?;
            let store = HarnessStore::new(&self.collaboration_root);
            let attestation_context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: attestation.work_application_service_ref.clone(),
                command_name: "import_source_work_attestation".into(),
                idempotency_key: format!("attestation:{}", operation.id),
                expected_revision: 0,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            store
                .put_source_work_attestation(
                    &attestation_context,
                    &attestation,
                    &attestation.work_application_service_ref,
                    operation.source_gateway_generation.unwrap_or_default(),
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Work attestation import failed: {error}"),
                    )
                })?;
            let policy = store
                .collaboration_inbound_policy(&self.company_id, policy_id)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("target inbound policy lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::UnauthorizedActor,
                        "target inbound policy is not centrally registered",
                    )
                })?;
            let business_actor = harness_core::agentfirm_api::ActorRef {
                kind: match reference.business_actor_kind.as_str() {
                    "human" => harness_core::agentfirm_api::ActorKind::Human,
                    "agent_member" => harness_core::agentfirm_api::ActorKind::AgentMember,
                    "service" => harness_core::agentfirm_api::ActorKind::Service,
                    _ => {
                        return Err(FabricError::none(
                            FabricErrorCode::UnauthorizedActor,
                            "proposal business actor kind is not allowed",
                        ))
                    }
                },
                id: reference.business_actor_id.clone(),
            };
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: attestation.source_host_ref.clone(),
                source_work_owner: attestation.source_owner_ref.clone(),
                target_host,
                target_placement: request.target_placement.clone(),
                source_work_application_service: attestation.work_application_service_ref.clone(),
                source_gateway_generation: operation.source_gateway_generation.unwrap_or_default(),
            };
            let context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: business_actor,
                command_name: "delegation_propose".into(),
                idempotency_key: operation.idempotency_key.clone(),
                expected_revision: reference.expected_revision,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            store
                .propose_collaboration_delegation(&context, &request, &authority, &policy)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane delegation fold failed: {error}"),
                    )
                })?;
            return Ok(Vec::new());
        }
        if reference.business_kind == "delegation_decide" {
            if result.result_schema != "agentfirm.collaboration.delegation_decision_validated.v1" {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "delegation decision applied receipt has an unexpected result schema",
                ));
            }
            let decision =
                serde_json::from_value::<harness_core::collaboration::DelegationDecision>(
                    result.result.get("decision").cloned().ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "delegation decision result lacks the frozen decision",
                        )
                    })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("delegation decision result is invalid: {error}"),
                    )
                })?;
            let observed =
                serde_json::from_value::<harness_core::collaboration::TargetPlacementRef>(
                    result
                        .result
                        .get("target_placement")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::unknown(
                                operation.id.clone(),
                                "delegation decision result lacks target placement",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("delegation decision placement is invalid: {error}"),
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            let delegation_id = result
                .result
                .get("delegation_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "delegation decision result lacks delegation_id",
                    )
                })?;
            let before = store
                .collaboration_delegation(&self.company_id, delegation_id)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("delegation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "delegation decision references no central relationship",
                    )
                })?;
            let business_actor = harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: reference.business_actor_id.clone(),
            };
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: before.source_owner_ref.clone(),
                source_work_owner: before.source_owner_ref.clone(),
                target_host: before.target_host_ref.clone(),
                target_placement: before.target_placement.clone(),
                source_work_application_service: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: format!("source-work-service:{}", before.source_node_id),
                },
                source_gateway_generation: operation.source_gateway_generation.unwrap_or_default(),
            };
            let context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: business_actor,
                command_name: "delegation_decide".into(),
                idempotency_key: operation.idempotency_key.clone(),
                expected_revision: reference.expected_revision,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            let decided = store
                .decide_collaboration_delegation(
                    &context,
                    delegation_id,
                    &decision,
                    &authority,
                    &observed,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane delegation decision fold failed: {error}"),
                    )
                })?;
            if decided.projection.state
                != harness_core::collaboration::DelegationState::ProvisioningTargetWork
            {
                return Ok(Vec::new());
            }
            let target_work = store
                .target_work_create_operation(
                    &self.company_id,
                    delegation_id,
                    &format!("unix-ms:{observed_at_unix_ms}"),
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("target Work follow-up build failed: {error}"),
                    )
                })?;
            let control_actor = AuthenticatedActor {
                company_id: self.company_id.clone(),
                actor_id: self.actor_id.clone(),
                actor_kind: harness_fabric::ActorKind::Service,
                role_bindings: BTreeSet::from([
                    "company_control_plane".into(),
                    "fabric_submit".into(),
                ]),
                session_id: format!("control-plane:{}", operation.control_plane_generation),
                issued_at_unix_ms: observed_at_unix_ms,
                expires_at_unix_ms: observed_at_unix_ms.saturating_add(30_000),
            };
            let routed = harness_store::route_collaboration_business_operation(
                &target_work,
                &harness_store::CollaborationFabricRouteContext {
                    authenticated_actor: control_actor.clone(),
                    resolved_business_actor: before.target_host_ref,
                    source: harness_store::CollaborationFabricSource::ControlPlane,
                    control_plane_generation: operation.control_plane_generation,
                    target_execution_space_id: operation.target_execution_space_id.clone(),
                    created_at_unix_ms: observed_at_unix_ms,
                    expires_at_unix_ms: observed_at_unix_ms.saturating_add(10 * 60 * 1000),
                },
            )?;
            return Ok(vec![
                harness_fabric::gateway_runtime::ControlPlaneFollowUp {
                    authenticated_actor: control_actor,
                    operation: routed,
                },
            ]);
        }
        if reference.business_kind == "delegation_cancel_decide" {
            if result.result_schema != "agentfirm.collaboration.cancellation_decision_validated.v1"
            {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "cancellation decision applied receipt has an unexpected result schema",
                ));
            }
            let delegation_id = result
                .result
                .get("delegation_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision result lacks delegation_id",
                    )
                })?;
            let request_id = result
                .result
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision result lacks request_id",
                    )
                })?;
            let decision = serde_json::from_value::<
                harness_core::collaboration::DelegationCancellationDecision,
            >(result.result.get("decision").cloned().ok_or_else(
                || {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision result lacks decision",
                    )
                },
            )?)
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("cancellation decision result is invalid: {error}"),
                )
            })?;
            let observed =
                serde_json::from_value::<harness_core::collaboration::TargetPlacementRef>(
                    result
                        .result
                        .get("target_placement")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::unknown(
                                operation.id.clone(),
                                "cancellation decision result lacks target placement",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("cancellation placement is invalid: {error}"),
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            let delegation = store
                .collaboration_delegation(&self.company_id, delegation_id)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("cancellation Delegation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision references no Delegation",
                    )
                })?;
            let source_attestation = store
                .collaboration_source_work_attestation(
                    &self.company_id,
                    &delegation.source_work_attestation_id,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Host attestation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision has no source Host attestation",
                    )
                })?;
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: source_attestation.source_host_ref,
                source_work_owner: delegation.source_owner_ref.clone(),
                target_host: delegation.target_host_ref.clone(),
                target_placement: delegation.target_placement.clone(),
                source_work_application_service: source_attestation.work_application_service_ref,
                source_gateway_generation: source_attestation.source_gateway_generation,
            };
            store
                .decide_delegation_cancellation(
                    &harness_store::CollaborationMutationContext {
                        company_id: self.company_id.clone(),
                        authenticated_actor: delegation.target_host_ref,
                        command_name: "delegation_cancel_decide".into(),
                        idempotency_key: operation.idempotency_key.clone(),
                        expected_revision: reference.expected_revision,
                        occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
                    },
                    delegation_id,
                    request_id,
                    &decision,
                    &authority,
                    &observed,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane cancellation decision fold failed: {error}"),
                    )
                })?;
            return Ok(Vec::new());
        }
        if reference.business_kind == "artifact_grant" {
            if result.result_schema != "agentfirm.collaboration.artifact_imported.v1" {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "artifact grant receipt is not a canonical source ArtifactImport",
                ));
            }
            let import = serde_json::from_value::<harness_core::collaboration::ArtifactImport>(
                result
                    .result
                    .get("artifact_import")
                    .cloned()
                    .ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "artifact import result is absent",
                        )
                    })?,
            )
            .map_err(|error| FabricError::unknown(operation.id.clone(), error.to_string()))?;
            let store = HarnessStore::new(&self.collaboration_root);
            let control_actor = harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::Service,
                id: self.actor_id.clone(),
            };
            store
                .record_collaboration_artifact_import(
                    &harness_store::CollaborationMutationContext {
                        company_id: self.company_id.clone(),
                        authenticated_actor: control_actor.clone(),
                        command_name: "artifact_import.fold".into(),
                        idempotency_key: format!("artifact-import:{}", operation.idempotency_key),
                        expected_revision: 0,
                        occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
                    },
                    &import,
                    &operation.id,
                    &control_actor,
                )
                .map_err(|error| FabricError::unknown(operation.id.clone(), error.to_string()))?;
            return Ok(Vec::new());
        }
        if reference.business_kind != "target_work_create" {
            return Ok(Vec::new());
        }
        if result.result_schema != "agentfirm.collaboration.target_work_created.v1" {
            return Err(FabricError::unknown(
                operation.id.clone(),
                "target Work applied receipt has an unexpected result schema",
            ));
        }
        let target_work_ref = serde_json::from_value::<harness_core::collaboration::RemoteWorkRef>(
            result
                .result
                .get("target_work_ref")
                .cloned()
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "target Work applied receipt lacks target_work_ref",
                    )
                })?,
        )
        .map_err(|error| {
            FabricError::unknown(
                operation.id.clone(),
                format!("target Work reference is invalid: {error}"),
            )
        })?;
        let store = HarnessStore::new(&self.collaboration_root);
        let control_actor = harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: self.actor_id.clone(),
        };
        let context = harness_store::CollaborationMutationContext {
            company_id: self.company_id.clone(),
            authenticated_actor: control_actor.clone(),
            command_name: "fold_target_work_created".into(),
            idempotency_key: format!("fold:{}", operation.id),
            expected_revision: reference.expected_revision,
            occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
        };
        let observed = harness_core::collaboration::TargetPlacementRef {
            team_id: reference.target_team_id,
            team_revision: reference.target_team_revision,
            node_id: operation.target_node_id.clone(),
            placement_generation: reference.placement_generation,
        };
        store
            .apply_target_work_created(
                &context,
                result
                    .result
                    .get("target_work_ref")
                    .and_then(|_| {
                        operation
                            .idempotency_key
                            .strip_prefix("target-work-create:")
                    })
                    .ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "target Work operation has no canonical delegation idempotency binding",
                        )
                    })?,
                &target_work_ref,
                &observed,
                &operation.id,
                &control_actor,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("Control Plane collaboration fold failed: {error}"),
                )
            })?;
        Ok(Vec::new())
    }
}

pub(super) fn remote_fabric_schema_bundle_digest() -> String {
    harness_fabric::sha256_hex(include_bytes!(
        "../../../../schemas/remote-fabric/schema-bundle.v1.json"
    ))
}
