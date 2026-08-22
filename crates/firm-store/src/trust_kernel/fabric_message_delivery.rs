use super::*;

impl HarnessStore {
    /// Resolve one Team-subject delivery to one exact active membership
    /// generation. Admission intentionally has no AgentSession dependency;
    /// provider dispatch may bind the resolved member's current session later.
    pub fn claim_team_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: &TeamMessageDeliveryClaim,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(delivery_id, "delivery_id")?;
        required(&claim.claim_id, "TeamMessageDeliveryClaim.claim_id")?;
        required(
            &claim.team_membership_id,
            "TeamMessageDeliveryClaim.team_membership_id",
        )?;
        if context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "Team delivery claim is a new idempotent routing operation",
                "team_message_delivery_claim",
                &claim.claim_id,
                Some(0),
            ));
        }
        let request_payload = serde_json::json!({
            "delivery_id": delivery_id,
            "claim": claim,
            "updated_at": updated_at,
        });
        let request_fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "team_message_delivery_claim",
            &claim.claim_id,
            &request_fingerprint,
        )? {
            return Ok(replay);
        }
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Team MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        let team_id = delivery.target_team_id.clone().ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team-subject delivery is missing target_team_id",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            )
        })?;
        if delivery.recipient_kind != MessageSubjectKind::Team
            || delivery.recipient_ref != team_id
            || delivery.status != CanonicalMessageDeliveryStatus::Queued
            || delivery.resolved_team_membership_id.is_some()
            || delivery.recipient_agent_member_id.is_some()
            || delivery.recipient_session_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only one unresolved queued Team-subject delivery may be membership-claimed",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let current_subscriptions = self
            .fabric_message_subscriptions(&context.execution_space_id)?
            .into_iter()
            .filter(|subscription| subscription.id == delivery.subscription_id)
            .collect::<Vec<_>>();
        if current_subscriptions.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team delivery subscription is missing or ambiguous",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let subscription = &current_subscriptions[0];
        if subscription.subscriber_kind != MessageSubjectKind::Team
            || subscription.subscriber_ref != team_id
            || subscription.target_team_id.as_deref() != Some(team_id.as_str())
            || subscription.target_node_id != delivery.target_node_id
            || subscription.source_kind != MessageSubscriptionKind::AllAuthorized
            || subscription.source_ref != "authorized_peer_teams"
            || subscription.authorization_policy_ref != "collaboration.peer_message_deliver"
            || subscription.status != MessageSubscriptionStatus::Active
            || subscription.revision != delivery.subscription_revision
            || subscription.policy_digest != delivery.subscription_policy_digest
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team delivery claim requires the exact active durable subscription generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &delivery.target_node_id,
            &context.authenticated_actor.id,
            claim.node_daemon_generation,
            &context.authenticated_actor,
            "message_delivery",
            delivery_id,
        )?;
        let team = self
            .agent_teams(&context.execution_space_id)?
            .into_iter()
            .find(|team| team.id == team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "Team-subject delivery references a missing AgentTeam",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        if team.status != AgentTeamStatus::Active || team.node_id != delivery.target_node_id {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Team-subject delivery requires the exact Active Team placement",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let active_members = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .filter(|member| member.organization_status == AgentMemberOrganizationStatus::Active)
            .map(|member| (member.id.clone(), member))
            .collect::<BTreeMap<_, _>>();
        let eligible_memberships = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team.id
                    && membership.node_id == team.node_id
                    && membership.state == TeamMembershipStatus::Active
                    && membership.role != TeamMembershipRole::Observer
                    && active_members.contains_key(&membership.agent_member_id)
            })
            .collect::<Vec<_>>();
        if eligible_memberships.len() != 1
            || eligible_memberships[0].id != claim.team_membership_id
            || eligible_memberships[0].membership_generation != claim.membership_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team delivery remains queued unless exactly one eligible active Host/Member membership generation exists and matches the claim",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let membership = eligible_memberships.into_iter().next().ok_or_else(|| {
            trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team delivery has no eligible TeamMembership",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            )
        })?;
        let member = active_members
            .get(&membership.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "claimed TeamMembership references no active AgentMember",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        delivery.status = CanonicalMessageDeliveryStatus::Routed;
        delivery.resolved_team_membership_id = Some(membership.id);
        delivery.recipient_agent_member_id = Some(member.id.clone());
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_node_daemon_generation = Some(claim.node_daemon_generation);
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "team_message_delivery_claim",
            &claim.claim_id,
            "team_subject_resolved",
            request_payload,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_message_for_provider(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        dispatch_mode: firm_core::agentfirm_api::RuntimeDispatchMode,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<ProviderInvocation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "message_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.target_node_id != node_id
            || !matches!(
                delivery.status,
                CanonicalMessageDeliveryStatus::Queued | CanonicalMessageDeliveryStatus::Routed
            )
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "only the target NodeDaemon can claim a queued MessageDelivery",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        if delivery.recipient_kind == MessageSubjectKind::Team
            && (delivery.status != CanonicalMessageDeliveryStatus::Routed
                || delivery.claim_id.as_deref() != Some(claim_id)
                || delivery.claimed_node_daemon_generation != Some(daemon_generation)
                || delivery.resolved_team_membership_id.is_none())
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Team-subject delivery must first be resolved by the exact membership-generation claim",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let recipient_agent_member_id =
            delivery
                .recipient_agent_member_id
                .as_deref()
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "MessageDelivery has no resolved AgentMember",
                        "message_delivery",
                        delivery_id,
                        Some(delivery.version),
                    )
                })?;
        let current = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_member_id == recipient_agent_member_id
                    && session.node_id == node_id
                    && session.node_daemon_id == daemon_id
                    && session.node_daemon_generation == daemon_generation
                    && session.lifecycle != AgentSessionStatus::Closed
            })
            .collect::<Vec<_>>();
        if current.len() != 1 {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                if current.is_empty() {
                    "recipient has no current local AgentSession; delivery remains queued"
                } else {
                    "recipient identity has multiple current AgentSessions"
                },
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let session = &current[0];
        let invocation_binding = runtime_binding_for_session(session);
        self.require_live_runtime_binding_unlocked(
            session,
            &invocation_binding,
            false,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        let message = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "message")?
            .remove(&delivery.message_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery references a missing Message",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })
            .and_then(|envelope| event_projection::<Message>(&envelope))?;
        delivery.status = CanonicalMessageDeliveryStatus::Claimed;
        delivery.recipient_session_id = Some(session.id.clone());
        delivery.recipient_session_generation = Some(session.runtime_generation);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_node_daemon_generation = Some(daemon_generation);
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let dispatch = ProviderInvocation {
            id: format!("provider-invocation:{}:{}", delivery.id, delivery.attempt),
            source_plane: "message".into(),
            source_record_id: message.id,
            recipient_agent_member_id: recipient_agent_member_id.to_string(),
            recipient_session_id: session.id.clone(),
            recipient_session_generation: session.runtime_generation,
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
            provider: session.provider_kind.clone(),
            dispatch_mode,
            binding: invocation_binding,
            permission_ceiling: session.effective_permission_ceiling,
            content: message.body,
            content_fingerprint: message.content_fingerprint,
            created_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "provider_invocation",
            &dispatch.id,
            "prepared",
            serde_json::json!({
                "delivery_id": delivery_id,
                "claim_id": claim_id,
                "dispatch_mode": dispatch_mode,
            }),
            &dispatch,
            vec![serde_json::to_value(delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_message_provider_receipt(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "message_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.target_node_id != node_id
            || delivery.status != CanonicalMessageDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_node_daemon_generation != Some(daemon_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt does not match the exact delivery claim and NodeDaemon generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let session_id = delivery.recipient_session_id.as_deref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "claimed MessageDelivery did not freeze a recipient session",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            )
        })?;
        let current = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "frozen recipient session no longer exists",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        if Some(current.runtime_generation) != delivery.recipient_session_generation
            || current.node_daemon_generation != daemon_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "recipient session generation changed before provider receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = CanonicalMessageDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery_receipt",
            delivery_id,
            "provider_received",
            serde_json::json!({
                "delivery_id": delivery_id,
                "claim_id": claim_id,
                "provider_receipt_id": provider_receipt_id,
            }),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn acknowledge_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if context.authenticated_actor.kind != ActorKind::AgentMember
            || delivery.recipient_agent_member_id.as_deref()
                != Some(context.authenticated_actor.id.as_str())
            || delivery.status != CanonicalMessageDeliveryStatus::ProviderReceived
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "acknowledge requires the exact recipient identity after provider receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = CanonicalMessageDeliveryStatus::Acknowledged;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let current_cursor = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "subscription_cursor")?
            .remove(&delivery.subscription_id)
            .map(|envelope| event_projection::<SubscriptionCursor>(&envelope))
            .transpose()?;
        let cursor = SubscriptionCursor {
            subscription_id: delivery.subscription_id.clone(),
            recipient_agent_member_id: delivery
                .recipient_agent_member_id
                .clone()
                .expect("recipient checked above"),
            last_visible_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_visible_store_sequence.saturating_add(1))
                .unwrap_or(1),
            last_delivered_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_delivered_store_sequence.saturating_add(1))
                .unwrap_or(1),
            last_read_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_read_store_sequence.saturating_add(1))
                .unwrap_or(1),
            cursor_revision: current_cursor
                .as_ref()
                .map(|cursor| cursor.cursor_revision + 1)
                .unwrap_or(1),
            updated_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery_ack",
            delivery_id,
            "acknowledged",
            serde_json::json!({"delivery_id": delivery_id, "updated_at": updated_at}),
            &delivery,
            vec![
                serde_json::to_value(&delivery)?,
                serde_json::to_value(cursor)?,
            ],
            Vec::new(),
        )
    }

    /// Record an explicit pull/read acknowledgement by a user-driven external
    /// recipient. This path deliberately skips provider claim and receipt: an
    /// external interactive runtime has no daemon-owned provider effect to
    /// prove. The application layer must first verify that the exact recipient
    /// MemberRun is currently in `external_interactive` mode.
    pub fn acknowledge_external_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if context.authenticated_actor.kind != ActorKind::AgentMember
            || delivery.recipient_agent_member_id.as_deref()
                != Some(context.authenticated_actor.id.as_str())
            || !matches!(
                delivery.status,
                CanonicalMessageDeliveryStatus::Queued | CanonicalMessageDeliveryStatus::Routed
            )
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "external acknowledge requires the exact queued recipient identity",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = CanonicalMessageDeliveryStatus::Acknowledged;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let current_cursor = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "subscription_cursor")?
            .remove(&delivery.subscription_id)
            .map(|envelope| event_projection::<SubscriptionCursor>(&envelope))
            .transpose()?;
        let cursor = SubscriptionCursor {
            subscription_id: delivery.subscription_id.clone(),
            recipient_agent_member_id: delivery
                .recipient_agent_member_id
                .clone()
                .expect("recipient checked above"),
            last_visible_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_visible_store_sequence.saturating_add(1))
                .unwrap_or(1),
            last_delivered_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_delivered_store_sequence.saturating_add(1))
                .unwrap_or(1),
            last_read_store_sequence: current_cursor
                .as_ref()
                .map(|cursor| cursor.last_read_store_sequence.saturating_add(1))
                .unwrap_or(1),
            cursor_revision: current_cursor
                .as_ref()
                .map(|cursor| cursor.cursor_revision + 1)
                .unwrap_or(1),
            updated_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "external_message_delivery_ack",
            delivery_id,
            "externally_acknowledged",
            serde_json::json!({"delivery_id": delivery_id, "updated_at": updated_at}),
            &delivery,
            vec![
                serde_json::to_value(&delivery)?,
                serde_json::to_value(cursor)?,
            ],
            Vec::new(),
        )
    }

    /// Operator-requested recovery is executed by the exact current target
    /// NodeDaemon. Replay is resolved before mutable delivery state, and an
    /// acknowledged provider receipt can never be converted into a retry.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_canonical_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        outcome: DeliveryReconcileOutcome,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalMessageDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(evidence_ref, "MessageDelivery reconciliation evidence_ref")?;
        let fingerprint = canonical_json_fingerprint(&serde_json::json!({
            "transport_request_fingerprint": context.request_fingerprint,
            "delivery_id": delivery_id,
            "node_id": node_id,
            "daemon_id": daemon_id,
            "daemon_generation": daemon_generation,
            "outcome": outcome,
            "evidence_ref": evidence_ref,
        }));
        let existing = self.trust_operation_envelopes_unlocked()?;
        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint
                || replay.operation.event.aggregate_kind != "canonical_message_delivery"
                || replay.operation.event.aggregate_id != delivery_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "MessageDelivery reconciliation key was reused with different semantics",
                    "canonical_message_delivery",
                    delivery_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "canonical_message_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalMessageDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "canonical MessageDelivery not found",
                    "canonical_message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.target_node_id != node_id || context.expected_version != delivery.version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "MessageDelivery recovery requires its exact target Node and revision",
                "canonical_message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        match outcome {
            DeliveryReconcileOutcome::Acknowledged => {
                if delivery.status != CanonicalMessageDeliveryStatus::ProviderReceived
                    || delivery.provider_receipt_id.is_none()
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "acknowledged recovery requires a durable provider receipt",
                        "canonical_message_delivery",
                        delivery_id,
                        Some(delivery.version),
                    ));
                }
                delivery.status = CanonicalMessageDeliveryStatus::Acknowledged;
            }
            DeliveryReconcileOutcome::RetrySafeFailure => {
                if delivery.status != CanonicalMessageDeliveryStatus::Claimed
                    || delivery.provider_receipt_id.is_some()
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "retry requires a claimed delivery with proven no provider receipt",
                        "canonical_message_delivery",
                        delivery_id,
                        Some(delivery.version),
                    ));
                }
                delivery.status = CanonicalMessageDeliveryStatus::Queued;
                delivery.attempt += 1;
                delivery.claim_id = None;
                delivery.claimed_node_daemon_generation = None;
                delivery.recipient_session_id = None;
                delivery.recipient_session_generation = None;
                delivery.failure_code = Some("RETRY_SAFE_FAILURE".into());
                delivery.failure_detail = Some(evidence_ref.to_string());
            }
        }
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let aggregate_version = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "canonical_message_delivery"
                    && envelope.operation.event.aggregate_id == delivery_id
            })
            .map(|envelope| envelope.operation.event.resulting_version)
            .max()
            .unwrap_or(0);
        let mut commit_context = context.clone();
        commit_context.expected_version = aggregate_version;
        commit_context.request_fingerprint = Some(fingerprint);
        self.commit_trust_projection_unlocked(
            &commit_context,
            "canonical_message_delivery",
            delivery_id,
            "reconciled",
            serde_json::json!({
                "outcome": outcome,
                "evidence_ref": evidence_ref,
                "daemon_generation": daemon_generation,
            }),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }
}
