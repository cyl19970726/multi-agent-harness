//! Provider-receipt evidence for a Work execution: the one canonical
//! `WorkDelivery` a member's semantic report must be able to name before the
//! kernel admits it (moved out of `fabric_work_execution.rs` as the first
//! slice of GitHub #847; pure move, no behaviour change). Its tests stay in
//! `trust_kernel_tests` (the Work responsibility admission and close-cutover
//! suites); keep them there when slicing further.

use super::*;

impl HarnessStore {
    pub(crate) fn require_provider_received_work_delivery_unlocked(
        &self,
        execution_space_id: &str,
        binding: &WorkExecutionBinding,
    ) -> StoreResult<CanonicalWorkDelivery> {
        let delivery = self
            .canonical_fabric_work_deliveries_unlocked(execution_space_id)?
            .remove(&binding.delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::DeliveryRecoveryUncertain,
                    "semantic Work report requires the exact canonical WorkDelivery",
                    "work_delivery",
                    &binding.delivery_id,
                    None,
                )
            })?;
        let binds_this_execution = delivery.work_execution_binding_id == binding.id
            && delivery.work_id == binding.work_id
            && delivery.work_revision == binding.work_revision
            && delivery.recipient_agent_member_id == binding.agent_member_id
            && delivery.recipient_session_id == binding.agent_session_id
            && delivery.recipient_session_generation == binding.agent_session_generation;
        // A Queued delivery that was never claimed and never produced a
        // provider receipt is a certain, self-resolving state, not a recovery
        // fence: the Supervisor has simply not dispatched it yet.
        if binds_this_execution
            && delivery.status == WorkDeliveryStatus::Queued
            && delivery.claim_id.as_deref().is_none_or(str::is_empty)
            && delivery
                .provider_receipt_id
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Err(retryable_trust_error(
                TrustErrorCode::DeliveryNotDispatched,
                "WorkDelivery is queued and not yet dispatched to the provider; wait for the Supervisor wake and retry",
                "work_delivery",
                &delivery.id,
                Some(delivery.version),
            ));
        }
        if !binds_this_execution
            || delivery.status != WorkDeliveryStatus::ProviderReceived
            || delivery
                .provider_receipt_id
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "semantic Work report requires exact provider-received delivery evidence",
                "work_delivery",
                &delivery.id,
                Some(delivery.version),
            ));
        }
        Ok(delivery)
    }
}
