use super::*;

impl HarnessStore {
    pub fn apply_target_work_created(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        target_work_ref: &RemoteWorkRef,
        observed_target_placement: &TargetPlacementRef,
        routed_operation_id: &str,
        resolved_control_plane_actor: &ActorRef,
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
            || delegation.state != DelegationState::ProvisioningTargetWork
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "target Work result does not match current provisioning revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if resolved_control_plane_actor.kind != ActorKind::Service
            || !exact_actor(&context.authenticated_actor, resolved_control_plane_actor)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the server-resolved Control Plane Service may fold an applied routed result",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if observed_target_placement != &delegation.target_placement
            || target_work_ref.node_id != delegation.target_placement.node_id
            || target_work_ref.team_id != delegation.target_placement.team_id
            || target_work_ref.team_revision != delegation.target_placement.team_revision
            || target_work_ref.placement_generation
                != delegation.target_placement.placement_generation
            || target_work_ref.work_id == delegation.source_work_ref.work_id
        {
            return Err(collaboration_error(
                FabricErrorCode::TargetTeamPlacementChanged,
                "target Work result is outside the frozen placement",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.target_work_ref = Some(target_work_ref.clone());
        delegation.state = DelegationState::Active;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "target_work_ref": target_work_ref,
                "observed_target_placement": observed_target_placement,
                "routed_operation_id": routed_operation_id,
            }),
            &delegation,
            Vec::new(),
        )
    }

    pub fn request_delegation_cancellation(
        &self,
        context: &CollaborationMutationContext,
        request: &DelegationCancellationRequest,
        authority: &ResolvedCollaborationAuthority,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut frozen_request = request.clone();
        frozen_request.state = CancellationRequestState::Pending;
        frozen_request.revision = 1;
        frozen_request.updated_at = context.occurred_at.clone();
        let replay_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(&frozen_request)?);
        if let Some(existing) =
            self.collaboration_operations_unlocked()?
                .into_iter()
                .find(|operation| {
                    operation.company_id == context.company_id
                        && operation.authenticated_actor == context.authenticated_actor
                        && operation.command_name == context.command_name
                        && operation.idempotency_key == context.idempotency_key
                })
        {
            if existing.request_fingerprint != replay_fingerprint
                || existing.aggregate_kind != "work_delegation_v1"
                || existing.aggregate_id != request.delegation_id
            {
                return Err(collaboration_error(
                    FabricErrorCode::IdempotencyConflict,
                    "cancellation request idempotency key changed its fingerprint",
                    "work_delegation_v1",
                    &request.delegation_id,
                    Some(existing.resulting_revision),
                ));
            }
            return Ok(CollaborationMutationResult {
                projection: serde_json::from_value(existing.resulting_projection.clone())?,
                operation: existing,
                replayed: true,
            });
        }
        let mut delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &request.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    &request.delegation_id,
                    None,
                )
            })?;
        if context.expected_revision != delegation.revision
            || request.expected_delegation_revision != delegation.revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancellation request revision is stale",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.source_host)
            || request.requested_by != authority.source_host
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host may request active cancellation",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        if !matches!(
            delegation.state,
            DelegationState::Active | DelegationState::ResultAvailable
        ) {
            return Err(collaboration_error(
                FabricErrorCode::DelegationTerminal,
                "delegation is not active",
                "work_delegation_v1",
                &delegation.id,
                Some(delegation.revision),
            ));
        }
        delegation.state = DelegationState::CancellationRequested;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            &delegation.id,
            serde_json::to_value(&frozen_request)?,
            &delegation,
            vec![serde_json::to_value(&frozen_request)?],
        )
    }

    pub fn decide_delegation_cancellation(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        request_id: &str,
        decision: &DelegationCancellationDecision,
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
            || delegation.state != DelegationState::CancellationRequested
            || decision.cancellation_request_id != request_id
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancellation decision does not match the pending request",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let mut pending_request = self
            .latest_cancellation_request_unlocked(&context.company_id, delegation_id, request_id)?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::CancellationDecisionRequired,
                    "cancellation decision references no canonical pending request",
                    "delegation_cancellation_request",
                    request_id,
                    None,
                )
            })?;
        if pending_request.state != CancellationRequestState::Pending
            || decision.expected_request_revision != pending_request.revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "cancellation decision does not bind the exact pending request revision",
                "delegation_cancellation_request",
                request_id,
                Some(pending_request.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.target_host)
            || decision.decided_by_target_host != authority.target_host
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host may decide cancellation",
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
                "target placement changed before cancellation decision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        require_non_empty(&decision.native_work_event_ref, "native_work_event_ref")?;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        match decision.decision {
            CancellationDecisionKind::Accept => {
                delegation.state = DelegationState::Terminal;
                delegation.terminal_outcome = Some(DelegationTerminalOutcome::Cancelled);
                pending_request.state = CancellationRequestState::Accepted;
            }
            CancellationDecisionKind::Reject => {
                delegation.state = DelegationState::Active;
                pending_request.state = CancellationRequestState::Rejected;
            }
        }
        pending_request.target_host_decision_ref = Some(decision.id.clone());
        pending_request.revision += 1;
        pending_request.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "request_id": request_id,
                "decision": decision,
                "observed_target_placement": observed_target_placement,
            }),
            &delegation,
            vec![
                serde_json::to_value(decision)?,
                serde_json::to_value(&pending_request)?,
            ],
        )
    }

    pub fn publish_remote_fact(
        &self,
        context: &CollaborationMutationContext,
        publication: &RemoteFactPublication,
        authorized_target_actors: &[ActorRef],
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<RemoteFactPublication>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &publication.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "delegation does not exist",
                    "work_delegation_v1",
                    &publication.delegation_id,
                    None,
                )
            })?;
        if !authorized_target_actors
            .iter()
            .any(|actor| exact_actor(&context.authenticated_actor, actor))
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "remote publication requires an exact target Work actor",
                "remote_fact_publication",
                &publication.id,
                None,
            ));
        }
        if !matches!(
            delegation.state,
            DelegationState::Active
                | DelegationState::ResultAvailable
                | DelegationState::CancellationRequested
        ) || publication.company_id != context.company_id
            || publication.origin_node_id != delegation.target_placement.node_id
            || publication.origin_team_id != delegation.target_placement.team_id
            || delegation.target_work_ref.as_ref() != Some(&publication.fact_work_ref)
            || publication.native_fact_work_ref.work_id != publication.fact_work_ref.work_id
            || publication.native_fact_work_ref.team_id != publication.fact_work_ref.team_id
            || publication.native_fact_work_ref.node_id != publication.fact_work_ref.node_id
            || publication.native_fact_work_ref.placement_generation
                != publication.fact_work_ref.placement_generation
            || publication.fact_work_ref.team_id != delegation.target_placement.team_id
            || publication.fact_work_ref.node_id != delegation.target_placement.node_id
            || publication.fact_work_ref.placement_generation
                != delegation.target_placement.placement_generation
            || publication.delegation_source_work_ref != delegation.source_work_ref
            || observed_target_placement != &delegation.target_placement
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "remote fact is outside the exact Delegation/Work/placement scope",
                "remote_fact_publication",
                &publication.id,
                None,
            ));
        }
        let digest = canonical_json_fingerprint(&publication.snapshot.canonical_redacted_fact);
        if publication.snapshot.publication_id != publication.id
            || publication.snapshot.canonical_digest != digest
            || publication.fact_digest != digest
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationDigestMismatch,
                "remote fact canonical digest does not match the redacted snapshot",
                "remote_fact_publication",
                &publication.id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "remote_fact_publication",
            &publication.id,
            serde_json::to_value(publication)?,
            publication,
            Vec::new(),
        )
    }

    /// Persist a target-Node-delivered, read-only copy of the central
    /// publication. This aggregate is intentionally a cache and is never
    /// consulted by Company collaboration mutations.
    pub fn persist_remote_fact_cache(
        &self,
        context: &CollaborationMutationContext,
        publication: &RemoteFactPublication,
        routed_operation_id: &str,
        target_placement: &TargetPlacementRef,
        current_node_id: &str,
    ) -> StoreResult<CollaborationMutationResult<RemoteFactPublication>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let canonical_digest =
            canonical_json_fingerprint(&publication.snapshot.canonical_redacted_fact);
        if context.expected_revision != 0
            || context.authenticated_actor.kind != ActorKind::Service
            || routed_operation_id.trim().is_empty()
            || publication.company_id != context.company_id
            || publication.fact_digest != canonical_digest
            || publication.snapshot.canonical_digest != canonical_digest
            || target_placement.node_id != current_node_id
            || target_placement.team_id != publication.delegation_source_work_ref.team_id
            || target_placement.node_id != publication.delegation_source_work_ref.node_id
            || target_placement.placement_generation != 1
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "remote fact cache is not bound to the exact central publication and target placement",
                "remote_fact_cache",
                &publication.id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "remote_fact_cache",
            &publication.id,
            serde_json::json!({
                "publication": publication,
                "routed_operation_id": routed_operation_id,
                "target_placement": target_placement,
            }),
            publication,
            Vec::new(),
        )
    }

    pub fn mark_delegation_result_available(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        publication_id: &str,
        operational_decision: &firm_core::collaboration::WorkOperationalDecisionRef,
        authority: &ResolvedCollaborationAuthority,
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "publication_id": publication_id,
            "operational_decision": operational_decision,
            "observed_target_placement": observed_target_placement,
        });
        if let Some(existing) =
            self.collaboration_operations_unlocked()?
                .into_iter()
                .find(|operation| {
                    operation.company_id == context.company_id
                        && operation.authenticated_actor == context.authenticated_actor
                        && operation.command_name == context.command_name
                        && operation.idempotency_key == context.idempotency_key
                })
        {
            let projection =
                serde_json::from_value::<WorkDelegationV1>(existing.resulting_projection)?;
            return self.commit_collaboration_projection_unlocked(
                context,
                "work_delegation_v1",
                delegation_id,
                request_payload,
                &projection,
                vec![serde_json::to_value(operational_decision)?],
            );
        }
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
            || delegation.state != DelegationState::Active
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "result publication does not match the current active Delegation revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.target_host)
            || observed_target_placement != &delegation.target_placement
            || observed_target_placement != &authority.target_placement
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host on the frozen placement may publish an accepted result",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        let publication = self
            .latest_collaboration_projection_unlocked::<RemoteFactPublication>(
                &context.company_id,
                "remote_fact_publication",
                publication_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::PublicationScopeMismatch,
                    "accepted result references a missing immutable publication",
                    "remote_fact_publication",
                    publication_id,
                    None,
                )
            })?;
        let target_work = delegation.target_work_ref.as_ref().ok_or_else(|| {
            collaboration_error(
                FabricErrorCode::TargetWorkCreateFailed,
                "active Delegation has no exact target Work ref",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            )
        })?;
        if publication.delegation_id != delegation.id
            || publication.fact_work_ref.work_id != target_work.work_id
            || operational_decision.work_ref.work_id != target_work.work_id
            || operational_decision.work_ref.work_revision
                != publication.native_fact_work_ref.work_revision
        {
            return Err(collaboration_error(
                FabricErrorCode::PublicationScopeMismatch,
                "publication and WorkOperationalDecision do not bind the same target Submitted Work revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.state = DelegationState::ResultAvailable;
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            request_payload,
            &delegation,
            vec![serde_json::to_value(operational_decision)?],
        )
    }

    pub fn complete_delegation_after_source_integration(
        &self,
        context: &CollaborationMutationContext,
        delegation_id: &str,
        integrated_source_work_ref: &RemoteWorkRef,
        source_integration_event_ref: &str,
        authority: &ResolvedCollaborationAuthority,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        require_non_empty(source_integration_event_ref, "source_integration_event_ref")?;
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
            || delegation.state != DelegationState::ResultAvailable
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "source integration requires the exact result-available revision",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if !exact_actor(&context.authenticated_actor, &authority.source_host) {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the exact source Host may close the collaboration relationship after integration",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        if integrated_source_work_ref.execution_space_id
            != delegation.source_work_ref.execution_space_id
            || integrated_source_work_ref.node_id != delegation.source_work_ref.node_id
            || integrated_source_work_ref.team_id != delegation.source_work_ref.team_id
            || integrated_source_work_ref.work_id != delegation.source_work_ref.work_id
            || integrated_source_work_ref.work_revision < delegation.source_work_ref.work_revision
        {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "source integration evidence does not bind the original source Work lineage",
                "work_delegation_v1",
                delegation_id,
                Some(delegation.revision),
            ));
        }
        delegation.state = DelegationState::Terminal;
        delegation.terminal_outcome = Some(DelegationTerminalOutcome::Completed);
        delegation.revision += 1;
        delegation.updated_at = context.occurred_at.clone();
        self.commit_collaboration_projection_unlocked(
            context,
            "work_delegation_v1",
            delegation_id,
            serde_json::json!({
                "integrated_source_work_ref": integrated_source_work_ref,
                "source_integration_event_ref": source_integration_event_ref,
            }),
            &delegation,
            Vec::new(),
        )
    }
}

/// Build Company-visible, read-only projections from target-owned canonical
/// deliveries. Exactly one row per recipient is required; partial success is
/// represented by independent states and never collapsed to Message-level
/// delivered truth.
pub fn project_cross_node_deliveries(
    message: &Message,
    remote_replica: &RemoteMessageReplica,
    deliveries: &[CanonicalMessageDelivery],
    routed_operation_id: &str,
    target_gateway_generation: Option<u64>,
    target_observed_sequence: u64,
    observed_at: &str,
) -> StoreResult<Vec<CrossNodeDeliveryProjection>> {
    let persisted_message =
        serde_json::from_slice::<Message>(&remote_replica.canonical_message_bytes)
            .map_err(StoreError::from)?;
    if &persisted_message != message
        || remote_replica.source_execution_space_id != message.source_execution_space_id
        || remote_replica.message_id != message.id
        || remote_replica.schema_version != message.schema_version
        || remote_replica.content_fingerprint != message.content_fingerprint
        || remote_replica.body_digest != message.body_digest
    {
        return Err(collaboration_error(
            FabricErrorCode::MessageReplicaMismatch,
            "canonical deliveries require the exact target-persisted immutable Message replica",
            "message",
            &message.id,
            None,
        ));
    }
    let expected_subjects = message
        .recipients
        .iter()
        .filter_map(|recipient| {
            let kind = match recipient.kind {
                firm_core::agentfirm_api::MessageRecipientKind::AgentMember => {
                    firm_core::agentfirm_api::MessageSubjectKind::AgentMember
                }
                firm_core::agentfirm_api::MessageRecipientKind::Team => {
                    firm_core::agentfirm_api::MessageSubjectKind::Team
                }
                firm_core::agentfirm_api::MessageRecipientKind::ControlPlaneActor => {
                    return None;
                }
            };
            Some((kind, recipient.id.clone()))
        })
        .collect::<BTreeSet<_>>();
    let actual = deliveries
        .iter()
        .map(|delivery| (delivery.recipient_kind, delivery.recipient_ref.clone()))
        .collect::<BTreeSet<_>>();
    let target_nodes = deliveries
        .iter()
        .map(|delivery| delivery.target_node_id.as_str())
        .collect::<BTreeSet<_>>();
    if expected_subjects != actual || actual.len() != deliveries.len() || target_nodes.len() != 1 {
        return Err(collaboration_error(
            FabricErrorCode::MessageRecipientUnauthorized,
            "per-recipient delivery batch is missing, duplicated, cross-node mixed, or outside the immutable Message/subscription expansion",
            "message",
            &message.id,
            None,
        ));
    }
    deliveries
        .iter()
        .map(|delivery| {
            if delivery.message_id != message.id {
                return Err(collaboration_error(
                    FabricErrorCode::MessageRecipientUnauthorized,
                    "delivery references a different immutable Message",
                    "canonical_message_delivery",
                    &delivery.id,
                    Some(delivery.version),
                ));
            }
            if (delivery.recipient_kind
                == firm_core::agentfirm_api::MessageSubjectKind::AgentMember
                && delivery.recipient_agent_member_id.as_deref()
                    != Some(delivery.recipient_ref.as_str()))
                || (delivery.recipient_kind
                    == firm_core::agentfirm_api::MessageSubjectKind::Team
                    && delivery.resolved_team_membership_id.is_none())
            {
                return Err(collaboration_error(
                    FabricErrorCode::MessageRecipientUnauthorized,
                    "delivery subject and resolved AgentMember/membership authority disagree",
                    "canonical_message_delivery",
                    &delivery.id,
                    Some(delivery.version),
                ));
            }
            let recipient_agent_member_id = delivery
                .recipient_agent_member_id
                .clone()
                .ok_or_else(|| {
                    collaboration_error(
                        FabricErrorCode::MessageRecipientUnauthorized,
                        "Team-subject delivery must be membership-generation resolved before AgentMember projection",
                        "canonical_message_delivery",
                        &delivery.id,
                        Some(delivery.version),
                    )
                })?;
            Ok(CrossNodeDeliveryProjection {
                delivery_id: delivery.id.clone(),
                message_id: delivery.message_id.clone(),
                recipient_actor_ref: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: recipient_agent_member_id,
                },
                recipient_session_id: delivery.recipient_session_id.clone(),
                recipient_runtime_generation: delivery.recipient_session_generation,
                target_node_id: delivery.target_node_id.clone(),
                target_gateway_generation,
                routed_operation_id: routed_operation_id.into(),
                state: delivery.status,
                attempt_refs: if delivery.attempt == 0 {
                    Vec::new()
                } else {
                    vec![format!(
                        "delivery-attempt:{}:{}",
                        delivery.id, delivery.attempt
                    )]
                },
                receipt_refs: delivery.provider_receipt_id.clone().into_iter().collect(),
                target_observed_sequence,
                observed_at: observed_at.into(),
            })
        })
        .collect()
}
