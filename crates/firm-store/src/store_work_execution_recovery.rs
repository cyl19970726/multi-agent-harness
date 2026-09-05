//! Host recovery of a Work whose execution authority is provably lost.
//!
//! A started Work (`phase == Active`) holds exactly one Active
//! `WorkExecutionBinding` frozen on a MemberRun adapter-process epoch and an
//! AgentSession provider-session epoch (ADR 0065). Two sequences leave such a
//! Work with no honest way forward (GitHub #799, #734):
//!
//! - a NodeDaemon drain or predecessor recovery already invalidated the
//!   binding (`invalidated_by_lost_runtime_generation`), but a started Work is
//!   not `Open`, so the ordinary dispatch path never re-delivers it, the
//!   member cannot submit without an Active binding, and `redeliver` /
//!   `release` refuse a started Work;
//! - the MemberRun epoch advanced without a clean Close, so the binding is
//!   still Active while nothing can ever settle it: the runtime fence admits
//!   only the exact current generations, and the daemon's stale reconciliation
//!   deliberately refuses a provider-received delivery.
//!
//! `recover_lost_work_execution` is the explicit Host authority for exactly
//! that state. It proves the loss from durable records alone — the binding's
//! exact MemberRun generation is no longer the member's current one, the
//! MemberRun has no live runtime authority, or the AgentSession is Closed —
//! and otherwise fails closed with `WORK_EXECUTION_AUTHORITY_LIVE`. When the
//! loss is proven it (1) releases a still-executable binding through the same
//! lost-runtime-generation writer the NodeDaemon uses, recording the claim id
//! and provider receipt as evidence and never asserting a provider outcome,
//! and (2) appends one `ExecutionRecovered` WorkOperation that returns the
//! Work to `Open` with its responsibility intact and its revision advanced.
//! The ordinary NodeDaemon path then binds the new revision to the member's
//! current generation and delivers it again, exactly as after `work assign`.
//! Nothing is deleted or rewritten, and no provider effect is replayed.

use super::store_work_redelivery::{delivery_staleness, SupersededWorkDelivery};
use super::*;
use firm_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSessionStatus, MutationContext, WorkExecutionBinding,
    WorkExecutionBindingStatus,
};

/// Why one Work's execution is judged lost, live, or not lost at all. The
/// tokens are durable evidence in the `ExecutionRecovered` payload and in the
/// `team-run recover` report, so they stay stable snake_case rather than prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkExecutionLoss {
    /// An executable binding whose exact runtime authority is still current.
    Live {
        binding_id: String,
        member_run_id: String,
        member_run_generation: u64,
        agent_session_id: String,
        agent_session_generation: u64,
    },
    /// The Work has no lost execution to recover.
    NotLost { reason: String },
    /// The execution is provably lost.
    Lost(Box<LostExecutionEvidence>),
}

/// The durable facts that prove one execution is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LostExecutionEvidence {
    /// Stable tokens naming each proof, e.g. `member_run_generation_superseded`.
    pub causes: Vec<String>,
    /// The binding that still claims to be executable and must be released by
    /// the recovery, if any. `None` when a NodeDaemon settlement already ended
    /// the binding and only the Work plane is stuck.
    pub executable_binding: Option<WorkExecutionBinding>,
    /// The most recent binding of the Work, whatever its status.
    pub latest_binding_id: Option<String>,
    pub latest_binding_status: Option<WorkExecutionBindingStatus>,
    /// The transition that ended the latest binding, when it is no longer
    /// executable (`invalidated_by_lost_runtime_generation`, `released`, ...).
    pub latest_binding_end_transition: Option<String>,
    /// The `lost_runtime_generation.cause` token of that end event, if any.
    pub latest_binding_end_cause: Option<String>,
    pub member_run_id: Option<String>,
    pub member_run_generation_at_binding: Option<u64>,
    pub member_run_generation_now: Option<u64>,
    pub member_run_has_live_runtime_authority: Option<bool>,
    pub agent_session_id: Option<String>,
    pub agent_session_generation: Option<u64>,
    pub agent_session_lifecycle: Option<AgentSessionStatus>,
}

/// One Work whose execution is provably lost, as reported by
/// `team-run recover` so the Host does not discover it from member complaints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LostWorkExecution {
    pub work_id: String,
    pub work_version: u64,
    pub phase: WorkPhase,
    pub owner_member_id: Option<String>,
    pub assignee_membership_id: Option<String>,
    pub causes: Vec<String>,
    pub executable_binding_id: Option<String>,
    pub latest_binding_end_transition: Option<String>,
}

fn binding_can_still_execute(status: WorkExecutionBindingStatus) -> bool {
    matches!(
        status,
        WorkExecutionBindingStatus::Offered
            | WorkExecutionBindingStatus::Accepted
            | WorkExecutionBindingStatus::Active
    )
}

impl HarnessStore {
    /// Judge one Work's execution from durable records only. Never writes.
    fn classify_work_execution_loss_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        bindings: &[WorkExecutionBinding],
    ) -> StoreResult<WorkExecutionLoss> {
        if work.assignee_membership_id.is_none() || work.owner_member_id.is_none() {
            return Ok(WorkExecutionLoss::NotLost {
                reason: "work_has_no_responsible_member".into(),
            });
        }
        if work.phase == WorkPhase::Review {
            return Ok(WorkExecutionLoss::NotLost {
                reason: "work_is_in_review".into(),
            });
        }
        let mut work_bindings = bindings
            .iter()
            .filter(|binding| binding.work_id == work.id)
            .collect::<Vec<_>>();
        work_bindings.sort_by_key(|binding| binding.binding_generation);
        let latest = work_bindings.last().copied();
        let executable = work_bindings
            .iter()
            .copied()
            .find(|binding| binding_can_still_execute(binding.status));

        let mut evidence = LostExecutionEvidence {
            causes: Vec::new(),
            executable_binding: None,
            latest_binding_id: latest.map(|binding| binding.id.clone()),
            latest_binding_status: latest.map(|binding| binding.status),
            latest_binding_end_transition: None,
            latest_binding_end_cause: None,
            member_run_id: None,
            member_run_generation_at_binding: None,
            member_run_generation_now: None,
            member_run_has_live_runtime_authority: None,
            agent_session_id: None,
            agent_session_generation: None,
            agent_session_lifecycle: None,
        };

        if let Some(binding) = executable {
            let admission = self.work_execution_runtime_binding(execution_space_id, &binding.id)?;
            let (Some(member_run_id), Some(member_run_generation)) = (
                admission.target_member_run_id.clone(),
                admission.target_member_run_generation,
            ) else {
                return Err(StoreError::Conflict(format!(
                    "WORK_EXECUTION_RUNTIME_BINDING_NOT_PROVABLE: WorkExecutionBinding {} carries no exact MemberRun generation",
                    binding.id
                )));
            };
            let member_run = self
                .trust_member_runs(execution_space_id)?
                .into_iter()
                .find(|member| member.id == member_run_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "WORK_EXECUTION_MEMBER_RUN_MISSING: WorkExecutionBinding {} references MemberRun {member_run_id}, which has no canonical source fact",
                        binding.id
                    ))
                })?;
            let session = self
                .fabric_agent_sessions(execution_space_id)?
                .into_iter()
                .find(|session| session.id == binding.agent_session_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "WORK_EXECUTION_SESSION_MISSING: WorkExecutionBinding {} references AgentSession {}, which has no canonical source fact",
                        binding.id, binding.agent_session_id
                    ))
                })?;
            evidence.member_run_id = Some(member_run.id.clone());
            evidence.member_run_generation_at_binding = Some(member_run_generation);
            evidence.member_run_generation_now = Some(member_run.runtime_generation);
            evidence.member_run_has_live_runtime_authority =
                Some(member_run.has_live_runtime_authority());
            evidence.agent_session_id = Some(session.id.clone());
            evidence.agent_session_generation = Some(binding.agent_session_generation);
            evidence.agent_session_lifecycle = Some(session.lifecycle);
            if member_run.runtime_generation != member_run_generation {
                evidence
                    .causes
                    .push("member_run_generation_superseded".into());
            }
            if !member_run.has_live_runtime_authority() {
                evidence
                    .causes
                    .push("member_run_runtime_authority_ended".into());
            }
            if session.lifecycle == AgentSessionStatus::Closed {
                evidence.causes.push("agent_session_closed".into());
            }
            if session.runtime_generation != binding.agent_session_generation {
                evidence
                    .causes
                    .push("agent_session_generation_superseded".into());
            }
            if evidence.causes.is_empty() {
                return Ok(WorkExecutionLoss::Live {
                    binding_id: binding.id.clone(),
                    member_run_id,
                    member_run_generation,
                    agent_session_id: binding.agent_session_id.clone(),
                    agent_session_generation: binding.agent_session_generation,
                });
            }
            evidence.executable_binding = Some(binding.clone());
            return Ok(WorkExecutionLoss::Lost(Box::new(evidence)));
        }

        // No executable binding. An open Work is simply dispatchable (or, when
        // a provider-received delivery froze it, a `redeliver` case); only a
        // started Work is stranded here.
        if work.phase != WorkPhase::Active {
            return Ok(WorkExecutionLoss::NotLost {
                reason: match latest {
                    Some(_) => "open_work_has_no_executable_binding".into(),
                    None => "work_was_never_dispatched".into(),
                },
            });
        }
        let Some(latest) = latest else {
            evidence
                .causes
                .push("started_work_has_no_execution_binding".into());
            return Ok(WorkExecutionLoss::Lost(Box::new(evidence)));
        };
        let end_event = self
            .canonical_operations()?
            .into_iter()
            .map(|operation| operation.event)
            .rfind(|event| {
                event.aggregate_kind == "work_execution_binding"
                    && event.aggregate_id == latest.id
                    && event.transition != "bound"
            });
        evidence.latest_binding_end_transition =
            end_event.as_ref().map(|event| event.transition.clone());
        evidence.latest_binding_end_cause = end_event
            .as_ref()
            .and_then(|event| event.payload["lost_runtime_generation"]["cause"].as_str())
            .map(str::to_string);
        evidence.causes.push(format!(
            "started_work_binding_{}",
            serde_snake(latest.status)
        ));
        Ok(WorkExecutionLoss::Lost(Box::new(evidence)))
    }

    /// Read-only report of every non-terminal Work of one TeamRun whose
    /// execution is provably lost. Used by `team-run recover`.
    pub fn lost_work_executions(
        &self,
        execution_space_id: &str,
        team_run_id: &str,
    ) -> StoreResult<Vec<LostWorkExecution>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let bindings = self.fabric_work_execution_bindings(execution_space_id)?;
        let mut works = self
            .latest_works_unlocked()?
            .into_values()
            .filter(|work| work.team_run_id == team_run_id && !work.is_terminal())
            .collect::<Vec<_>>();
        works.sort_by(|left, right| left.id.cmp(&right.id));
        let mut lost = Vec::new();
        for work in works {
            if let WorkExecutionLoss::Lost(evidence) =
                self.classify_work_execution_loss_unlocked(execution_space_id, &work, &bindings)?
            {
                lost.push(LostWorkExecution {
                    work_id: work.id.clone(),
                    work_version: work.version,
                    phase: work.phase,
                    owner_member_id: work.owner_member_id.clone(),
                    assignee_membership_id: work.assignee_membership_id.clone(),
                    causes: evidence.causes.clone(),
                    executable_binding_id: evidence
                        .executable_binding
                        .as_ref()
                        .map(|binding| binding.id.clone()),
                    latest_binding_end_transition: evidence.latest_binding_end_transition.clone(),
                });
            }
        }
        Ok(lost)
    }

    /// Return a Work whose execution authority is provably lost to the
    /// dispatchable state, releasing its dead binding when one still claims to
    /// be executable. Fails closed whenever the loss cannot be proven.
    pub fn recover_lost_work_execution(
        &self,
        work_id: &str,
        expected_version: u64,
        execution_space_id: &str,
        reason: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let require_work_execution_space = |work: &Work| -> StoreResult<String> {
            let run = self.require_team_run_unlocked(&work.team_run_id)?;
            let work_execution_space_id = self.current_team_run_execution_space(&run)?;
            if execution_space_id != work_execution_space_id {
                return Err(StoreError::Conflict(format!(
                    "EXECUTION_SPACE_SCOPE_MISMATCH: Work {work_id} belongs to Execution Space {work_execution_space_id}, not caller-supplied Execution Space {execution_space_id}"
                )));
            }
            Ok(work_execution_space_id)
        };
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::ExecutionRecovered,
        )? {
            require_work_execution_space(&existing.work)?;
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        let work_execution_space_id = require_work_execution_space(&current)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "WORK_TERMINAL_NOT_RECOVERABLE: Work {work_id} is closed; create a new Work instead of recovering a terminal one"
            )));
        }
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work is read/export evidence and cannot be recovered"
                    .to_string(),
            ));
        }
        if current.phase == WorkPhase::Review {
            return Err(StoreError::Conflict(format!(
                "WORK_IN_REVIEW_NOT_RECOVERABLE: Work {work_id} is awaiting review; accept it or use request-changes"
            )));
        }
        let (Some(membership_id), Some(agent_member_id)) = (
            current.assignee_membership_id.clone(),
            current.owner_member_id.clone(),
        ) else {
            return Err(StoreError::Conflict(format!(
                "WORK_NOT_ASSIGNED: Work {work_id} has no TeamMembership assignee whose execution could be lost; use team-run work assign"
            )));
        };
        let bindings = self.fabric_work_execution_bindings(&work_execution_space_id)?;
        let evidence = match self.classify_work_execution_loss_unlocked(
            &work_execution_space_id,
            &current,
            &bindings,
        )? {
            WorkExecutionLoss::Live {
                binding_id,
                member_run_id,
                member_run_generation,
                agent_session_id,
                agent_session_generation,
            } => {
                return Err(StoreError::Conflict(format!(
                    "WORK_EXECUTION_AUTHORITY_LIVE: Work {work_id} binding {binding_id} is bound to MemberRun {member_run_id} generation {member_run_generation} and AgentSession {agent_session_id} generation {agent_session_generation}, which are still the member's current runtime authority; interrupt or close the member, or let NodeDaemon settlement invalidate the binding, before recovering"
                )));
            }
            WorkExecutionLoss::NotLost { reason } => {
                return Err(StoreError::Conflict(format!(
                    "WORK_EXECUTION_NOT_LOST: Work {work_id} has no lost execution to recover ({reason})"
                )));
            }
            WorkExecutionLoss::Lost(evidence) => *evidence,
        };

        // Snapshot the current revision's deliveries before the release below
        // rewrites the live one, so the payload records what was superseded.
        let superseded_before = self
            .canonical_work_deliveries_for_work_unlocked(&current)?
            .into_iter()
            .filter(|delivery| {
                // A started Work carries deliveries from the revision it was
                // dispatched at, which is older than its current one.
                delivery.work_revision <= current.version
                    && matches!(
                        delivery.status,
                        WorkDeliveryStatus::Queued
                            | WorkDeliveryStatus::Claimed
                            | WorkDeliveryStatus::ProviderReceived
                    )
            })
            .collect::<Vec<_>>();

        let released_binding = match &evidence.executable_binding {
            Some(binding) => {
                let actor = ActorRef {
                    kind: ActorKind::AgentMember,
                    id: context.performed_by_actor.id.clone(),
                };
                let binding_context = MutationContext {
                    execution_space_id: work_execution_space_id.clone(),
                    authenticated_actor: actor.clone(),
                    authority_actor: Some(actor),
                    command_name: "host.work_execution_binding.release_lost_execution".into(),
                    idempotency_key: format!(
                        "{}:work-execution-binding:{}:{}",
                        context.idempotency_key, binding.id, binding.version
                    ),
                    expected_version: binding.version,
                    request_fingerprint: None,
                };
                let evidence_json = serde_json::json!({
                    "recovered_by": "team-run work recover-lost-execution",
                    "work_id": current.id,
                    "work_revision": current.version,
                    "causes": evidence.causes,
                    "member_run_id": evidence.member_run_id,
                    "member_run_generation_at_binding": evidence.member_run_generation_at_binding,
                    "member_run_generation_now": evidence.member_run_generation_now,
                    "member_run_has_live_runtime_authority": evidence.member_run_has_live_runtime_authority,
                    "agent_session_id": evidence.agent_session_id,
                    "agent_session_generation": evidence.agent_session_generation,
                    "agent_session_lifecycle": evidence.agent_session_lifecycle,
                });
                let (released, _observed) = self.release_lost_execution_binding_unlocked(
                    &binding_context,
                    binding,
                    &evidence_json,
                    &context.created_at,
                )?;
                Some(released.projection)
            }
            None => None,
        };

        let bindings_after = self.fabric_work_execution_bindings(&work_execution_space_id)?;
        let mut superseded = superseded_before
            .into_iter()
            .map(|delivery| {
                let binding = bindings_after
                    .iter()
                    .find(|binding| binding.id == delivery.work_execution_binding_id);
                SupersededWorkDelivery {
                    stale_because: delivery_staleness(binding).to_string(),
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

        // Evidence only, as in `redeliver`: the binding that freezes the new
        // generation is written later by the NodeDaemon.
        let target_session = self
            .fabric_agent_sessions(&work_execution_space_id)?
            .into_iter()
            .find(|session| {
                session.agent_member_id == agent_member_id
                    && session.lifecycle != AgentSessionStatus::Closed
            });
        let mut next = current.clone();
        next.phase = WorkPhase::Open;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        let phase_before = current.phase;
        let condition_before = current.condition;
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::ExecutionRecovered,
            context,
            serde_json::json!({
                "recovery": "lost_execution",
                "reason": reason,
                "phase_before": phase_before,
                "condition_before": condition_before,
                "assignee_membership_id": membership_id,
                "assignee_agent_member_id": agent_member_id,
                "lost_execution": {
                    "causes": evidence.causes,
                    "latest_binding_id": evidence.latest_binding_id,
                    "latest_binding_status": evidence.latest_binding_status,
                    "latest_binding_end_transition": evidence.latest_binding_end_transition,
                    "latest_binding_end_cause": evidence.latest_binding_end_cause,
                    "member_run_id": evidence.member_run_id,
                    "member_run_generation_at_binding": evidence.member_run_generation_at_binding,
                    "member_run_generation_now": evidence.member_run_generation_now,
                    "member_run_has_live_runtime_authority": evidence.member_run_has_live_runtime_authority,
                    "agent_session_id": evidence.agent_session_id,
                    "agent_session_generation": evidence.agent_session_generation,
                    "agent_session_lifecycle": evidence.agent_session_lifecycle,
                },
                "released_binding": released_binding,
                "superseded_deliveries": superseded,
                "target_agent_session_id": target_session.as_ref().map(|session| session.id.clone()),
                "target_agent_session_generation": target_session
                    .as_ref()
                    .map(|session| session.runtime_generation),
            }),
        )
    }
}

fn serde_snake<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}
