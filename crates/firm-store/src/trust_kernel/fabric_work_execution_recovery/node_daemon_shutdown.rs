use crate::*;

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
        let lease = self
            .authoritative_machine_lease(node_id)
            .map_err(|error| {
                StoreError::Conflict(format!(
                    "NODE_DAEMON_GENERATION_FENCED: {node_id} has no authoritative machine lease: {}",
                    HarnessStore::machine_lease_refusal_reason(&error)
                ))
            })?
            .into_lease();
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
                // A lane an earlier loss flagged `settlement_incomplete` is not
                // settled, however idle it looks: nothing has yet proven its
                // process group terminal. Settle it now and clear the flag.
                if session.control_state.runtime_residency == RuntimeResidency::Detached
                    && session.current_cycle_marker.is_none()
                    && session.control_state.settlement_incomplete.is_none()
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
                session.control_state.settlement_incomplete = None;
                session.current_cycle_marker = None;
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
                        "provider_process_groups_terminated": provider_process_groups_terminated,
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
                    "provider_process_groups_terminated": provider_process_groups_terminated,
                }),
                updated_at,
            )?;
        }
        Ok(())
    }
    /// Record — never claim — that this exact generation could not settle the
    /// lanes it owned.
    ///
    /// The dying daemon reaches this when its machine authority is already
    /// gone (the Space's latest lease moved to another daemon or instance) or
    /// when its own drain did not converge. In both cases it has no proof that
    /// the owning provider process groups are terminal, so it must not write
    /// `Interrupted`, detach residency, or anything else that reads as
    /// settlement. It writes the honest fact instead: *this generation went
    /// dark here, for this reason*. Predecessor recovery — automatic or
    /// operator-driven — reads the flag, settles the lane under a real
    /// termination proof, and clears it (ADR 0073).
    ///
    /// This writer deliberately takes no lease argument. Its whole purpose is
    /// the case where the lease is no longer this generation's to read; the
    /// fence is instead the exact daemon Service actor plus the exact
    /// node/daemon/generation/instance the marked Sessions must already carry.
    #[allow(clippy::too_many_arguments)]
    pub fn record_node_daemon_settlement_incomplete(
        &self,
        context: &firm_core::agentfirm_api::MutationContext,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        reason: &str,
        updated_at: &str,
    ) -> StoreResult<Vec<String>> {
        use firm_core::agentfirm_api::{ActorKind, RuntimeResidency, SessionSettlementIncomplete};

        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if context.authenticated_actor.kind != ActorKind::Service
            || context.authenticated_actor.id != daemon_id
        {
            return Err(StoreError::Conflict(
                "NODE_DAEMON_SETTLEMENT_INCOMPLETE_UNAUTHORIZED: only the exact daemon Service may record its own unsettled lanes"
                    .into(),
            ));
        }
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "NODE_DAEMON_SETTLEMENT_INCOMPLETE_REASON_REQUIRED: an unsettled lane names why it could not be settled"
                    .into(),
            ));
        }
        let marker = SessionSettlementIncomplete {
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: generation,
            instance_id: instance_id.to_string(),
            reason: reason.to_string(),
            observed_at: updated_at.to_string(),
        };
        let mut recorded = Vec::new();
        for execution_space_id in self.canonical_execution_space_ids()? {
            for mut session in self
                .fabric_agent_sessions(&execution_space_id)?
                .into_iter()
                .filter(|session| {
                    session.node_id == node_id
                        && session.node_daemon_id == daemon_id
                        && session.node_daemon_generation == generation
                })
            {
                // A lane already at rest needs no marker, and the same
                // observation twice is a replay rather than a second record.
                // `observed_at` is deliberately excluded from that comparison:
                // it is when the daemon looked, not what it saw.
                let already_recorded = session
                    .control_state
                    .settlement_incomplete
                    .as_ref()
                    .is_some_and(|existing| {
                        existing.node_id == marker.node_id
                            && existing.node_daemon_id == marker.node_daemon_id
                            && existing.node_daemon_generation == marker.node_daemon_generation
                            && existing.instance_id == marker.instance_id
                            && existing.reason == marker.reason
                    });
                if (session.control_state.runtime_residency == RuntimeResidency::Detached
                    && session.current_cycle_marker.is_none())
                    || already_recorded
                {
                    continue;
                }
                let previous_version = session.version;
                session.control_state.settlement_incomplete = Some(marker.clone());
                session.version = session.version.saturating_add(1);
                session.last_active_at = updated_at.to_string();
                let mut session_context = context.clone();
                session_context.execution_space_id = execution_space_id.clone();
                session_context.command_name = "node_daemon.settlement_incomplete".into();
                // Keyed by the exact dying generation, like the drain and the
                // recovery detach (#837), so repeating the observation replays
                // instead of colliding — and by the reason's digest, because a
                // generation that first loses its drain and then its lease is
                // making a second, different observation of the same lane. The
                // reason is in the payload, so without the digest that second
                // observation reuses the first one's key under a different
                // payload and is refused as IDEMPOTENCY_KEY_REUSED.
                session_context.idempotency_key = format!(
                    "node-daemon-settlement-incomplete:{node_id}:{daemon_id}:{generation}:{instance_id}:{}:session:{}",
                    crate::canonical_json_fingerprint(&serde_json::Value::String(
                        reason.to_string()
                    )),
                    session.id
                );
                session_context.expected_version = previous_version;
                session_context.request_fingerprint = None;
                self.commit_trust_projection_unlocked(
                    &session_context,
                    "agent_session",
                    &session.id,
                    "settlement_incomplete_recorded",
                    serde_json::json!({
                        "node_id": node_id,
                        "daemon_id": daemon_id,
                        "generation": generation,
                        "instance_id": instance_id,
                        "reason": reason,
                    }),
                    &session,
                    Vec::new(),
                    Vec::new(),
                )?;
                recorded.push(session.id.clone());
            }
        }
        Ok(recorded)
    }
}
