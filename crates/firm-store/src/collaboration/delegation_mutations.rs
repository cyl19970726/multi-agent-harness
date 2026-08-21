use super::*;

impl HarnessStore {
    pub fn propose_collaboration_delegation(
        &self,
        context: &CollaborationMutationContext,
        request: &ProposeDelegationRequest,
        authority: &ResolvedCollaborationAuthority,
        policy: &DelegationInboundPolicy,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        for (value, field) in [
            (&context.company_id, "company_id"),
            (&context.idempotency_key, "idempotency_key"),
            (&request.delegation_id, "delegation_id"),
            (
                &request.source_work_attestation_id,
                "source_work_attestation_id",
            ),
            (&request.requested_outcome, "requested_outcome"),
            (&request.acceptance_contract, "acceptance_contract"),
        ] {
            require_non_empty(value, field)?;
        }
        if context.expected_revision != 0 {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "delegation propose must start at revision zero",
                "work_delegation_v1",
                &request.delegation_id,
                Some(context.expected_revision),
            ));
        }
        let attestation = self
            .latest_collaboration_projection_unlocked::<SourceWorkAttestation>(
                &context.company_id,
                "source_work_attestation",
                &request.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "delegation proposal requires a canonical source Work attestation",
                    "source_work_attestation",
                    &request.source_work_attestation_id,
                    None,
                )
            })?;
        if attestation.attestation_digest != source_work_attestation_digest(&attestation)?
            || attestation.work_application_service_ref != authority.source_work_application_service
            || attestation.source_gateway_generation != authority.source_gateway_generation
            || attestation.source_host_ref != authority.source_host
            || attestation.source_owner_ref != authority.source_work_owner
        {
            return Err(collaboration_error(
                FabricErrorCode::SourceWorkAttestationInvalid,
                "canonical source Work attestation is stale or outside the server-resolved source authority",
                "source_work_attestation",
                &attestation.id,
                None,
            ));
        }
        if !exact_actor(&context.authenticated_actor, &attestation.source_host_ref)
            && !exact_actor(&context.authenticated_actor, &attestation.source_owner_ref)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host or source Work owner may propose",
                "work_delegation_v1",
                &request.delegation_id,
                None,
            ));
        }
        if attestation.source_work_ref.team_id == request.target_placement.team_id
            || request.target_placement != authority.target_placement
            || request.target_placement.placement_generation != 1
            || attestation.source_work_ref.placement_generation != 1
        {
            return Err(collaboration_error(
                FabricErrorCode::TargetTeamPlacementChanged,
                "source/target authority or exact target placement does not match",
                "work_delegation_v1",
                &request.delegation_id,
                None,
            ));
        }
        if policy.company_id != context.company_id
            || policy.source_team_id != attestation.source_work_ref.team_id
            || policy.target_team_id != request.target_placement.team_id
            || policy.created_by_target_host != authority.target_host
            || policy.revoked_at.is_some()
            || !policy
                .allowed_outcome_classes
                .iter()
                .any(|class| class == &request.outcome_class)
        {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "target-owned inbound policy does not authorize this delegation",
                "delegation_inbound_policy",
                &policy.id,
                Some(policy.revision),
            ));
        }
        let canonical_policy = self
            .latest_collaboration_projection_unlocked::<DelegationInboundPolicy>(
                &context.company_id,
                "delegation_inbound_policy",
                &policy.id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::DelegationPolicyRejected,
                    "target inbound policy is not present in the canonical collaboration Store",
                    "delegation_inbound_policy",
                    &policy.id,
                    None,
                )
            })?;
        if canonical_json_fingerprint(&serde_json::to_value(&canonical_policy)?)
            != canonical_json_fingerprint(&serde_json::to_value(policy)?)
        {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "caller policy does not match the exact canonical target-owned revision",
                "delegation_inbound_policy",
                &policy.id,
                Some(canonical_policy.revision),
            ));
        }
        let active_count = self
            .latest_collaboration_delegations_unlocked(&context.company_id)?
            .values()
            .filter(|delegation| {
                delegation.source_team_id == attestation.source_work_ref.team_id
                    && delegation.target_placement.team_id == request.target_placement.team_id
                    && delegation.state != DelegationState::Terminal
            })
            .count() as u64;
        if active_count >= policy.max_active_delegations {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "target inbound policy active delegation limit is reached",
                "delegation_inbound_policy",
                &policy.id,
                Some(policy.revision),
            ));
        }
        let snapshot = policy_snapshot(policy)?;
        let state = match policy.mode {
            DelegationInboundMode::HostApprovalRequired => DelegationState::AwaitingTargetDecision,
            DelegationInboundMode::AutoAccept => DelegationState::ProvisioningTargetWork,
        };
        let delegation = WorkDelegationV1 {
            id: request.delegation_id.clone(),
            company_id: context.company_id.clone(),
            source_work_attestation_id: attestation.id.clone(),
            source_work_ref: attestation.source_work_ref.clone(),
            source_owner_ref: attestation.source_owner_ref.clone(),
            source_team_id: attestation.source_work_ref.team_id.clone(),
            source_node_id: attestation.source_work_ref.node_id.clone(),
            target_placement: request.target_placement.clone(),
            target_host_ref: authority.target_host.clone(),
            requested_outcome: request.requested_outcome.clone(),
            outcome_class: request.outcome_class.clone(),
            acceptance_contract: request.acceptance_contract.clone(),
            inbound_policy_snapshot: snapshot,
            target_work_ref: None,
            state,
            terminal_outcome: None,
            revision: 1,
            operation_id: request.operation_id.clone(),
            idempotency_key: context.idempotency_key.clone(),
            created_by: context.authenticated_actor.clone(),
            created_at: context.occurred_at.clone(),
            updated_at: context.occurred_at.clone(),
        };
        let payload = serde_json::json!({
            "request": request,
            "resolved_source_host": authority.source_host,
            "resolved_source_work_owner": authority.source_work_owner,
            "resolved_target_host": authority.target_host,
            "policy_snapshot": delegation.inbound_policy_snapshot,
        });
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            &request.delegation_id,
            payload,
            &delegation,
            Vec::new(),
        )
    }

    pub fn decide_collaboration_delegation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        decision: &DelegationDecision,
        authority: &ResolvedCollaborationAuthority,
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
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
        if context.expected_revision != delegation.revision
            || decision.expected_delegation_revision != delegation.revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "delegation decision revision is stale",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if delegation.state != DelegationState::AwaitingTargetDecision {
            return Err(collaboration_error(
                FabricErrorCode::DelegationTerminal,
                "delegation is not awaiting a target decision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.target_host)
            || decision.decided_by_target_host != authority.target_host
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host may decide an inbound delegation",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if observed_target_placement != &delegation.target_placement
            || observed_target_placement != &authority.target_placement
        {
            return Err(collaboration_error(
                FabricErrorCode::TargetTeamPlacementChanged,
                "target Team placement generation changed before decision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        match decision.decision {
            DelegationDecisionKind::Accept => {
                delegation.state = DelegationState::ProvisioningTargetWork;
            }
            DelegationDecisionKind::Reject => {
                delegation.state = DelegationState::Terminal;
                delegation.terminal_outcome = Some(DelegationTerminalOutcome::Rejected);
            }
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "decision": decision,
                "observed_target_placement": observed_target_placement,
            }),
            &delegation,
            vec![serde_json::to_value(decision)?],
        )
    }

    pub fn cancel_delegation_before_accept(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        reason: &str,
        authority: &ResolvedCollaborationAuthority,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        require_non_empty(reason, "reason")?;
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
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
        if context.expected_revision != delegation.revision
            || delegation.state != DelegationState::AwaitingTargetDecision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancel-before-accept requires the exact awaiting decision revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.source_host) {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host may cancel before target acceptance",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.state = DelegationState::Terminal;
        delegation.terminal_outcome = Some(DelegationTerminalOutcome::Cancelled);
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({"reason": reason}),
            &delegation,
            Vec::new(),
        )
    }
}
