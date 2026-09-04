//! Host redelivery of an open Work whose delivery never started.
//!
//! A WorkDelivery freezes the exact AgentSession generation it was bound to.
//! When the Host closes and reopens a member runtime, that generation is gone:
//! the WorkExecutionBinding is released and the frozen delivery can never be
//! claimed again. The delivery evidence itself stays immutable — a provider
//! receipt is a durable fact — so the ordinary binding path refuses to replay
//! it and waits for explicit new Host authority (see
//! `provider_received_work_requires_host_reauthorization`).
//!
//! `redeliver_work_to_current_session` is that explicit authority. It appends
//! one Host `Rebound` WorkOperation that records which deliveries it supersedes
//! and advances the Work revision. Nothing is deleted or rewritten: the stale
//! rows keep their status, receipt and version. The ordinary NodeDaemon path
//! then binds the new revision to the member's current AgentSession generation
//! and produces the new WorkDelivery, exactly as it does after `work assign`.

use super::*;
use firm_core::agentfirm_api::{
    AgentSessionStatus, WorkExecutionBinding, WorkExecutionBindingStatus,
};

/// One stale delivery superseded by a Host redelivery, recorded verbatim in
/// the `Rebound` WorkEvent payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersededWorkDelivery {
    pub delivery_id: String,
    pub delivery_version: u64,
    pub status: WorkDeliveryStatus,
    pub work_revision: u64,
    pub work_execution_binding_id: String,
    pub work_execution_binding_status: Option<WorkExecutionBindingStatus>,
    pub recipient_agent_member_id: String,
    pub recipient_session_id: String,
    pub recipient_session_generation: u64,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    /// Why this delivery can never reach the provider again.
    pub stale_because: String,
}

/// Only these binding states can still carry a delivery to the provider.
fn binding_can_still_execute(status: WorkExecutionBindingStatus) -> bool {
    matches!(
        status,
        WorkExecutionBindingStatus::Offered
            | WorkExecutionBindingStatus::Accepted
            | WorkExecutionBindingStatus::Active
    )
}

impl HarnessStore {
    /// Re-authorize an open, never-started Work so the ordinary delivery path
    /// binds it to the member's current AgentSession generation.
    ///
    /// This is a responsibility-plane mutation only: it creates no
    /// WorkExecutionBinding, issues no RuntimeCommand, and never touches the
    /// provider boundary. The Work keeps its phase, condition, owner and
    /// assignee; only its revision advances.
    pub fn redeliver_work_to_current_session(
        &self,
        work_id: &str,
        expected_version: u64,
        execution_space_id: &str,
        reason: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Rebound,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "WORK_TERMINAL_NOT_REDELIVERABLE: Work {work_id} is closed; create a new Work instead of redelivering a terminal one"
            )));
        }
        if current.phase != WorkPhase::Open {
            return Err(StoreError::Conflict(format!(
                "WORK_ALREADY_STARTED: Work {work_id} is in phase {:?}; its delivery already began execution, so use request-changes or release instead of redelivery",
                current.phase
            )));
        }
        let (Some(membership_id), Some(agent_member_id)) = (
            current.assignee_membership_id.clone(),
            current.owner_member_id.clone(),
        ) else {
            return Err(StoreError::Conflict(format!(
                "WORK_NOT_ASSIGNED: Work {work_id} has no TeamMembership assignee to redeliver to; use team-run work assign"
            )));
        };
        let bindings = self.fabric_work_execution_bindings(execution_space_id)?;
        if bindings.iter().any(|binding| {
            binding.work_id == current.id && binding_can_still_execute(binding.status)
        }) {
            return Err(StoreError::Conflict(format!(
                "WORK_DELIVERY_LIVE: Work {work_id} still has an execution binding that can reach the provider, so its delivery is not stale; close the member runtime, or use team-run work release / team-run work assign"
            )));
        }
        let sessions = self.fabric_agent_sessions(execution_space_id)?;
        let mut superseded = self
            .canonical_work_deliveries_for_work_unlocked(&current)?
            .into_iter()
            .filter(|delivery| {
                delivery.work_revision == current.version
                    && matches!(
                        delivery.status,
                        WorkDeliveryStatus::Queued
                            | WorkDeliveryStatus::Claimed
                            | WorkDeliveryStatus::ProviderReceived
                    )
            })
            .map(|delivery| {
                let binding = bindings
                    .iter()
                    .find(|binding| binding.id == delivery.work_execution_binding_id);
                SupersededWorkDelivery {
                    stale_because: delivery_staleness(&delivery, binding, &sessions),
                    delivery_id: delivery.id.clone(),
                    delivery_version: delivery.version,
                    status: delivery.status,
                    work_revision: delivery.work_revision,
                    work_execution_binding_id: delivery.work_execution_binding_id.clone(),
                    work_execution_binding_status: binding.map(|binding| binding.status),
                    recipient_agent_member_id: delivery.recipient_agent_member_id.clone(),
                    recipient_session_id: delivery.recipient_session_id.clone(),
                    recipient_session_generation: delivery.recipient_session_generation,
                    provider_receipt_id: delivery.provider_receipt_id.clone(),
                }
            })
            .collect::<Vec<_>>();
        superseded.sort_by(|left, right| left.delivery_id.cmp(&right.delivery_id));
        if superseded.is_empty() {
            return Err(StoreError::Conflict(format!(
                "WORK_HAS_NO_UNSTARTED_DELIVERY: Work {work_id} has no queued, claimed, or provider-received WorkDelivery at revision {} to supersede",
                current.version
            )));
        }
        // The member's current AgentSession, if its runtime is already up. It
        // is recorded as evidence only: redelivery must also work while the
        // reopened runtime is still starting, and the binding that actually
        // freezes a generation is written later by the NodeDaemon.
        let target_session = sessions.iter().find(|session| {
            session.agent_member_id == agent_member_id
                && session.lifecycle != AgentSessionStatus::Closed
        });
        let mut next = current.clone();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Rebound,
            context,
            serde_json::json!({
                "redelivery": true,
                "reason": reason,
                "assignee_membership_id": membership_id,
                "assignee_agent_member_id": agent_member_id,
                "superseded_deliveries": superseded,
                "target_agent_session_id": target_session.map(|session| session.id.clone()),
                "target_agent_session_generation": target_session
                    .map(|session| session.runtime_generation),
            }),
        )
    }
}

/// Name the exact reason one delivery can never reach the provider again. The
/// value is durable evidence in the `Rebound` WorkEvent, so it stays a stable
/// snake_case token rather than prose.
fn delivery_staleness(
    delivery: &CanonicalWorkDelivery,
    binding: Option<&WorkExecutionBinding>,
    sessions: &[firm_core::agentfirm_api::AgentSession],
) -> String {
    let Some(binding) = binding else {
        return "work_execution_binding_missing".to_string();
    };
    if !binding_can_still_execute(binding.status) {
        return match binding.status {
            WorkExecutionBindingStatus::Released => "work_execution_binding_released",
            WorkExecutionBindingStatus::Completed => "work_execution_binding_completed",
            WorkExecutionBindingStatus::Invalidated => "work_execution_binding_invalidated",
            _ => "work_execution_binding_not_executable",
        }
        .to_string();
    }
    let Some(session) = sessions
        .iter()
        .find(|session| session.id == delivery.recipient_session_id)
    else {
        return "agent_session_missing".to_string();
    };
    if session.lifecycle == AgentSessionStatus::Closed {
        return "agent_session_closed".to_string();
    }
    if session.runtime_generation != delivery.recipient_session_generation {
        return "agent_session_generation_advanced".to_string();
    }
    "agent_session_generation_current".to_string()
}
