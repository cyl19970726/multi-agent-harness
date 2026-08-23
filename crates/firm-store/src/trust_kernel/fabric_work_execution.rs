use super::fabric_foundation::RuntimeBindingAdmission;
use super::*;

impl HarnessStore {
    pub fn fabric_work_execution_bindings(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<WorkExecutionBinding>> {
        let mut latest = BTreeMap::<String, WorkExecutionBinding>::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id != execution_space_id {
                continue;
            }
            if envelope.operation.event.aggregate_kind == "work_execution_binding" {
                let binding = event_projection::<WorkExecutionBinding>(&envelope)?;
                latest.insert(binding.id.clone(), binding);
            }
            // StopSession atomically quiesces active Work bindings in the same
            // RuntimeCommand operation. Side records are full resulting
            // projections and participate in latest-version selection.
            for record in envelope.operation.immutable_side_records {
                if let Ok(binding) = serde_json::from_value::<WorkExecutionBinding>(record) {
                    let replace = latest
                        .get(&binding.id)
                        .is_none_or(|current| binding.version > current.version);
                    if replace {
                        latest.insert(binding.id.clone(), binding);
                    }
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn fabric_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CanonicalWorkDelivery>> {
        Ok(self
            .canonical_fabric_work_deliveries_unlocked(execution_space_id)?
            .into_values()
            .collect())
    }

    pub(super) fn canonical_fabric_work_deliveries_unlocked(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<BTreeMap<String, CanonicalWorkDelivery>> {
        self.latest_fabric_side_records_unlocked(
            execution_space_id,
            |row: &CanonicalWorkDelivery| row.id.clone(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_for_provider(
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
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .canonical_fabric_work_deliveries_unlocked(&context.execution_space_id)?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        let binding = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .find(|binding| binding.id == delivery.work_execution_binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery binding is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|session| session.id == delivery.recipient_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "WorkDelivery session is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let work = self
            .latest_works_unlocked()?
            .remove(&delivery.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkDelivery Work is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        // Provider admission is decided from the locked authoritative Work
        // graph, never from a stale RoleView or client-side readiness hint.
        self.require_work_ready_for_execution_unlocked(&work)?;
        if delivery.status != WorkDeliveryStatus::Queued
            || delivery.target_node_id != node_id
            || binding.status != WorkExecutionBindingStatus::Active
            || binding.work_revision != work.version
            || delivery.work_revision != work.version
            || session.agent_member_id != delivery.recipient_agent_member_id
            || session.runtime_generation != delivery.recipient_session_generation
            || session.node_daemon_id != daemon_id
            || session.node_daemon_generation != daemon_generation
            || session.lifecycle == AgentSessionStatus::Closed
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkDelivery no longer matches its exact active binding, Work revision, session, or NodeDaemon generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let invocation_binding = runtime_binding_for_session(&session);
        self.require_live_runtime_binding_unlocked(
            &session,
            &invocation_binding,
            RuntimeBindingAdmission::Invocation,
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        delivery.status = WorkDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_node_daemon_generation = Some(daemon_generation);
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let content = serde_json::to_string(&serde_json::json!({
            "work_id": work.id,
            "work_revision": work.version,
            "title": work.title,
            "context_markdown": work.context_markdown,
            "completion_criteria_markdown": work.completion_criteria_markdown,
        }))?;
        let invocation = ProviderInvocation {
            id: format!("provider-invocation:{}:{}", delivery.id, delivery.attempt),
            source_plane: "work_delivery".into(),
            source_record_id: delivery.id.clone(),
            recipient_agent_member_id: delivery.recipient_agent_member_id.clone(),
            recipient_session_id: delivery.recipient_session_id.clone(),
            recipient_session_generation: delivery.recipient_session_generation,
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
            provider: session.provider_kind.clone(),
            dispatch_mode,
            binding: invocation_binding,
            permission_ceiling: session.effective_permission_ceiling,
            content_fingerprint: canonical_json_fingerprint(
                &serde_json::json!({"content": content}),
            ),
            content,
            created_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "provider_invocation",
            &invocation.id,
            "prepared_from_work_delivery",
            serde_json::json!({"delivery_id": delivery_id, "claim_id": claim_id}),
            &invocation,
            vec![serde_json::to_value(delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_work_provider_receipt(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalWorkDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalWorkDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_node_daemon_generation != Some(daemon_generation)
            || delivery.target_node_id != node_id
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt does not match the exact WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery_receipt",
            delivery_id,
            "provider_received",
            serde_json::json!({"claim_id": claim_id, "provider_receipt_id": provider_receipt_id}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_work_provider_failure(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        failure_code: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalWorkDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .latest_fabric_side_records_unlocked(
                &context.execution_space_id,
                |row: &CanonicalWorkDelivery| row.id.clone(),
            )?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_node_daemon_generation != Some(daemon_generation)
            || delivery.target_node_id != node_id
            || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider failure does not match one exact unreceived WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Failed;
        delivery.failure_code = Some(failure_code.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery_failure",
            delivery_id,
            "provider_negative_ack",
            serde_json::json!({"claim_id": claim_id, "failure_code": failure_code}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn bind_work_execution(
        &self,
        context: &MutationContext,
        binding: WorkExecutionBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if binding.version != 1 || binding.status != WorkExecutionBindingStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new WorkExecutionBinding must be active at version 1",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let membership = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.team_membership_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing TeamMembership",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.agent_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing AgentSession",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        let work = self
            .latest_works_unlocked()?
            .remove(&binding.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkExecutionBinding references a missing Work",
                    "work",
                    &binding.work_id,
                    None,
                )
            })?;
        if membership.state != TeamMembershipStatus::Active
            || membership.agent_member_id != binding.agent_member_id
            || session.agent_member_id != binding.agent_member_id
            || session.node_id != membership.node_id
            || session.runtime_generation != binding.agent_session_generation
            || session.lifecycle == AgentSessionStatus::Closed
            || work.version != binding.work_revision
            || work.accountable_team_id.as_deref() != Some(membership.team_id.as_str())
            || binding.team_id != membership.team_id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding identity, session generation, Team, or Work revision mismatch",
                "work_execution_binding",
                &binding.id,
                None,
            ));
        }
        // Bind only a Work that is ready at this exact Store revision. The
        // provider claim repeats this check immediately before the effect.
        self.require_work_ready_for_execution_unlocked(&work)?;
        if self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .iter()
            .any(|row| {
                row.work_id == binding.work_id && row.status == WorkExecutionBindingStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Work already has an active WorkExecutionBinding; explicit release is required",
                "work",
                &binding.work_id,
                Some(work.version),
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            &binding.id,
            "bound",
            serde_json::to_value(&binding)?,
            &binding,
            vec![serde_json::to_value(CanonicalWorkDelivery {
                id: format!(
                    "work-delivery:{}:{}",
                    binding.work_id, binding.binding_generation
                ),
                work_id: binding.work_id.clone(),
                work_revision: binding.work_revision,
                work_execution_binding_id: binding.id.clone(),
                recipient_agent_member_id: binding.agent_member_id.clone(),
                recipient_session_id: binding.agent_session_id.clone(),
                recipient_session_generation: binding.agent_session_generation,
                target_node_id: session.node_id.clone(),
                status: WorkDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_node_daemon_generation: None,
                provider_receipt_id: None,
                failure_code: None,
                version: 1,
                created_at: binding.bound_at.clone(),
                updated_at: binding.bound_at.clone(),
            })?],
            Vec::new(),
        )
    }

    pub fn release_work_execution_binding(
        &self,
        context: &MutationContext,
        binding_id: &str,
        ended_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut binding = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "work_execution_binding")?
            .remove(binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding not found",
                    "work_execution_binding",
                    binding_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<WorkExecutionBinding>(&envelope))?;
        if binding.status != WorkExecutionBindingStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an active WorkExecutionBinding can be released",
                "work_execution_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let exact_member = context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == binding.agent_member_id;
        let host_or_operator = matches!(
            context.authenticated_actor.kind,
            ActorKind::Human | ActorKind::Service
        );
        if !exact_member && !host_or_operator {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding release requires exact member or control-plane authority",
                "work_execution_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        binding.status = WorkExecutionBindingStatus::Released;
        binding.version += 1;
        binding.ended_at = Some(ended_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            binding_id,
            "released",
            serde_json::json!({"ended_at": ended_at}),
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }
}
