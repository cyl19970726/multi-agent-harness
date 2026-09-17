//! Operator recovery of one exact crashed NodeDaemon predecessor generation.
//!
//! Split out of `store_node_runtime` so the machine-authority writers and this
//! fenced, evidence-gated recovery path each stay one readable seam.

use crate::*;

mod author_message;

/// What one exact predecessor-generation recovery settled in one Execution
/// Space.
///
/// `sessions_already_settled` names every Session this recovery found already
/// detached — by the dying generation's own partial drain, or by an earlier
/// recovery attempt — so the operator reads what recovery skipped instead of
/// inferring it from an absence (#837).
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeDaemonPredecessorRecovery {
    /// The predecessor lease as it stands after recovery.
    pub lease: NodeDaemonLease,
    /// True only when the exact lease was already Released at admission.
    /// This is a response projection, not an additional durable lease field.
    pub already_released: bool,
    /// TeamRun ids whose Supervisor lease this recovery released.
    pub supervisors_released: Vec<String>,
    /// AgentSession ids this recovery detached.
    pub sessions_detached: Vec<String>,
    /// AgentSession ids that were already detached and idle when recovery ran.
    pub sessions_already_settled: Vec<String>,
    /// AgentSession ids this recovery found carrying a `settlement_incomplete`
    /// marker — lanes a dying generation honestly reported it could not settle
    /// (ADR 0073). They are a subset of `sessions_detached`: recovery is the
    /// first party with a termination proof, so it settles them and clears the
    /// marker rather than trusting the dead generation's last look at them.
    pub sessions_settlement_incomplete: Vec<String>,
}

/// One Execution Space's latest lease for the predecessor being recovered.
pub struct PredecessorSpaceLease {
    pub execution_space_id: String,
    pub store: HarnessStore,
    pub lease: NodeDaemonLease,
}

/// The predecessor instance a recovery selected: its latest lease, plus the
/// lease as each registered Space resolved it — one machine document since ADR
/// 0075, so every entry is the same record — still paired with whatever the
/// caller keyed its Spaces by.
pub type SelectedPredecessorSpaces<T> = (NodeDaemonLease, Vec<(T, NodeDaemonLease)>);

/// Choose the one predecessor instance a recovery may touch, from the latest
/// NodeDaemonLease of every registered Execution Space.
///
/// Generations are machine-wide and monotonic since ADR 0075, and every
/// candidate resolves the same document, so the selection is by instance
/// identity and the "different unreleased instances across Spaces" case can no
/// longer be written down: the latest unreleased lease names the instance, and
/// the consistency check below is defence in depth, not a sweep of genuinely
/// divergent rows. What it still refuses is a caller-supplied `expected` tuple
/// that does not match the document — recovery never sweeps an instance nobody
/// asked about.
///
/// `expected` narrows the selection to one exact tuple for callers whose
/// authorization names it (the Operator HTTP action). `None` selects the
/// document's latest instance.
///
/// Both the `daemon recover-predecessor` CLI and the successor daemon's
/// automatic recovery select through this one function, so they can never
/// disagree about which instance is the predecessor (ADR 0073).
pub fn select_exact_predecessor_spaces<T>(
    candidates: Vec<(T, NodeDaemonLease)>,
    expected: Option<(&str, &str, u64)>,
) -> Result<SelectedPredecessorSpaces<T>, (String, String)> {
    let latest = candidates
        .iter()
        .find(|(_, lease)| lease.status != NodeDaemonLeaseStatus::Released)
        .or_else(|| candidates.first())
        .map(|(_, lease)| lease.clone())
        .ok_or_else(|| {
            (
                "SUPERVISOR_GENERATION_FENCED".to_string(),
                "Node has no predecessor lease to recover".to_string(),
            )
        })?;
    if candidates.iter().any(|(_, lease)| {
        lease.status != NodeDaemonLeaseStatus::Released
            && (lease.daemon_id != latest.daemon_id || lease.instance_id != latest.instance_id)
    }) {
        return Err((
            "SUPERVISOR_GENERATION_FENCED".to_string(),
            "Node has different unreleased predecessor instances; recovery must not sweep unrelated instances".to_string(),
        ));
    }
    let spaces: Vec<_> = candidates
        .into_iter()
        .filter(|(_, lease)| {
            if let Some((daemon, instance, generation)) = expected {
                lease.daemon_id == daemon
                    && lease.instance_id == instance
                    && lease.generation == generation
            } else {
                lease.daemon_id == latest.daemon_id && lease.instance_id == latest.instance_id
            }
        })
        .collect();
    if spaces.is_empty()
        || expected.is_some_and(|(daemon, instance, _)| {
            daemon != latest.daemon_id || instance != latest.instance_id
        })
    {
        return Err((
            "SUPERVISOR_GENERATION_FENCED".to_string(),
            "recovery intent does not match the exact latest predecessor".to_string(),
        ));
    }
    Ok((latest, spaces))
}

/// Run `recover_node_daemon_predecessor` for each captured Execution Space and
/// return the operator-readable receipt.
///
/// Errors are `(code, detail)` pairs the caller wraps in its own envelope (HTTP
/// role-action error, CLI usage error, or the daemon's own diagnostics). On
/// partial failure the detail is a JSON receipt retaining successful
/// settlements, so no caller loses what a half-finished recovery did achieve.
/// `evidence_ref` identifies this request; repeating recovery does not replace
/// the evidence ref on rows an earlier successful attempt already settled.
#[allow(clippy::too_many_arguments)]
pub fn recover_predecessor_generation_across_spaces(
    node_id: &str,
    daemon_id: &str,
    instance_id: &str,
    spaces: &[PredecessorSpaceLease],
    actor: &firm_core::agentfirm_api::ActorRef,
    provider_process_groups_terminated_confirmed: bool,
    evidence_ref: &str,
    idempotency_key_prefix: &str,
    request_fingerprint: Option<String>,
    now_unix_ms: u64,
) -> Result<serde_json::Value, (String, String)> {
    let mut recovered_spaces = Vec::new();
    let mut space_settlements = Vec::new();
    let mut failures = Vec::new();
    for space in spaces {
        let lease = &space.lease;
        let context = firm_core::agentfirm_api::MutationContext {
            execution_space_id: space.execution_space_id.clone(),
            authenticated_actor: actor.clone(),
            authority_actor: None,
            command_name: "node_daemon.predecessor_recover".into(),
            idempotency_key: format!(
                "{idempotency_key_prefix}:space:{}",
                space.execution_space_id
            ),
            expected_version: lease.generation,
            request_fingerprint: request_fingerprint.clone(),
        };
        // Phase 1 only. `Released` is one statement about the machine, so it
        // is published once, below, after every Space has proved it owes
        // nothing (ADR 0075).
        match space.store.settle_node_daemon_predecessor_in_space(
            &context,
            node_id,
            daemon_id,
            lease.generation,
            instance_id,
            true,
            provider_process_groups_terminated_confirmed,
            evidence_ref,
            now_unix_ms,
            &format!("unix-ms:{now_unix_ms}"),
        ) {
            // The settlement summary is part of the receipt: an operator must
            // be able to see which Sessions this recovery detached, which it
            // skipped because the dying generation had already settled them
            // (#837), and which carried an unsettled-lane marker (ADR 0073) —
            // not infer the difference from silence.
            Ok(recovery) => {
                space_settlements.push(serde_json::json!({
                    "execution_space_id": space.execution_space_id,
                    "generation": lease.generation,
                    "daemon_id": lease.daemon_id,
                    "instance_id": lease.instance_id,
                    "already_released": recovery.already_released,
                    "supervisors_released": recovery.supervisors_released,
                    "sessions_detached": recovery.sessions_detached,
                    "sessions_already_settled": recovery.sessions_already_settled,
                    "sessions_settlement_incomplete": recovery.sessions_settlement_incomplete,
                }));
                recovered_spaces.push(space.execution_space_id.clone());
            }
            Err(error) => failures.push(format!("{}: {error}", space.execution_space_id)),
        }
    }
    let mut receipt = serde_json::json!({
        "node_id": node_id,
        "daemon_id": daemon_id,
        "instance_id": instance_id,
        "generation": spaces.first().map(|space| space.lease.generation).filter(|generation| spaces.iter().all(|space| space.lease.generation == *generation)),
        "already_released": failures.is_empty() && space_settlements.iter().all(|row| row["already_released"] == true),
        "space_leases": spaces.iter().map(|space| serde_json::json!({
            "execution_space_id": space.execution_space_id, "daemon_id": space.lease.daemon_id,
            "instance_id": space.lease.instance_id, "generation": space.lease.generation,
        })).collect::<Vec<_>>(),
        "status": "released",
        "recovered_spaces": recovered_spaces,
        "space_settlements": space_settlements,
        "evidence_ref": evidence_ref,
    });
    if !failures.is_empty() {
        // All-or-nothing: no Space's failure may leave the machine looking
        // released. The settlements that did land stay landed and are named in
        // the receipt — they are per-Space facts and remain true — but the
        // document still carries the predecessor generation, so a successor
        // cannot acquire and the retry has something to retry.
        receipt["status"] = serde_json::json!("partial");
        receipt["failures"] = serde_json::json!(failures);
        receipt["authority_released"] = serde_json::json!(false);
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE".to_string(),
            receipt.to_string(),
        ));
    }
    // One publish for the machine, through any Space's Store handle: they all
    // resolve the same `<FIRM_HOME>/nodes/<node_id>/` document.
    if let Some(space) = spaces.first() {
        if !space_settlements
            .iter()
            .all(|row| row["already_released"] == true)
        {
            match space.store.release_machine_lease(
                node_id,
                daemon_id,
                space.lease.generation,
                instance_id,
            ) {
                Ok(_) => {}
                Err(error) => {
                    receipt["status"] = serde_json::json!("partial");
                    receipt["authority_released"] = serde_json::json!(false);
                    receipt["failures"] =
                        serde_json::json!([format!("machine lease release failed: {error}")]);
                    return Err((
                        "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE".to_string(),
                        receipt.to_string(),
                    ));
                }
            }
        }
    }
    receipt["authority_released"] = serde_json::json!(true);
    Ok(receipt)
}

impl HarnessStore {
    /// Explicit hard-crash recovery for one exact predecessor generation, in
    /// one Execution Space: settle everything that Space owes, and publish the
    /// machine's `Released` once it does.
    ///
    /// This is not authority acquisition. The machine Operator confirms the
    /// external process/process-group facts, while the Store independently
    /// rejects every unknown RuntimeCommand before projecting the dead
    /// generation's sessions and supervisors into a settled state. Only then
    /// may the predecessor lease become Released.
    ///
    /// A node serving several Execution Spaces must not use this verb per
    /// Space: the machine has one lease, so the first Space to finish would
    /// publish `Released` while later Spaces still owed settlement. Those
    /// callers gather the proof with
    /// [`HarnessStore::settle_node_daemon_predecessor_in_space`] across every
    /// Space first and publish once — which is what
    /// `recover_predecessor_generation_across_spaces` does.
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
    ) -> StoreResult<NodeDaemonPredecessorRecovery> {
        let settled = self.settle_node_daemon_predecessor_in_space(
            context,
            node_id,
            daemon_id,
            generation,
            instance_id,
            process_terminated_confirmed,
            provider_process_groups_terminated_confirmed,
            evidence_ref,
            now_unix_ms,
            updated_at,
        )?;
        if settled.already_released {
            return Ok(settled);
        }
        Ok(NodeDaemonPredecessorRecovery {
            lease: self.release_machine_lease(node_id, daemon_id, generation, instance_id)?,
            ..settled
        })
    }

    /// Phase 1 of recovery for one Execution Space: settle every obligation
    /// this generation owes *here*, and stop short of publishing the machine's
    /// `Released`.
    ///
    /// Separated from the publish because the machine lease is one document for
    /// the whole node (ADR 0075). "Released" is a statement about the machine,
    /// so it may only be made once every registered Space has proved it owes
    /// nothing — which turns the old continue-past-failure partial release into
    /// one all-or-nothing publish over an explicit proof set, and makes
    /// `authority_released: false` stop meaning "partly".
    #[allow(clippy::too_many_arguments)]
    pub fn settle_node_daemon_predecessor_in_space(
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
    ) -> StoreResult<NodeDaemonPredecessorRecovery> {
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
        // ADR 0075: the lease half of recovery reads and writes the one
        // document. The confirm, dead-socket and dead-pid proofs above are
        // unchanged, and the per-Space session settlement below still walks
        // every Space — only "who owns this machine" moved.
        //
        // Deliberately NOT the authoritative_machine_lease fence: recovery
        // exists to settle a predecessor that no longer authorizes anything,
        // so it must be able to read a lease the fence would refuse.
        let mut lease = self
            .current_machine_lease(node_id)?
            .filter(|(_, source)| source.authorizes_provider_effect())
            .map(|(lease, _)| lease)
            .ok_or_else(|| {
                crate::store_machine_lease::node_daemon_generation_fenced(
                    node_id,
                    node_id.to_string(),
                )
            })?;
        if lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
        {
            return Err(crate::store_machine_lease::node_daemon_generation_fenced(
                node_id,
                format!("recovery does not match Node {node_id} exact predecessor"),
            ));
        }
        if lease.status == NodeDaemonLeaseStatus::Released {
            return Ok(NodeDaemonPredecessorRecovery {
                lease,
                already_released: true,
                supervisors_released: Vec::new(),
                sessions_detached: Vec::new(),
                sessions_already_settled: Vec::new(),
                sessions_settlement_incomplete: Vec::new(),
            });
        }
        if lease.expires_unix_ms > now_unix_ms {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE: generation {generation} has not expired"
            )));
        }

        let execution_space_ids = self.canonical_execution_space_ids()?;
        self.reconcile_predecessor_author_messages_unlocked(
            context,
            &execution_space_ids,
            &lease,
            evidence_ref,
            updated_at,
        )?;
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

        let mut supervisors_released = Vec::new();
        let mut sessions_detached = Vec::new();
        let mut sessions_already_settled = Vec::new();
        let mut sessions_settlement_incomplete = Vec::new();

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
            supervisors_released.push(supervisor.team_run_id.clone());
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
                // A lane the dying generation flagged `settlement_incomplete`
                // is not settled, however idle it looks: that generation had
                // no process-group termination proof when it wrote the flag,
                // which is exactly why it wrote one instead of a settlement.
                // Recovery does have the proof, so it settles the lane and
                // clears the flag (ADR 0073).
                let unsettled_marker = session.control_state.settlement_incomplete.is_some();
                if session.control_state.runtime_residency == RuntimeResidency::Detached
                    && session.current_cycle_marker.is_none()
                    && !unsettled_marker
                {
                    // Already settled by this generation's own partial drain or
                    // by an earlier recovery attempt: recovery records the skip
                    // and never rewrites a settled Session (#837).
                    sessions_already_settled.push(session.id.clone());
                    continue;
                }
                if unsettled_marker {
                    sessions_settlement_incomplete.push(session.id.clone());
                }
                session.control_state.settlement_incomplete = None;
                session.control_state.runtime_residency = RuntimeResidency::Detached;
                session.control_state.activity = RuntimeActivity::Idle;
                session.control_state.handoff_state = DriverHandoffState::None;
                session.control_state.continuation.activation =
                    NativeContinuationActivation::Disarmed;
                session.control_state.last_reconciled_at = Some(updated_at.to_string());
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
                session_context.command_name =
                    "node_daemon.predecessor_recovery.session_detach".into();
                // Same rule as the in-process drain
                // (`node_daemon_shutdown.rs`): the key names the exact
                // predecessor generation being settled, never the caller's
                // recovery-attempt prefix. That prefix is stable across every
                // recovery this Node ever runs (the CLI passes the constant
                // `cli-daemon-recover-predecessor:<node_id>`), so deriving the
                // key from it made the second recovery of one Node collide
                // with the first one's detach of the same Session under a
                // different payload — `IDEMPOTENCY_KEY_REUSED`, with no way to
                // ever release the lease again (#837).
                session_context.idempotency_key = format!(
                    "node-daemon-predecessor-recovery:{node_id}:{daemon_id}:{generation}:{instance_id}:session:{}",
                    session.id
                );
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
                sessions_detached.push(session.id.clone());
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
                    "process_terminated_confirmed": process_terminated_confirmed,
                    "provider_process_groups_terminated_confirmed": provider_process_groups_terminated_confirmed,
                }),
                updated_at,
            )?;
        }

        self.require_node_daemon_settlement_unlocked(&lease)?;
        // ---- end of phase 1 -------------------------------------------------
        // Everything above needs the Space write lock: the settlement proof and
        // the per-Space session/command settlement. The legacy row is written
        // here too, while that lock is still held, because it IS Space data.
        lease.status = NodeDaemonLeaseStatus::Released;
        lease.renewed_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms;
        lease.released_unix_ms = Some(now_unix_ms);
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        drop(_lock);

        // The machine's own `Released` is NOT published here. What is
        // load-bearing is the FUNCTION SPLIT, not this drop: the publish lives
        // in the caller, which only runs after this function — and therefore
        // the Space lock — has returned, so the lease lock is never taken while
        // the Space lock is held (ADR 0075 Rule 2). The explicit drop only
        // shortens the Space lock's hold by the width of the return; deleting
        // it alone changes nothing the debug lock registry can see, because the
        // lease-lock acquisition sits across a function boundary (#993).
        // Publishing is also the caller's decision, because on a multi-Space
        // node it is one statement about the machine and may only be made after
        // every Space has reached this line.
        Ok(NodeDaemonPredecessorRecovery {
            lease,
            already_released: false,
            supervisors_released,
            sessions_detached,
            sessions_already_settled,
            sessions_settlement_incomplete,
        })
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
                            || session.current_cycle_marker.is_some()
                            || session.control_state.settlement_incomplete.is_some())
                })
            {
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_PREDECESSOR_UNSETTLED: AgentSession {} still has {:?} residency, an open cycle, or an unsettled-lane marker",
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
