use super::*;

impl HarnessStore {
    pub fn target_work_create_operation(
        &self,
        company_id: &str,
        delegation_id: &str,
        created_at: &str,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                company_id,
                "work_delegation_v1",
                delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if delegation.state != DelegationState::ProvisioningTargetWork {
            return Err(collaboration_error(
                FabricErrorCode::DelegationTerminal,
                "delegation is not ready to provision target Work",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "requested_outcome": delegation.requested_outcome,
            "acceptance_contract": delegation.acceptance_contract,
            "source_work_ref": delegation.source_work_ref,
            "target_placement": delegation.target_placement,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-target-work-{}", delegation.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: company_id.into(),
            kind: RoutedBusinessKind::TargetWorkCreate,
            authenticated_actor: delegation.target_host_ref,
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: delegation.revision,
            idempotency_key: format!("target-work-create:{}", delegation.id),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: "collaboration.target_work_create".into(),
            ordering_key: format!("delegation:{}", delegation.id),
            created_at: created_at.into(),
        })
    }

    /// Build the source Node-authored proposal envelope from the immutable
    /// WorkApplicationService attestation. The public request contributes only
    /// desired outcome and target identity; it cannot select Work/owner facts.
    pub fn delegation_propose_operation(
        &self,
        context: &CollaborationMutationContext,
        request: &ProposeDelegationRequest,
        policy_id: &str,
    ) -> StoreResult<RoutedBusinessOperation> {
        let attestation = self
            .collaboration_source_work_attestation(
                &context.company_id,
                &request.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "delegation route requires the server-authored source Work attestation",
                    "source_work_attestation",
                    &request.source_work_attestation_id,
                    None,
                )
            })?;
        if context.expected_revision != 0
            || request.target_placement.placement_generation != 1
            || attestation.source_work_ref.node_id == request.target_placement.node_id
            || (!exact_actor(&context.authenticated_actor, &attestation.source_host_ref)
                && !exact_actor(&context.authenticated_actor, &attestation.source_owner_ref))
            || policy_id.trim().is_empty()
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "delegation route is outside exact source Work actor or v1 target placement",
                "work_delegation_v1",
                &request.delegation_id,
                None,
            ));
        }
        let payload = serde_json::json!({
            "request": request,
            "source_work_attestation": attestation,
            "policy_id": policy_id,
        });
        Ok(RoutedBusinessOperation {
            id: request.operation_id.clone(),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationPropose,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: attestation.source_work_ref.node_id,
            target_placement: request.target_placement.clone(),
            expected_revision: 0,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationPropose.required_capability(),
            ordering_key: format!("delegation:{}", request.delegation_id),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn delegation_decide_operation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        decision: &DelegationDecision,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation decision route requires the central relationship",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || decision.expected_delegation_revision != delegation.revision
            || decision.delegation_id != delegation.id
            || !exact_actor(&context.authenticated_actor, &delegation.target_host_ref)
            || decision.decided_by_target_host != delegation.target_host_ref
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "delegation decision route requires exact target Host and relationship revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "decision": decision,
            "target_placement": delegation.target_placement,
        });
        Ok(RoutedBusinessOperation {
            id: decision.id.clone(),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationDecide,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationDecide.required_capability(),
            ordering_key: format!("delegation:{delegation_id}"),
            created_at: context.occurred_at.clone(),
        })
    }

    /// Target WorkApplicationService publishes only a redacted immutable fact
    /// whose native Work identity is proven by the local target Store. The
    /// Company registry will independently re-check Delegation scope.
    pub fn remote_fact_publish_operation(
        &self,
        context: &CollaborationMutationContext,
        publication: &RemoteFactPublication,
        source_team_placement: &TargetPlacementRef,
        current_node_id: &str,
    ) -> StoreResult<RoutedBusinessOperation> {
        let work = self
            .latest_works()?
            .into_iter()
            .find(|work| work.id == publication.native_fact_work_ref.work_id)
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::PublicationScopeMismatch,
                    "remote publication references no target-owned native Work",
                    "remote_fact_publication",
                    &publication.id,
                    None,
                )
            })?;
        let team = self
            .teams()?
            .into_iter()
            .rev()
            .find(|team| team.id == publication.origin_team_id)
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::PublicationScopeMismatch,
                    "remote publication references no target-owned AgentTeam",
                    "remote_fact_publication",
                    &publication.id,
                    None,
                )
            })?;
        let team_scope = self.agent_team_scope(&team.id)?.ok_or_else(|| {
            collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "remote publication Team has no canonical Execution Space scope",
                "remote_fact_publication",
                &publication.id,
                None,
            )
        })?;
        let host_membership = self.team_host_membership(&team_scope, &team.id, true)?;
        let actor_is_host = context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == host_membership.agent_member_id;
        let actor_is_owner = context.authenticated_actor.kind == ActorKind::AgentMember
            && work.owner_member_id.as_deref() == Some(context.authenticated_actor.id.as_str());
        let accepted_result_revision =
            publication
                .operational_decision_ref
                .as_ref()
                .is_some_and(|decision| {
                    decision.work_ref == publication.native_fact_work_ref
                        && work.version == publication.native_fact_work_ref.work_revision + 1
                        && work.phase == firm_core::WorkPhase::Closed
                        && work.resolution == Some(firm_core::WorkResolution::Accepted)
                });
        let canonical_digest =
            canonical_json_fingerprint(&publication.snapshot.canonical_redacted_fact);
        if context.expected_revision == 0
            || publication.fact_revision == 0
            || publication.company_id != context.company_id
            || publication.origin_node_id != current_node_id
            || publication.native_fact_work_ref.node_id != current_node_id
            || publication.native_fact_work_ref.team_id != team.id
            || publication.native_fact_work_ref.work_id != publication.fact_work_ref.work_id
            || publication.native_fact_work_ref.node_id != publication.fact_work_ref.node_id
            || publication.native_fact_work_ref.team_id != publication.fact_work_ref.team_id
            || publication.native_fact_work_ref.placement_generation
                != publication.fact_work_ref.placement_generation
            || (publication.native_fact_work_ref.work_revision != work.version
                && !accepted_result_revision)
            || publication.fact_digest != canonical_digest
            || publication.snapshot.canonical_digest != canonical_digest
            || publication.snapshot.publication_id != publication.id
            || publication.created_by != context.authenticated_actor
            || (!actor_is_host && !actor_is_owner)
            || source_team_placement.team_id != publication.delegation_source_work_ref.team_id
            || source_team_placement.node_id != publication.delegation_source_work_ref.node_id
            || source_team_placement.placement_generation != 1
            || source_team_placement.node_id == current_node_id
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "remote publication is not server-bound to the exact local Work/actor or source Team placement",
                "remote_fact_publication",
                &publication.id,
                Some(work.version),
            ));
        }
        let payload = serde_json::json!({
            "publication": publication,
            "source_team_placement": source_team_placement,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-publication:{}", publication.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::RemoteFactPublish,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: current_node_id.into(),
            target_placement: source_team_placement.clone(),
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::RemoteFactPublish.required_capability(),
            ordering_key: format!("delegation:{}", publication.delegation_id),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn delegation_cancel_request_operation(
        &self,
        context: &CollaborationMutationContext,
        request: &DelegationCancellationRequest,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, &request.delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "cancellation route requires the central Delegation",
                    "work_delegation_v1",
                    &request.delegation_id,
                    None,
                )
            })?;
        let source_attestation = self
            .collaboration_source_work_attestation(
                &context.company_id,
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "cancellation route has no source Host attestation",
                    "source_work_attestation",
                    &delegation.source_work_attestation_id,
                    None,
                )
            })?;
        if delegation.state != DelegationState::CancellationRequested
            || delegation.revision != request.expected_delegation_revision.saturating_add(1)
            || context.expected_revision != delegation.revision
            || request.requested_by != context.authenticated_actor
            || !exact_actor(
                &context.authenticated_actor,
                &source_attestation.source_host_ref,
            )
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "cancellation request requires the exact source actor and Delegation revision",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "request": request,
            "target_placement": delegation.target_placement,
            "target_work_ref": delegation.target_work_ref,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-cancellation-request:{}", request.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationCancelRequest,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: request.expected_delegation_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationCancelRequest.required_capability(),
            ordering_key: format!("delegation:{}", request.delegation_id),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn delegation_cancel_decide_operation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        request_id: &str,
        decision: &DelegationCancellationDecision,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "cancellation decision route requires the central Delegation",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        let request = self
            .collaboration_cancellation_request(&context.company_id, delegation_id, request_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::CancellationDecisionRequired,
                    "cancellation decision references no pending request",
                    "delegation_cancellation_request",
                    request_id,
                    None,
                )
            })?;
        if delegation.revision != context.expected_revision
            || request.state != CancellationRequestState::Pending
            || decision.cancellation_request_id != request.id
            || decision.expected_request_revision != request.revision
            || decision.decided_by_target_host != delegation.target_host_ref
            || !exact_actor(&context.authenticated_actor, &delegation.target_host_ref)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "cancellation decision requires exact target Host, pending request, and revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "request_id": request_id,
            "decision": decision,
            "target_placement": delegation.target_placement,
            "target_work_ref": delegation.target_work_ref,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-cancellation-decision:{}", decision.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::DelegationCancelDecide,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.source_node_id,
            target_placement: delegation.target_placement,
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationCancelDecide.required_capability(),
            ordering_key: format!("delegation:{delegation_id}"),
            created_at: context.occurred_at.clone(),
        })
    }

    pub fn artifact_grant_operation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        manifest: &RemoteArtifactManifest,
        capability: &ArtifactCapability,
    ) -> StoreResult<RoutedBusinessOperation> {
        let delegation = self
            .collaboration_delegation(&context.company_id, delegation_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "artifact grant requires the central Delegation",
                    "work_delegation_v1",
                    delegation_id,
                    None,
                )
            })?;
        let source_attestation = self
            .collaboration_source_work_attestation(
                &context.company_id,
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "artifact grant has no source Host attestation",
                    "source_work_attestation",
                    &delegation.source_work_attestation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || !matches!(
                delegation.state,
                DelegationState::Active | DelegationState::ResultAvailable
            )
            || !exact_actor(&context.authenticated_actor, &delegation.target_host_ref)
            || manifest.company_id != context.company_id
            || manifest.source_node_id != delegation.target_placement.node_id
            || manifest.source_team_id.as_deref()
                != Some(delegation.target_placement.team_id.as_str())
            || manifest.source_work_id.as_deref()
                != delegation
                    .target_work_ref
                    .as_ref()
                    .map(|work| work.work_id.as_str())
            || manifest.completed_at_unix_ms.is_none()
            || manifest.deleted_at_unix_ms.is_some()
            || !manifest
                .authorized_readers
                .contains(&source_attestation.source_host_ref.id)
            || capability.purpose != ArtifactCapabilityPurpose::Download
            || capability.company_id != context.company_id
            || capability.artifact_id != manifest.id
            || capability.artifact_digest != manifest.sha256
            || capability.node_id != delegation.source_node_id
            || capability.issued_to != source_attestation.source_host_ref.id
        {
            return Err(collaboration_error(
                FabricErrorCode::ArtifactScopeUnauthorized,
                "artifact grant is not bound to the exact Delegation, complete manifest, source Host, and source Node",
                "remote_artifact_manifest",
                &manifest.id,
                Some(manifest.revision),
            ));
        }
        let source_placement = TargetPlacementRef {
            team_id: delegation.source_team_id.clone(),
            team_revision: delegation.source_work_ref.team_revision,
            node_id: delegation.source_node_id.clone(),
            placement_generation: delegation.source_work_ref.placement_generation,
        };
        let payload = serde_json::json!({
            "delegation_id": delegation.id,
            "delegation": delegation,
            "source_work_attestation": source_attestation,
            "manifest": manifest,
            "read_capability": capability,
            "source_placement": source_placement,
        });
        Ok(RoutedBusinessOperation {
            id: format!("route-artifact-grant:{}:{}", delegation_id, manifest.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: context.company_id.clone(),
            kind: RoutedBusinessKind::ArtifactGrant,
            authenticated_actor: context.authenticated_actor.clone(),
            source_node_id: delegation.target_placement.node_id,
            target_placement: source_placement,
            expected_revision: context.expected_revision,
            idempotency_key: context.idempotency_key.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::ArtifactGrant.required_capability(),
            ordering_key: format!("delegation:{delegation_id}"),
            created_at: context.occurred_at.clone(),
        })
    }
}
