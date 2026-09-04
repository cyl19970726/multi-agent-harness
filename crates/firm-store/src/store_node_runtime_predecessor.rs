//! Operator recovery of one exact crashed NodeDaemon predecessor generation.
//!
//! Split out of `store_node_runtime` so the machine-authority writers and this
//! fenced, evidence-gated recovery path each stay one readable seam.

use super::*;

impl HarnessStore {
    /// Explicit hard-crash recovery for one exact predecessor generation.
    ///
    /// This is not authority acquisition. The machine Operator confirms the
    /// external process/process-group facts, while the Store independently
    /// rejects every unknown RuntimeCommand before projecting the dead
    /// generation's sessions and supervisors into a settled state. Only then
    /// may the predecessor lease become Released.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_node_daemon_predecessor(
        &self,
        context: &firm_core::agentfirm_api::MutationContext,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        process_terminated_confirmed: bool,
        provider_process_groups_terminated_confirmed: bool,
        evidence_ref: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<NodeDaemonLease> {
        use firm_core::agentfirm_api::{
            ActorKind, AgentSessionStatus, DriverHandoffState, NativeContinuationActivation,
            RuntimeActivity, RuntimeCommandPhase, RuntimeEffectCertainty, RuntimeResidency,
        };

        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if context.authenticated_actor.kind != ActorKind::Service
            || context.authenticated_actor.id != node_id
        {
            return Err(StoreError::Conflict(
                "NODE_DAEMON_PREDECESSOR_RECOVERY_UNAUTHORIZED: recovery requires the exact Execution Node Operator Service"
                    .into(),
            ));
        }
        if !process_terminated_confirmed
            || !provider_process_groups_terminated_confirmed
            || evidence_ref.trim().is_empty()
        {
            return Err(StoreError::Conflict(
                "NODE_DAEMON_PREDECESSOR_RECOVERY_EVIDENCE_REQUIRED: process termination, provider process-group termination, and a non-empty evidence ref are required"
                    .into(),
            ));
        }
        let mut lease = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
        if lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
        {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: recovery does not match Node {node_id} exact predecessor"
            )));
        }
        if lease.status == NodeDaemonLeaseStatus::Released {
            return Ok(lease);
        }
        if lease.expires_unix_ms > now_unix_ms {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE: generation {generation} has not expired"
            )));
        }

        let execution_space_ids = self.canonical_execution_space_ids()?;
        for execution_space_id in &execution_space_ids {
            if let Some(command) =
                self.runtime_commands(execution_space_id)?
                    .into_iter()
                    .find(|command| {
                        command.target_node_id == node_id
                            && command.target_node_daemon_id == daemon_id
                            && command.target_node_daemon_generation == generation
                            && (!matches!(
                                command.phase,
                                RuntimeCommandPhase::Settled | RuntimeCommandPhase::Rejected
                            ) || !matches!(
                                command.effect_certainty,
                                RuntimeEffectCertainty::Applied
                                    | RuntimeEffectCertainty::NotApplied
                            ))
                    })
            {
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_PREDECESSOR_RECOVERY_COMMAND_UNSETTLED: RuntimeCommand {} is {:?}/{:?}",
                    command.id, command.phase, command.effect_certainty
                )));
            }
        }

        let mut supervisors = latest_by_id(self.team_supervisor_leases()?, |supervisor| {
            supervisor.team_run_id.clone()
        });
        for supervisor in supervisors.values_mut().filter(|supervisor| {
            supervisor.node_id == node_id
                && supervisor.node_daemon_id == daemon_id
                && supervisor.node_daemon_generation == generation
                && supervisor.status != TeamSupervisorLeaseStatus::Released
        }) {
            supervisor.status = TeamSupervisorLeaseStatus::Released;
            supervisor.heartbeat_unix_ms = now_unix_ms;
            supervisor.expires_unix_ms = now_unix_ms;
            supervisor.released_unix_ms = Some(now_unix_ms);
            self.append_jsonl_unlocked("team_supervisor_leases.jsonl", supervisor)?;
        }

        for execution_space_id in execution_space_ids {
            // Every lane the dead generation owned. The Operator's evidence
            // covers the whole process group, so no claim or provider receipt
            // admitted under it can ever be settled by that generation again.
            let lanes = self
                .fabric_agent_sessions(&execution_space_id)?
                .into_iter()
                .filter(|session| {
                    session.node_id == node_id
                        && session.node_daemon_id == daemon_id
                        && session.node_daemon_generation == generation
                })
                .map(|session| crate::LostRuntimeLane {
                    agent_session_id: session.id.clone(),
                    agent_session_generation: session.runtime_generation,
                })
                .collect::<Vec<_>>();
            for mut session in self
                .fabric_agent_sessions(&execution_space_id)?
                .into_iter()
                .filter(|session| {
                    session.node_id == node_id
                        && session.node_daemon_id == daemon_id
                        && session.node_daemon_generation == generation
                })
            {
                if session.control_state.runtime_residency == RuntimeResidency::Detached
                    && session.current_turn_id.is_none()
                {
                    continue;
                }
                session.control_state.runtime_residency = RuntimeResidency::Detached;
                session.control_state.activity = RuntimeActivity::Idle;
                session.control_state.handoff_state = DriverHandoffState::None;
                session.control_state.continuation.activation =
                    NativeContinuationActivation::Disarmed;
                session.control_state.last_reconciled_at = Some(updated_at.to_string());
                session.current_turn_id = None;
                session.queued_input_count = 0;
                if !matches!(
                    session.lifecycle,
                    AgentSessionStatus::Cold
                        | AgentSessionStatus::Idle
                        | AgentSessionStatus::Interrupted
                        | AgentSessionStatus::Closed
                ) {
                    session.lifecycle = AgentSessionStatus::Interrupted;
                }
                session.version = session.version.saturating_add(1);
                session.last_active_at = updated_at.to_string();
                let mut session_context = context.clone();
                session_context.execution_space_id = execution_space_id.clone();
                session_context.command_name =
                    "node_daemon.predecessor_recovery.session_detach".into();
                session_context.idempotency_key =
                    format!("{}:session:{}", context.idempotency_key, session.id);
                session_context.expected_version = session.version.saturating_sub(1);
                session_context.request_fingerprint = None;
                self.commit_trust_projection_unlocked(
                    &session_context,
                    "agent_session",
                    &session.id,
                    "predecessor_process_terminated",
                    serde_json::json!({
                        "node_id": node_id,
                        "daemon_id": daemon_id,
                        "generation": generation,
                        "instance_id": instance_id,
                        "evidence_ref": evidence_ref,
                    }),
                    &session,
                    Vec::new(),
                    Vec::new(),
                )?;
            }
            // Same rule as the in-process drain (#756): hand the dead
            // generation's in-flight Work back to the ordinary dispatch path
            // with a recorded cause instead of replaying its killed turn.
            self.invalidate_lost_generation_work_bindings_unlocked(
                context,
                &execution_space_id,
                &lanes,
                crate::LostRuntimeGenerationCause::NodeDaemonPredecessorRecovery,
                &serde_json::json!({
                    "node_id": node_id,
                    "daemon_id": daemon_id,
                    "node_daemon_generation": generation,
                    "instance_id": instance_id,
                    "evidence_ref": evidence_ref,
                    "process_terminated_confirmed": true,
                    "provider_process_groups_terminated_confirmed": true,
                }),
                updated_at,
            )?;
        }

        self.require_node_daemon_settlement_unlocked(&lease)?;
        lease.status = NodeDaemonLeaseStatus::Released;
        lease.renewed_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms;
        lease.released_unix_ms = Some(now_unix_ms);
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Also used by `release_node_daemon_lease`: no generation may be released
    /// while one of its Sessions or RuntimeCommands is still unsettled.
    pub(crate) fn require_node_daemon_settlement_unlocked(
        &self,
        lease: &NodeDaemonLease,
    ) -> StoreResult<()> {
        use firm_core::agentfirm_api::{
            RuntimeCommandPhase, RuntimeEffectCertainty, RuntimeResidency,
        };

        let supervisors = latest_by_id(self.team_supervisor_leases()?, |supervisor| {
            supervisor.team_run_id.clone()
        });
        if let Some(supervisor) = supervisors.values().find(|supervisor| {
            supervisor.node_id == lease.node_id
                && supervisor.node_daemon_id == lease.daemon_id
                && supervisor.node_daemon_generation == lease.generation
                && supervisor.status != TeamSupervisorLeaseStatus::Released
        }) {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_PREDECESSOR_UNSETTLED: TeamRun {} Supervisor generation {} is not Released",
                supervisor.team_run_id, supervisor.generation
            )));
        }

        for execution_space_id in self.canonical_execution_space_ids()? {
            if let Some(session) = self
                .fabric_agent_sessions(&execution_space_id)?
                .into_iter()
                .find(|session| {
                    session.node_id == lease.node_id
                        && session.node_daemon_id == lease.daemon_id
                        && session.node_daemon_generation == lease.generation
                        && (session.control_state.runtime_residency != RuntimeResidency::Detached
                            || session.current_turn_id.is_some())
                })
            {
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_PREDECESSOR_UNSETTLED: AgentSession {} still has {:?} residency or an active turn",
                    session.id, session.control_state.runtime_residency
                )));
            }
            if let Some(command) = self
                .runtime_commands(&execution_space_id)?
                .into_iter()
                .find(|command| {
                    command.target_node_id == lease.node_id
                        && command.target_node_daemon_id == lease.daemon_id
                        && command.target_node_daemon_generation == lease.generation
                        && (!matches!(
                            command.phase,
                            RuntimeCommandPhase::Settled | RuntimeCommandPhase::Rejected
                        ) || !matches!(
                            command.effect_certainty,
                            RuntimeEffectCertainty::Applied | RuntimeEffectCertainty::NotApplied
                        ))
                })
            {
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_PREDECESSOR_UNSETTLED: RuntimeCommand {} is {:?}/{:?}",
                    command.id, command.phase, command.effect_certainty
                )));
            }
        }
        Ok(())
    }
}
