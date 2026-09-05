//! What happens to one member's in-flight Work when the exact runtime
//! generation that received it is provably gone.
//!
//! One settlement seam reaches this module (#756): a NodeDaemon drain or
//! Operator predecessor recovery, both of which already prove that this daemon
//! generation's owned provider process groups terminated.
//!
//! The killed turn is never replayed: its `RuntimeCommand` stays settled
//! against the dead generation, and no `StartCycle` is re-issued here. What is
//! written is the honest opposite — the binding is invalidated with a recorded
//! cause, and the in-flight `CanonicalWorkDelivery` is superseded with an
//! explicit failure code that says the attempt can never be settled, never that
//! the turn completed. The Work itself keeps its responsibility and revision,
//! so the ordinary dispatch path mints a fresh binding generation and a fresh
//! delivery on the next Supervisor pass.
//!
//! A `Claimed` (pre-receipt) delivery is covered by the same rule even though
//! no receipt row proves the provider ever answered: the claim may have
//! reached the provider before the crash. Superseding it is still safe
//! because the caller has already proved this generation's provider process
//! groups terminated, the killed turn is never replayed, and the next
//! dispatch mints a fresh binding generation — never a replay of the claimed
//! attempt.
//!
//! #734's shape — an Active binding with a `ProviderReceived` delivery after a
//! non-clean MemberRun generation advance — is deliberately NOT handled here.
//! Superseding it automatically would change the shipped Close/Reopen contract,
//! which requires the Host to re-drive that Work explicitly
//! (`kimi_provider_error_after_receipt_requires_recovery_without_replay`), so
//! who re-drives it is an open owner decision rather than a mechanism gap.

use super::*;

/// Why one WorkExecutionBinding's exact runtime generation is provably gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LostRuntimeGenerationCause {
    /// This daemon generation drained and confirmed its owned provider process
    /// groups terminated.
    NodeDaemonDrain,
    /// The Operator recovered a crashed predecessor generation with exact
    /// process/process-group termination evidence.
    NodeDaemonPredecessorRecovery,
    /// The Host proved from durable epochs alone that the binding's exact
    /// MemberRun/AgentSession generation can never pass the runtime fence
    /// again, and recovered the Work (`team-run work recover-lost-execution`).
    HostLostExecutionRecovery,
}

impl LostRuntimeGenerationCause {
    /// Stable snake_case token recorded on the binding's invalidation event.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NodeDaemonDrain => "node_daemon_drain",
            Self::NodeDaemonPredecessorRecovery => "node_daemon_predecessor_recovery",
            Self::HostLostExecutionRecovery => "host_lost_execution_recovery",
        }
    }

    /// The exact `CanonicalWorkDelivery.failure_code` this cause writes.
    pub fn delivery_failure_code(self) -> &'static str {
        match self {
            Self::NodeDaemonDrain => {
                firm_core::agentfirm_api::WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN
            }
            Self::NodeDaemonPredecessorRecovery => {
                firm_core::agentfirm_api::WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_PREDECESSOR_RECOVERY
            }
            Self::HostLostExecutionRecovery => {
                firm_core::agentfirm_api::WORK_DELIVERY_SUPERSEDED_BY_HOST_LOST_EXECUTION_RECOVERY
            }
        }
    }
}

/// One binding invalidated because its runtime generation is gone, plus the
/// delivery status that was superseded. Returned as observation, so a caller
/// reports what actually happened rather than what it expected to happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidatedWorkExecution {
    pub binding_id: String,
    pub work_id: String,
    pub delivery_id: String,
    pub superseded_delivery_status: WorkDeliveryStatus,
    pub cause: LostRuntimeGenerationCause,
}

/// The exact lane (AgentSession id + its runtime generation) whose Work
/// authority a settlement writer is ending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LostRuntimeLane {
    pub agent_session_id: String,
    pub agent_session_generation: u64,
}

impl HarnessStore {
    /// End the Work authority of every lane a NodeDaemon settlement just
    /// terminated. Only a delivery that was `Claimed` or `ProviderReceived` is
    /// superseded: those are exactly the states the process-group kill cut, and
    /// they are in neither the `Queued` state the ordinary dispatch path can
    /// re-claim nor a terminal one. A `Queued` delivery was never handed to a
    /// provider, so the reattached lane can still claim it unchanged.
    ///
    /// The caller must already hold the Store write lock and must already have
    /// proved that this generation's owned provider process groups terminated.
    pub(crate) fn invalidate_lost_generation_work_bindings_unlocked(
        &self,
        context: &MutationContext,
        execution_space_id: &str,
        lanes: &[LostRuntimeLane],
        cause: LostRuntimeGenerationCause,
        evidence: &Value,
        ended_at: &str,
    ) -> StoreResult<Vec<InvalidatedWorkExecution>> {
        if lanes.is_empty() {
            return Ok(Vec::new());
        }
        let mut deliveries = self.canonical_fabric_work_deliveries_unlocked(execution_space_id)?;
        let mut invalidated = Vec::new();
        for binding in self
            .fabric_work_execution_bindings(execution_space_id)?
            .into_iter()
            .filter(|binding| binding.status == WorkExecutionBindingStatus::Active)
            .filter(|binding| {
                lanes.iter().any(|lane| {
                    lane.agent_session_id == binding.agent_session_id
                        && lane.agent_session_generation == binding.agent_session_generation
                })
            })
        {
            let Some(delivery) = deliveries.remove(&binding.delivery_id) else {
                continue;
            };
            if !matches!(
                delivery.status,
                WorkDeliveryStatus::Claimed | WorkDeliveryStatus::ProviderReceived
            ) {
                continue;
            }
            let mut binding_context = context.clone();
            binding_context.execution_space_id = execution_space_id.to_string();
            binding_context.command_name = format!(
                "node_daemon.work_execution_binding.invalidate.{}",
                cause.reason()
            );
            binding_context.idempotency_key = format!(
                "work-execution-invalidation:{}:{}:{}",
                cause.reason(),
                binding.id,
                binding.version
            );
            binding_context.expected_version = binding.version;
            binding_context.request_fingerprint = None;
            let (_, observed) = self.supersede_lost_generation_work_execution_unlocked(
                &binding_context,
                &binding,
                delivery,
                cause,
                evidence,
                ended_at,
            )?;
            invalidated.push(observed);
        }
        Ok(invalidated)
    }

    /// Release one binding whose exact runtime generation the Host proved
    /// gone from durable epochs (`team-run work recover-lost-execution`).
    ///
    /// A `Claimed` or `ProviderReceived` delivery is superseded through the
    /// same writer a NodeDaemon settlement uses, so the claim id and provider
    /// receipt stay on the event as evidence and no provider outcome is ever
    /// asserted. A `Queued` delivery was never handed to a provider and fails
    /// with the ordinary released-before-claim code. The caller must already
    /// hold the Store write lock and must already have proved the loss.
    pub(crate) fn release_lost_execution_binding_unlocked(
        &self,
        context: &MutationContext,
        binding: &WorkExecutionBinding,
        evidence: &Value,
        ended_at: &str,
    ) -> StoreResult<(
        CanonicalMutationResult<WorkExecutionBinding>,
        Option<InvalidatedWorkExecution>,
    )> {
        if !matches!(
            binding.status,
            WorkExecutionBindingStatus::Offered
                | WorkExecutionBindingStatus::Accepted
                | WorkExecutionBindingStatus::Active
        ) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an executable WorkExecutionBinding can be released as a lost execution",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let mut delivery = self
            .canonical_fabric_work_deliveries_unlocked(&context.execution_space_id)?
            .remove(&binding.delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "lost-execution WorkExecutionBinding release requires its exact canonical WorkDelivery",
                    "work_delivery",
                    &binding.delivery_id,
                    None,
                )
            })?;
        let cause = LostRuntimeGenerationCause::HostLostExecutionRecovery;
        if matches!(
            delivery.status,
            WorkDeliveryStatus::Claimed | WorkDeliveryStatus::ProviderReceived
        ) {
            let (result, observed) = self.supersede_lost_generation_work_execution_unlocked(
                context, binding, delivery, cause, evidence, ended_at,
            )?;
            return Ok((result, Some(observed)));
        }
        if delivery.work_execution_binding_id != binding.id
            || delivery.work_id != binding.work_id
            || delivery.work_revision != binding.work_revision
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "lost-execution WorkExecutionBinding release found conflicting canonical delivery evidence",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let status_before = delivery.status;
        let mut side_records = Vec::new();
        if delivery.status == WorkDeliveryStatus::Queued {
            delivery.status = WorkDeliveryStatus::Failed;
            delivery.failure_code =
                Some("WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM".to_string());
            delivery.version += 1;
            delivery.updated_at = ended_at.to_string();
            side_records.push(serde_json::to_value(&delivery)?);
        }
        let request_payload = serde_json::json!({
            "lost_runtime_generation": {
                "cause": cause.reason(),
                "agent_session_id": binding.agent_session_id,
                "agent_session_generation": binding.agent_session_generation,
                "evidence": evidence,
            },
            "superseded_delivery": {
                "delivery_id": delivery.id,
                "status_before_supersession": status_before,
                "claim_id": delivery.claim_id,
                "claimed_node_daemon_generation": delivery.claimed_node_daemon_generation,
                "provider_receipt_id": delivery.provider_receipt_id,
                "failure_code": delivery.failure_code,
            },
            "ended_at": ended_at,
        });
        let mut released = binding.clone();
        released.status = WorkExecutionBindingStatus::Released;
        released.version += 1;
        released.ended_at = Some(ended_at.to_string());
        let result = self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            &binding.id,
            "invalidated_by_lost_runtime_generation",
            request_payload,
            &released,
            side_records,
            Vec::new(),
        )?;
        Ok((result, None))
    }

    /// Release one exact Active binding and supersede its in-flight delivery.
    /// The provider receipt, claim id and pre-supersession status stay on the
    /// event as evidence of what did cross the provider boundary.
    fn supersede_lost_generation_work_execution_unlocked(
        &self,
        context: &MutationContext,
        binding: &WorkExecutionBinding,
        mut delivery: CanonicalWorkDelivery,
        cause: LostRuntimeGenerationCause,
        evidence: &Value,
        ended_at: &str,
    ) -> StoreResult<(
        CanonicalMutationResult<WorkExecutionBinding>,
        InvalidatedWorkExecution,
    )> {
        if delivery.work_execution_binding_id != binding.id
            || delivery.work_id != binding.work_id
            || delivery.work_revision != binding.work_revision
            || delivery.recipient_agent_member_id != binding.agent_member_id
            || delivery.recipient_session_id != binding.agent_session_id
            || delivery.recipient_session_generation != binding.agent_session_generation
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "lost-generation WorkExecutionBinding invalidation found conflicting canonical delivery evidence",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        if !matches!(
            delivery.status,
            WorkDeliveryStatus::Claimed | WorkDeliveryStatus::ProviderReceived
        ) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only a claimed or provider-received WorkDelivery is superseded by a lost runtime generation",
                "work_delivery",
                &delivery.id,
                Some(delivery.version),
            ));
        }
        let superseded_delivery_status = delivery.status;
        let observed = InvalidatedWorkExecution {
            binding_id: binding.id.clone(),
            work_id: binding.work_id.clone(),
            delivery_id: delivery.id.clone(),
            superseded_delivery_status,
            cause,
        };
        let request_payload = serde_json::json!({
            "lost_runtime_generation": {
                "cause": cause.reason(),
                "agent_session_id": binding.agent_session_id,
                "agent_session_generation": binding.agent_session_generation,
                "evidence": evidence,
            },
            "superseded_delivery": {
                "delivery_id": delivery.id,
                "status_before_supersession": superseded_delivery_status,
                "claim_id": delivery.claim_id,
                "claimed_node_daemon_generation": delivery.claimed_node_daemon_generation,
                "provider_receipt_id": delivery.provider_receipt_id,
                "failure_code": cause.delivery_failure_code(),
            },
            "ended_at": ended_at,
        });
        delivery.status = WorkDeliveryStatus::Failed;
        delivery.failure_code = Some(cause.delivery_failure_code().to_string());
        delivery.version += 1;
        delivery.updated_at = ended_at.to_string();
        let mut released = binding.clone();
        released.status = WorkExecutionBindingStatus::Released;
        released.version += 1;
        released.ended_at = Some(ended_at.to_string());
        let result = self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            &binding.id,
            "invalidated_by_lost_runtime_generation",
            request_payload,
            &released,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )?;
        Ok((result, observed))
    }
}
