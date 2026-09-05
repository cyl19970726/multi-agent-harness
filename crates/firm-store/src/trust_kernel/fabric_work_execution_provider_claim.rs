//! Provider claim/receipt path for canonical Work deliveries: the NodeDaemon
//! claim that prepares one exact `ProviderInvocation` and the provider
//! receipt/failure settlement of that claim (pure move out of `fabric_work_execution.rs`, GitHub #847).

use super::fabric_foundation::RuntimeBindingAdmission;
use super::*;

impl HarnessStore {
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
        let mut invocation_binding =
            self.work_execution_runtime_binding(&context.execution_space_id, &binding.id)?;
        // Same rule as `work_execution_binding_is_current_unlocked`: a binding
        // frozen before the first provider Open stays claimable once the native
        // id attaches to the same exact generation (GitHub #745).
        self.require_live_runtime_binding_unlocked(
            &session,
            &invocation_binding,
            RuntimeBindingAdmission::RuntimeCommand {
                allow_native_session_attachment: true,
                settlement_only: false,
            },
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        // The invocation records the session the claim ran against: once the
        // admission above accepted the pre-Open binding, its empty native ref
        // is filled from the session so provider drivers can hold the stored
        // fence to the live session (every other field already matched).
        if invocation_binding.native_session_ref.is_none() {
            invocation_binding.native_session_ref = session.native_session_ref.clone();
        }
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
        self.require_node_daemon_settlement_authority_unlocked(
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
        self.require_node_daemon_settlement_authority_unlocked(
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
}
