use super::*;

impl HarnessStore {
    /// Settle the exact in-process daemon's Session ownership after its
    /// Supervisor threads and owned provider process groups have terminated.
    /// This is settlement-only authority: it cannot acquire a successor lease
    /// or admit a new provider effect.
    #[allow(clippy::too_many_arguments)]
    pub fn settle_node_daemon_shutdown_sessions(
        &self,
        context: &firm_core::agentfirm_api::MutationContext,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        provider_process_groups_terminated: bool,
        updated_at: &str,
    ) -> StoreResult<()> {
        use firm_core::agentfirm_api::{
            ActorKind, AgentSessionStatus, DriverHandoffState, NativeContinuationActivation,
            RuntimeActivity, RuntimeCommandPhase, RuntimeEffectCertainty, RuntimeResidency,
        };

        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if context.authenticated_actor.kind != ActorKind::Service
            || context.authenticated_actor.id != daemon_id
        {
            return Err(StoreError::Conflict(
                "NODE_DAEMON_SHUTDOWN_SETTLEMENT_UNAUTHORIZED: settlement requires the exact daemon Service"
                    .into(),
            ));
        }
        if !provider_process_groups_terminated {
            return Err(StoreError::Conflict(
                "NODE_DAEMON_SHUTDOWN_SETTLEMENT_EVIDENCE_REQUIRED: owned provider process groups must be terminal"
                    .into(),
            ));
        }
        let lease = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
        if lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
            || !matches!(
                lease.status,
                NodeDaemonLeaseStatus::Active | NodeDaemonLeaseStatus::Draining
            )
        {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: shutdown settlement does not match Node {node_id} exact daemon generation"
            )));
        }
        if let Some(supervisor) = latest_by_id(self.team_supervisor_leases()?, |supervisor| {
            supervisor.team_run_id.clone()
        })
        .values()
        .find(|supervisor| {
            supervisor.node_id == node_id
                && supervisor.node_daemon_id == daemon_id
                && supervisor.node_daemon_generation == generation
                && supervisor.status != TeamSupervisorLeaseStatus::Released
        }) {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_SHUTDOWN_SUPERVISOR_UNSETTLED: TeamRun {} Supervisor generation {} is not Released",
                supervisor.team_run_id, supervisor.generation
            )));
        }

        for execution_space_id in self.canonical_execution_space_ids()? {
            if let Some(command) = self
                .runtime_commands(&execution_space_id)?
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
                            RuntimeEffectCertainty::Applied | RuntimeEffectCertainty::NotApplied
                        ))
                })
            {
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_SHUTDOWN_COMMAND_UNSETTLED: RuntimeCommand {} is {:?}/{:?}",
                    command.id, command.phase, command.effect_certainty
                )));
            }
            // Every lane this generation owned, mid-turn or not: the drain
            // killed the provider process groups behind all of them, so no
            // claim or provider receipt admitted here can ever be settled by
            // this generation again.
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
                let previous_version = session.version;
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
                session_context.command_name = "node_daemon.shutdown.session_detach".into();
                session_context.idempotency_key = format!(
                    "node-daemon-shutdown:{node_id}:{daemon_id}:{generation}:session:{}",
                    session.id
                );
                session_context.expected_version = previous_version;
                session_context.request_fingerprint = None;
                self.commit_trust_projection_unlocked(
                    &session_context,
                    "agent_session",
                    &session.id,
                    "daemon_shutdown_settled",
                    serde_json::json!({
                        "node_id": node_id,
                        "daemon_id": daemon_id,
                        "generation": generation,
                        "instance_id": instance_id,
                        "provider_process_groups_terminated": true,
                    }),
                    &session,
                    Vec::new(),
                    Vec::new(),
                )?;
            }
            // The killed turn is never replayed. Its in-flight Work is instead
            // handed back to the ordinary dispatch path: the binding is
            // invalidated with the drain as its recorded cause, and the
            // claimed/provider-received delivery is superseded with an
            // explicit failure code, so the successor generation mints a fresh
            // binding generation and a fresh delivery (#756).
            self.invalidate_lost_generation_work_bindings_unlocked(
                context,
                &execution_space_id,
                &lanes,
                crate::LostRuntimeGenerationCause::NodeDaemonDrain,
                &serde_json::json!({
                    "node_id": node_id,
                    "daemon_id": daemon_id,
                    "node_daemon_generation": generation,
                    "instance_id": instance_id,
                    "provider_process_groups_terminated": true,
                }),
                updated_at,
            )?;
        }
        Ok(())
    }
}
