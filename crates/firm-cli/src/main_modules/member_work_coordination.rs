use super::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LiveProviderActivityUpdate {
    Updated {
        team_run_id: String,
        agent_member_id: String,
        member_run_id: String,
        member_run_generation: u64,
        provider: String,
        kind: provider_event_api::LiveProviderActivityKind,
        native_event: serde_json::Value,
    },
    Terminal {
        team_run_id: String,
        agent_member_id: String,
        member_run_id: String,
        member_run_generation: u64,
    },
}

pub(super) fn require_live_member_run_generation(
    member_run_id: &str,
    current_generation: u64,
    source_generation: u64,
) -> CliResult<()> {
    if current_generation != source_generation {
        return Err(CliError::Usage(format!(
            "MEMBER_GENERATION_FENCED: live provider activity came from MemberRun {} generation {}, current generation is {}",
            member_run_id, source_generation, current_generation
        )));
    }
    Ok(())
}

pub(super) type LiveMemberActivitySink = Arc<dyn Fn(LiveProviderActivityUpdate) + Send + Sync>;

pub(super) fn emit_live_provider_activity(
    sink: &LiveMemberActivitySink,
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    kind: provider_event_api::LiveProviderActivityKind,
    native_event: serde_json::Value,
) {
    sink(LiveProviderActivityUpdate::Updated {
        team_run_id: ledger.run_id.clone(),
        agent_member_id: member.agent_member_id.clone(),
        member_run_id: member.id.clone(),
        member_run_generation: member.runtime_generation,
        provider: member.provider.clone(),
        kind,
        native_event,
    });
}

#[cfg(test)]
pub(super) fn display_safe_tool_status(status: &str, started_event: bool) -> &'static str {
    match status {
        "in_progress" | "running" | "started" => "running",
        "completed" | "success" | "succeeded" => "completed",
        "failed" | "error" => "failed",
        "cancelled" | "canceled" => "cancelled",
        _ if started_event => "running",
        _ => "completed",
    }
}

pub(super) struct LiveProviderTurnGuard {
    pub(super) sink: Option<LiveMemberActivitySink>,
    pub(super) team_run_id: String,
    pub(super) agent_member_id: String,
    pub(super) member_run_id: String,
    pub(super) member_run_generation: u64,
}

impl LiveProviderTurnGuard {
    pub(super) fn new(
        sink: Option<LiveMemberActivitySink>,
        team_run_id: String,
        agent_member_id: String,
        member_run_id: String,
        member_run_generation: u64,
    ) -> Self {
        Self {
            sink,
            team_run_id,
            agent_member_id,
            member_run_id,
            member_run_generation,
        }
    }
}

impl Drop for LiveProviderTurnGuard {
    fn drop(&mut self) {
        if let Some(sink) = &self.sink {
            sink(LiveProviderActivityUpdate::Terminal {
                team_run_id: self.team_run_id.clone(),
                agent_member_id: self.agent_member_id.clone(),
                member_run_id: self.member_run_id.clone(),
                member_run_generation: self.member_run_generation,
            });
        }
    }
}

/// The orchestrator's serialized view of one run's ledger. Read paths are
/// unlocked (append-only JSONL); every "compute next seq + append" pair holds
/// `write_lock` so concurrent member threads never allocate duplicate seqs.
pub(super) struct TeamRunLedger {
    pub(super) store: HarnessStore,
    pub(super) run_id: String,
    pub(super) supervisor_id: String,
    pub(super) supervisor_generation: u64,
    pub(super) supervisor_valid: Arc<AtomicBool>,
    pub(super) write_lock: Mutex<()>,
}

#[derive(Debug, Clone)]
pub(super) struct ClaimedWork {
    pub(super) work: Work,
    pub(super) delivery: harness_core::agentfirm_api::CanonicalWorkDelivery,
    pub(super) execution_space_id: String,
}

pub(super) fn claim_canonical_work_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
) -> CliResult<Option<ClaimedWork>> {
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    let mut placements = Vec::new();
    for space_id in ledger.store.canonical_execution_space_ids()? {
        let sessions = ledger.store.fabric_agent_sessions(&space_id)?;
        let memberships = ledger.store.fabric_team_memberships(&space_id)?;
        for session in sessions.into_iter().filter(|session| {
            session.agent_member_id == member.agent_member_id
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        }) {
            if let Some(membership) = memberships.iter().find(|membership| {
                membership.agent_member_id == member.agent_member_id
                    && membership.team_id == run.agent_team_id
                    && membership.state == harness_core::agentfirm_api::TeamMembershipStatus::Active
            }) {
                placements.push((space_id.clone(), session, membership.clone()));
            }
        }
    }
    if placements.is_empty() {
        return Ok(None);
    }
    if placements.len() != 1 {
        return Err(CliError::Usage(format!(
            "WORK_EXECUTION_BINDING_AMBIGUOUS: {} has {} current Team/session placements",
            member.agent_member_id,
            placements.len()
        )));
    }
    let (execution_space_id, session, membership) = placements.pop().unwrap();
    let scoped_member_runs = ledger.store.trust_member_runs(&execution_space_id)?;
    let current_member_runs = scoped_member_runs
        .iter()
        .filter(|current| {
            current.id == member.id
                && current.team_run_id == ledger.run_id
                && current.agent_member_id == member.agent_member_id
                && current.runtime_generation == member.runtime_generation
                && current.coordination_status
                    == harness_core::agentfirm_api::MemberCoordinationStatus::Active
        })
        .collect::<Vec<_>>();
    let [current_member_run] = current_member_runs.as_slice() else {
        return Err(CliError::Usage(format!(
            "MEMBER_RUN_GENERATION_FENCED: {} does not resolve to one exact current canonical MemberRun",
            member.id
        )));
    };
    let daemon = ledger
        .store
        .latest_node_daemon_lease(&session.node_id)?
        .filter(|lease| {
            lease.daemon_id == session.node_daemon_id
                && lease.generation == session.node_daemon_generation
                && lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED".into()))?;
    let all_works = ledger.store.latest_works()?;
    let works = all_works
        .iter()
        .filter(|work| {
            work.team_run_id == ledger.run_id
                && work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str())
                && work.assignee_membership_id.as_deref() == Some(membership.id.as_str())
                && !work.is_terminal()
        })
        .cloned()
        .map(|work| (work.id.clone(), work))
        .collect::<BTreeMap<_, _>>();
    let mut bindings = ledger
        .store
        .fabric_work_execution_bindings(&execution_space_id)?;
    for binding in bindings.iter().filter(|binding| {
        binding.team_id == run.agent_team_id
            && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
    }) {
        let reconciliation = ledger.store.release_work_execution_binding_if_stale(
            &canonical_delivery_context(
                &execution_space_id,
                &daemon.daemon_id,
                "node_daemon.work_execution_binding.release_if_stale",
                format!(
                    "daemon:{}:{}:{}:reconcile-stale",
                    daemon.generation, binding.id, binding.version
                ),
                binding.version,
            ),
            &binding.id,
            &daemon.node_id,
            &daemon.daemon_id,
            daemon.generation,
            &now_string(),
        );
        if let Err(error) = reconciliation {
            if error.trust_error().is_some_and(|error| {
                error.code == harness_core::agentfirm_api::TrustErrorCode::DeliveryRecoveryUncertain
            }) {
                // This is a Work-attempt recovery fence, not a provider
                // transport failure.  Keep the persistent runtime/live
                // control registered so it can still answer Close/Retire,
                // while preventing this Work from being released or replayed.
                continue;
            }
            return Err(error.into());
        }
    }
    bindings = ledger
        .store
        .fabric_work_execution_bindings(&execution_space_id)?;
    for work in works
        .values()
        .filter(|work| harness_core::work_readiness(work, &all_works).ready)
    {
        if !bindings.iter().any(|binding| {
            binding.work_id == work.id
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        }) {
            let binding_generation = bindings
                .iter()
                .filter(|binding| binding.work_id == work.id)
                .map(|binding| binding.binding_generation)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let binding_id = format!(
                "work-binding:{}:{}:{}:{}",
                work.id, work.version, session.runtime_generation, binding_generation
            );
            let runtime_binding =
                runtime_command_binding_for_member_session(current_member_run, &session);
            ledger.store.bind_responsible_work_execution(
                &canonical_delivery_context(
                    &execution_space_id,
                    &daemon.daemon_id,
                    "node_daemon.work_execution_binding.bind",
                    binding_id.clone(),
                    0,
                ),
                &runtime_binding,
                harness_core::agentfirm_api::WorkExecutionBinding {
                    id: binding_id.clone(),
                    work_id: work.id.clone(),
                    work_revision: work.version,
                    team_id: run.agent_team_id.clone(),
                    team_membership_id: membership.id.clone(),
                    agent_member_id: member.agent_member_id.clone(),
                    agent_session_id: session.id.clone(),
                    agent_session_generation: session.runtime_generation,
                    delivery_id: format!("work-delivery:{}:{binding_generation}", work.id),
                    binding_generation,
                    status: harness_core::agentfirm_api::WorkExecutionBindingStatus::Active,
                    version: 1,
                    created_by: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: daemon.daemon_id.clone(),
                    },
                    bound_at: now_string(),
                    ended_at: None,
                },
            )?;
        }
    }
    bindings = ledger
        .store
        .fabric_work_execution_bindings(&execution_space_id)?;
    let active_binding_ids = bindings
        .into_iter()
        .filter(|binding| {
            binding.agent_member_id == member.agent_member_id
                && binding.agent_session_id == session.id
                && binding.agent_session_generation == session.runtime_generation
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        })
        .map(|binding| binding.id)
        .collect::<BTreeSet<_>>();
    let mut queued = ledger
        .store
        .fabric_work_deliveries(&execution_space_id)?
        .into_iter()
        .filter(|delivery| {
            active_binding_ids.contains(&delivery.work_execution_binding_id)
                && delivery.status == harness_core::agentfirm_api::WorkDeliveryStatus::Queued
        })
        .collect::<Vec<_>>();
    queued.sort_by(|left, right| {
        let left_work = works.get(&left.work_id);
        let right_work = works.get(&right.work_id);
        match (left_work, right_work) {
            (Some(left_work), Some(right_work)) => work_priority_rank(right_work.priority)
                .cmp(&work_priority_rank(left_work.priority))
                .then_with(|| left_work.created_at.cmp(&right_work.created_at))
                .then_with(|| left_work.id.cmp(&right_work.id)),
            _ => left.id.cmp(&right.id),
        }
    });
    for delivery in queued {
        let Some(work) = works.get(&delivery.work_id) else {
            continue;
        };
        if work.version != delivery.work_revision
            || work.owner_member_id.as_deref() != Some(member.agent_member_id.as_str())
            || work.assignee_membership_id.as_deref() != Some(membership.id.as_str())
            || work.is_terminal()
            || !harness_core::work_readiness(work, &all_works).ready
        {
            continue;
        }
        let claim_id = generated_id("canonical-work-claim");
        let requested_mode = harness_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle;
        let dispatch_mode = crate::provider_adapter::effective_delivery_mode(
            &session.provider_kind,
            requested_mode,
            session.lifecycle,
            false,
        )
        .map_err(CliError::Usage)?;
        ledger.store.claim_work_for_provider(
            &canonical_delivery_context(
                &execution_space_id,
                &daemon.daemon_id,
                "node_daemon.work_delivery.claim",
                format!("daemon:{}:{}:claim", daemon.generation, delivery.id),
                delivery.version.saturating_sub(1),
            ),
            &delivery.id,
            &session.node_id,
            &daemon.daemon_id,
            daemon.generation,
            &claim_id,
            dispatch_mode,
            &now_string(),
        )?;
        let claimed = ledger
            .store
            .fabric_work_deliveries(&execution_space_id)?
            .into_iter()
            .find(|candidate| candidate.id == delivery.id)
            .ok_or_else(|| CliError::Usage("WorkDelivery disappeared after claim".into()))?;
        return Ok(Some(ClaimedWork {
            work: work.clone(),
            delivery: claimed,
            execution_space_id,
        }));
    }
    Ok(None)
}

pub(super) fn is_active_work_continuation_candidate(
    work: &Work,
    agent_member_id: &str,
    all_works: &[Work],
) -> bool {
    work.owner_member_id.as_deref() == Some(agent_member_id)
        && work.phase == WorkPhase::Active
        && work.condition == WorkCondition::Normal
        && work.prerequisites_satisfied(all_works.iter())
}

impl TeamRunLedger {
    pub(super) fn new(
        store: &HarnessStore,
        run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        supervisor_valid: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store: store.clone(),
            run_id: run_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            supervisor_generation,
            supervisor_valid,
            write_lock: Mutex::new(()),
        }
    }

    /// Read/journal helper for maintenance paths that do not start provider
    /// side effects. Any attempt to claim mail through this view fails.
    pub(super) fn without_supervisor(store: &HarnessStore, run_id: &str) -> Self {
        Self {
            store: store.clone(),
            run_id: run_id.to_string(),
            supervisor_id: "none".to_string(),
            supervisor_generation: 0,
            supervisor_valid: Arc::new(AtomicBool::new(false)),
            write_lock: Mutex::new(()),
        }
    }

    pub(super) fn require_supervisor_lease(&self) -> CliResult<()> {
        if !self.supervisor_valid.load(Ordering::Acquire) {
            return Err(supervisor_lease_lost_error(&self.run_id));
        }
        let now = current_unix_ms_u64();
        let Some(lease) = self.store.latest_team_supervisor_lease(&self.run_id)? else {
            return Err(latch_supervisor_lease_lost(
                &self.supervisor_valid,
                &self.run_id,
                &self.supervisor_id,
                self.supervisor_generation,
                "durable lease row is missing",
            ));
        };
        if lease.status != harness_core::TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != self.supervisor_id
            || lease.generation != self.supervisor_generation
            || lease.expires_unix_ms <= now
        {
            return Err(latch_supervisor_lease_lost(
                &self.supervisor_valid,
                &self.run_id,
                &self.supervisor_id,
                self.supervisor_generation,
                "durable lease moved, expired, or was released",
            ));
        }
        Ok(())
    }

    pub(super) fn write_lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write_lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Fold one event into the run's event log (seq assigned under the lock).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fold_event(
        &self,
        source_kind: TeamRunEventSourceKind,
        member_run_id: Option<String>,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
        summary: &str,
    ) -> CliResult<TeamRunEvent> {
        let _guard = self.write_lock();
        let seq = next_team_run_seq(&self.store, &self.run_id)?;
        append_team_run_event(
            &self.store,
            &self.run_id,
            seq,
            source_kind,
            member_run_id,
            entity_type,
            entity_id,
            operation,
            summary,
        )
    }

    /// Append one MemberAction (seq = max existing action seq for the run + 1,
    /// assigned under the lock).
    pub(super) fn append_action(
        &self,
        member_run_id: &str,
        action_type: &str,
        status: MemberActionStatus,
        title: &str,
        summary: &str,
    ) -> CliResult<MemberAction> {
        self.append_action_with_provider_status(
            member_run_id,
            action_type,
            status,
            title,
            summary,
            None,
            &[],
        )
    }

    /// `provider_status` carries the transport's OWN terminal metadata in a
    /// machine-readable token. It is the only field a capacity classifier may
    /// read. `summary` is a Harness-owned coordination fact and must never copy
    /// the provider-authored response or transcript.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_action_with_provider_status(
        &self,
        member_run_id: &str,
        action_type: &str,
        status: MemberActionStatus,
        title: &str,
        summary: &str,
        provider_status: Option<String>,
        evidence_refs: &[String],
    ) -> CliResult<MemberAction> {
        let _guard = self.write_lock();
        self.append_action_locked(
            member_run_id,
            action_type,
            status,
            title,
            summary,
            provider_status,
            evidence_refs,
        )
    }

    /// MemberAction append with the write lock already held by the caller, so
    /// a bounded check-then-append receipt shares one critical section with
    /// the seq assignment.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_action_locked(
        &self,
        member_run_id: &str,
        action_type: &str,
        status: MemberActionStatus,
        title: &str,
        summary: &str,
        provider_status: Option<String>,
        evidence_refs: &[String],
    ) -> CliResult<MemberAction> {
        let seq = self
            .store
            .member_actions()?
            .into_iter()
            .filter(|action| action.team_run_id == self.run_id)
            .map(|action| action.seq)
            .max()
            .unwrap_or(0)
            + 1;
        let completed_at = (!matches!(
            status,
            MemberActionStatus::Started | MemberActionStatus::Progress
        ))
        .then(now_string);
        let action = MemberAction {
            id: generated_id("mact"),
            seq,
            team_run_id: self.run_id.clone(),
            member_run_id: member_run_id.to_string(),
            task_id: None,
            provider_call_id: None,
            action_type: action_type.to_string(),
            status,
            provider_status,
            semantic_status: None,
            title: title.to_string(),
            summary: summary.to_string(),
            evidence_refs: evidence_refs.to_vec(),
            started_at: now_string(),
            completed_at,
        };
        self.store.append_member_action(&action)?;
        Ok(action)
    }

    /// Append at most one durable `provider_control` receipt per ProviderRuntimeProjection
    /// and receipt identity. The convergence key is the stable
    /// (member_run_id, "provider_control", title) triple, so an unrelated
    /// provider_control row never suppresses a distinct receipt. The first
    /// routine acknowledgement proves the control policy became active; later
    /// identical control effects are performed without growing the activity
    /// stream. The existence check and the append share one write lock so
    /// overlapping acknowledgements cannot duplicate the row. Returns true
    /// when this call wrote the receipt.
    pub(super) fn append_provider_control_receipt_once(
        &self,
        expected_member: &ProviderRuntimeProjection,
        title: &str,
        summary: &str,
    ) -> CliResult<bool> {
        self.append_provider_control_receipt_once_with_hook(expected_member, title, summary, || {
            Ok(())
        })
    }

    pub(super) fn append_provider_control_receipt_once_with_hook<F>(
        &self,
        expected_member: &ProviderRuntimeProjection,
        title: &str,
        summary: &str,
        before_atomic_append: F,
    ) -> CliResult<bool>
    where
        F: FnOnce() -> CliResult<()>,
    {
        let _guard = self.write_lock();
        let seq = self
            .store
            .member_actions()?
            .into_iter()
            .filter(|action| action.team_run_id == self.run_id)
            .map(|action| action.seq)
            .max()
            .unwrap_or(0)
            + 1;
        let action = MemberAction {
            id: generated_id("mact"),
            seq,
            team_run_id: self.run_id.clone(),
            member_run_id: expected_member.id.clone(),
            task_id: None,
            provider_call_id: None,
            action_type: "provider_control".to_string(),
            status: MemberActionStatus::Succeeded,
            provider_status: None,
            semantic_status: None,
            title: title.to_string(),
            summary: summary.to_string(),
            evidence_refs: Vec::new(),
            started_at: now_string(),
            completed_at: Some(now_string()),
        };
        before_atomic_append()?;
        Ok(self
            .store
            .append_member_action_if_member_run_current(expected_member, &action)?)
    }

    pub(super) fn save_member_run(
        &self,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
    ) -> CliResult<()> {
        let _guard = self.write_lock();
        if let Err(error) = self.store.compare_and_append_member_run(expected, next) {
            let already_committed = expected.id == next.id
                && self
                    .latest_member_run(&next.id)?
                    .as_ref()
                    .is_some_and(|current| current == next);
            if !already_committed {
                return Err(CliError::Store(error));
            }
        }
        self.sync_trust_native_session_binding(next)?;
        Ok(())
    }

    /// Post-settle write-back of the provider-native Session binding onto the
    /// trust fabric. A fresh-start MemberRun/AgentSession is materialized
    /// before the provider thread exists, so the settled binding must reach
    /// the trust MemberRun and the current AgentSession here — every provider
    /// driver persists its settle through `save_member_run`. Only the binding
    /// fields cross; `provider-source:` stays provenance and no provider
    /// stream enters a Harness ledger. Best-effort: stores without
    /// materialized trust fabric skip, and a stale-generation settle never
    /// rebinds a newer trust row.
    pub(super) fn sync_trust_native_session_binding(
        &self,
        next: &ProviderRuntimeProjection,
    ) -> CliResult<()> {
        let Some(native) = next.native_session.as_ref() else {
            return Ok(());
        };
        let Some(space_id) = self.store.trust_member_run_scope(&next.id)? else {
            return Ok(());
        };
        let native_ref = agentfirm_native_session_ref(native);
        let binding_fingerprint = harness_store::canonical_json_fingerprint(
            &serde_json::to_value(&native_ref).map_err(CliError::Json)?,
        );
        // Trust MemberRun binding. One re-read retry absorbs a concurrent trust
        // mutation (close/reopen) racing this settle.
        for attempt in 0..2 {
            let Some(trust_run) = self
                .store
                .trust_member_runs(&space_id)?
                .into_iter()
                .find(|run| run.id == next.id)
            else {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "NATIVE_SESSION_BINDING_INCOMPLETE: canonical MemberRun {} is missing",
                    next.id
                )));
            };
            if trust_run.runtime_generation != next.runtime_generation {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "NATIVE_SESSION_BINDING_GENERATION_FENCED: canonical MemberRun {} is generation {}, settlement is generation {}",
                    next.id, trust_run.runtime_generation, next.runtime_generation
                )));
            }
            if trust_run.native_session.as_ref() == Some(&native_ref) {
                break;
            }
            let context = harness_core::agentfirm_api::MutationContext {
                execution_space_id: space_id.clone(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: "node-daemon:member-run-native-bind".into(),
                },
                authority_actor: None,
                command_name: "team_run.member_run_native_session.bind".into(),
                idempotency_key: format!(
                    "member-run-native-bind:{}:{}:{}",
                    next.id, native.native_session_id, binding_fingerprint
                ),
                expected_version: trust_run.version,
                request_fingerprint: None,
            };
            match self.store.bind_member_run_native_session(
                &context,
                &next.id,
                next.runtime_generation,
                native_ref.clone(),
                &now_string(),
            ) {
                Ok(_) => break,
                Err(error) if attempt == 0 => {
                    let _ = error;
                    continue;
                }
                Err(error) => return Err(CliError::Store(error)),
            }
        }
        // Current AgentSession binding for the same identity + generation. The
        // daemon actor is read from the session row: the bind mutation proves
        // the exact owning NodeDaemon generation.
        for attempt in 0..2 {
            let sessions = self
                .store
                .fabric_agent_sessions(&space_id)?
                .into_iter()
                .filter(|session| {
                    session.agent_member_id == next.agent_member_id
                        && session.runtime_generation == next.runtime_generation
                        && session.lifecycle
                            != harness_core::agentfirm_api::AgentSessionStatus::Closed
                })
                .collect::<Vec<_>>();
            let session = match sessions.as_slice() {
                [] => return Ok(()),
                [session] => session,
                _ => {
                    return Err(CliError::RuntimeRecoveryRequired(format!(
                    "NATIVE_SESSION_BINDING_AMBIGUOUS: MemberRun {} has {} current AgentSessions",
                    next.id,
                    sessions.len()
                )))
                }
            };
            if session.native_session_ref.as_ref() == Some(&native_ref) {
                break;
            }
            let context = harness_core::agentfirm_api::MutationContext {
                execution_space_id: space_id.clone(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: session.node_daemon_id.clone(),
                },
                authority_actor: None,
                command_name: "runtime_fabric.session_native_session.bind".into(),
                idempotency_key: format!(
                    "agent-session-native-bind:{}:{}:{}",
                    session.id, native.native_session_id, binding_fingerprint
                ),
                expected_version: session.version,
                request_fingerprint: None,
            };
            match self.store.bind_agent_session_native_session(
                &context,
                &session.id,
                next.runtime_generation,
                native_ref.clone(),
            ) {
                Ok(_) => break,
                Err(error) if attempt == 0 => {
                    let _ = error;
                    continue;
                }
                Err(error) => return Err(CliError::Store(error)),
            }
        }
        Ok(())
    }

    pub(super) fn latest_member_run(
        &self,
        member_run_id: &str,
    ) -> CliResult<Option<ProviderRuntimeProjection>> {
        Ok(latest_member_runs_in_append_order(&self.store)?
            .into_iter()
            .find(|member| member.id == member_run_id))
    }

    /// Latest-wins messages of this run, in append order.
    pub(super) fn canonical_team_messages(&self) -> CliResult<Vec<TeamMessageProjection>> {
        canonical_team_messages_for_run(&self.store, &self.run_id)
    }

    pub(super) fn queued_works_for(
        &self,
        member_id: &str,
    ) -> CliResult<Vec<(Work, harness_application::CurrentWorkDeliveryView)>> {
        let member = self
            .latest_member_run(member_id)?
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?;
        let all_works = self.store.latest_works()?;
        let works = all_works
            .iter()
            .filter(|work| work.team_run_id == self.run_id)
            .cloned()
            .map(|work| (work.id.clone(), work))
            .collect::<std::collections::HashMap<_, _>>();
        let mut queued = self
            .store
            .current_work_deliveries_for_team_run(&self.run_id)?
            .into_iter()
            .filter(|delivery| {
                delivery.team_run_id == self.run_id
                    && delivery.recipient_agent_member_id.as_deref()
                        == Some(member.agent_member_id.as_str())
                    && delivery.status == harness_core::agentfirm_api::WorkDeliveryStatus::Queued
            })
            .filter_map(|delivery| {
                let work = works.get(&delivery.work_id)?;
                (work.version == delivery.work_revision
                    && work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str())
                    && !work.is_terminal()
                    && harness_core::work_readiness(work, &all_works).ready)
                    .then(|| (work.clone(), delivery))
            })
            .collect::<Vec<_>>();
        queued.sort_by(|(left, _), (right, _)| {
            work_priority_rank(right.priority)
                .cmp(&work_priority_rank(left.priority))
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(queued)
    }

    pub(super) fn claim_canonical_work_for(
        &self,
        member_id: &str,
    ) -> CliResult<Option<ClaimedWork>> {
        let member = self
            .latest_member_run(member_id)?
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?;
        claim_canonical_work_for_member(self, &member)
    }

    /// Return the member's sole durable active Work, or the next ready shared-
    /// pool Work it is eligible to claim, when no ownership delivery exists.
    /// This drives another provider-native cycle without fabricating a
    /// delivery row: self-claim is discovered from the shared board and the
    /// Member must perform the explicit atomic claim itself.
    pub(super) fn active_work_continuation_for(&self, member_id: &str) -> CliResult<Option<Work>> {
        let member = self
            .latest_member_run(member_id)?
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?;
        let all_works = self.store.latest_works()?;
        let mut active = all_works
            .iter()
            .filter(|work| {
                work.team_run_id == self.run_id
                    && is_active_work_continuation_candidate(
                        work,
                        &member.agent_member_id,
                        &all_works,
                    )
            })
            .cloned()
            .collect::<Vec<_>>();
        active.sort_by(|left, right| {
            work_priority_rank(right.priority)
                .cmp(&work_priority_rank(left.priority))
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        if active.len() > 1 {
            return Err(CliError::Usage(format!(
                "member {member_id} has multiple active Works; reconcile ownership before continuing its provider session"
            )));
        }
        if let Some(work) = active.pop() {
            return Ok(Some(work));
        }

        let stable_member_id = member.agent_member_id.as_str();
        let mut claimable = all_works
            .iter()
            .filter(|work| {
                work.team_run_id == self.run_id
                    && work.phase == WorkPhase::Open
                    && work.owner_member_id.is_none()
                    && work.claim_mode == WorkClaimMode::TeamClaim
                    && work.prerequisites_satisfied(all_works.iter())
                    && (work.eligible_member_ids.is_empty()
                        || work
                            .eligible_member_ids
                            .iter()
                            .any(|eligible| eligible == stable_member_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        claimable.sort_by(|left, right| {
            work_priority_rank(right.priority)
                .cmp(&work_priority_rank(left.priority))
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(claimable.into_iter().next())
    }

    /// A replacement runtime generation may resume an active Work only when
    /// the latest durable member action proves that the previous transport
    /// disconnected. This is separate from ordinary idle continuation so
    /// deterministic recovery tests can retain their bounded idle grace.
    pub(super) fn complete_work_delivery(
        &self,
        claimed: &ClaimedWork,
        receipt: &str,
    ) -> CliResult<()> {
        let claim_id = claimed.delivery.claim_id.as_deref().ok_or_else(|| {
            CliError::Usage(format!(
                "WorkDelivery {} has no durable claim",
                claimed.delivery.id
            ))
        })?;
        let session = self
            .store
            .fabric_agent_sessions(&claimed.execution_space_id)?
            .into_iter()
            .find(|session| {
                session.id == claimed.delivery.recipient_session_id
                    && session.runtime_generation == claimed.delivery.recipient_session_generation
            })
            .ok_or_else(|| CliError::Usage("AGENT_SESSION_GENERATION_FENCED".into()))?;
        let daemon_generation = claimed
            .delivery
            .claimed_node_daemon_generation
            .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED".into()))?;
        self.store.record_work_provider_receipt(
            &canonical_delivery_context(
                &claimed.execution_space_id,
                &session.node_daemon_id,
                "node_daemon.work_delivery.provider_received",
                format!("{claim_id}:provider-received"),
                0,
            ),
            &claimed.delivery.id,
            &session.node_id,
            &session.node_daemon_id,
            daemon_generation,
            claim_id,
            receipt,
            &now_string(),
        )?;
        Ok(())
    }

    /// A provider transport error returned before native acceptance is a
    /// positive negative-acknowledgement for every Work claim still owned by
    /// this member and Supervisor generation. Mark those attempts failed so
    /// the replacement transport can claim a new attempt. Claims that already
    /// carry a provider receipt are never selected and therefore never replayed.
    pub(super) fn fail_unreceived_work_claims_for(
        &self,
        member_id: &str,
        reason: &str,
    ) -> CliResult<()> {
        self.require_supervisor_lease()?;
        if let Some(execution_space_id) = self.store.trust_member_run_scope(member_id)? {
            let member = self
                .latest_member_run(member_id)?
                .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?;
            let sessions = self
                .store
                .fabric_agent_sessions(&execution_space_id)?
                .into_iter()
                .filter(|session| {
                    session.agent_member_id == member.agent_member_id
                        && session.lifecycle
                            != harness_core::agentfirm_api::AgentSessionStatus::Closed
                })
                .collect::<Vec<_>>();
            if sessions.len() != 1 {
                return Err(CliError::Usage(format!(
                    "AGENT_SESSION_AMBIGUOUS: Work negative acknowledgement for {} found {} current sessions",
                    member.agent_member_id,
                    sessions.len()
                )));
            }
            let session = sessions.into_iter().next().expect("one session");
            let daemon = self
                .store
                .latest_node_daemon_lease(&session.node_id)?
                .filter(|lease| {
                    lease.daemon_id == session.node_daemon_id
                        && lease.generation == session.node_daemon_generation
                        && lease.status == NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > current_unix_ms_u64()
                })
                .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED".into()))?;
            for delivery in self
                .store
                .fabric_work_deliveries(&execution_space_id)?
                .into_iter()
                .filter(|delivery| {
                    delivery.recipient_agent_member_id == member.agent_member_id
                        && delivery.recipient_session_id == session.id
                        && delivery.recipient_session_generation == session.runtime_generation
                        && delivery.status
                            == harness_core::agentfirm_api::WorkDeliveryStatus::Claimed
                        && delivery.claimed_node_daemon_generation == Some(daemon.generation)
                        && delivery.provider_receipt_id.is_none()
                })
            {
                let claim_id = delivery.claim_id.as_deref().ok_or_else(|| {
                    CliError::Usage(format!("WORK_DELIVERY_CLAIM_ID_MISSING: {}", delivery.id))
                })?;
                self.store.record_work_provider_failure(
                    &canonical_delivery_context(
                        &execution_space_id,
                        &daemon.daemon_id,
                        "node_daemon.work_delivery.negative_ack",
                        format!("{}:negative-ack", delivery.id),
                        0,
                    ),
                    &delivery.id,
                    &session.node_id,
                    &daemon.daemon_id,
                    daemon.generation,
                    claim_id,
                    &format!("provider-negative-ack:{reason}"),
                    &now_string(),
                )?;
            }
        }
        Ok(())
    }

    /// Fail every TeamMessageProjection delivery for `member_id` that is still
    /// `Queued` or `Claimed`.
    ///
    /// Pre-bind failures never claim a message, so `Queued` deliveries
    /// transition directly to `Failed`. Post-bind transport disconnects may
    /// have `Claimed` deliveries; those are only failed when the claim
    /// belongs to the current Supervisor generation.
    pub(super) fn fail_team_messages_for(&self, member_id: &str, reason: &str) -> CliResult<()> {
        self.require_supervisor_lease()?;
        let member = self
            .latest_member_run(member_id)?
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?;
        let run = latest_team_run(&self.store, &self.run_id)?;
        let execution_space_id = team_run_execution_space_id(&self.store, &run)?;
        let sessions = self
            .store
            .fabric_agent_sessions(&execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_member_id == member.agent_member_id
                    && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
            })
            .collect::<Vec<_>>();
        if sessions.len() != 1 {
            return Err(CliError::Usage(format!(
                "AGENT_SESSION_AMBIGUOUS: message recovery for {} found {} current sessions in TeamRun Execution Space {}",
                member.agent_member_id,
                sessions.len(),
                execution_space_id
            )));
        }
        let session = sessions.into_iter().next().expect("one session");
        let daemon = self
            .store
            .latest_node_daemon_lease(&session.node_id)?
            .filter(|lease| {
                lease.daemon_id == session.node_daemon_id
                    && lease.generation == session.node_daemon_generation
                    && lease.status == NodeDaemonLeaseStatus::Active
                    && lease.expires_unix_ms > current_unix_ms_u64()
            })
            .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED".into()))?;
        for delivery in self
            .store
            .fabric_message_deliveries(&execution_space_id)?
            .into_iter()
            .filter(|delivery| {
                delivery.recipient_agent_member_id.as_deref()
                    == Some(member.agent_member_id.as_str())
                    && delivery.status
                        == harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Claimed
                    && delivery.claimed_node_daemon_generation == Some(daemon.generation)
                    && delivery.provider_receipt_id.is_none()
            })
        {
            self.store.reconcile_canonical_message_delivery(
                &canonical_delivery_context(
                    &execution_space_id,
                    &daemon.daemon_id,
                    "node_daemon.message_delivery.negative_ack",
                    format!("{}:negative-ack", delivery.id),
                    delivery.version,
                ),
                &delivery.id,
                &session.node_id,
                &daemon.daemon_id,
                daemon.generation,
                harness_core::agentfirm_api::DeliveryReconcileOutcome::RetrySafeFailure,
                &format!("provider-negative-ack:{reason}"),
                &now_string(),
            )?;
        }
        Ok(())
    }

    /// Messages with a still-queued delivery to `member_id` (excluding the
    /// member's own sends, which it obviously already "has").
    pub(super) fn queued_messages_for(
        &self,
        member_id: &str,
    ) -> CliResult<Vec<TeamMessageProjection>> {
        Ok(self
            .canonical_team_messages()?
            .into_iter()
            .filter(|message| message.sender_runtime_id != member_id)
            .filter(|message| {
                message.deliveries.iter().any(|delivery| {
                    delivery.member_id == member_id
                        && delivery.policy != TeamDeliveryPolicy::Inject
                        && delivery.status == TeamDeliveryStatus::Queued
                })
            })
            .collect())
    }

    // Historical TeamRun delivery completion seam. Canonical
    // MessageDelivery claim/receipt/ack is the only executable path.
    #[cfg(any())]
    pub(super) fn complete_provider_interaction_response(
        &self,
        message: &TeamMessageProjection,
        member_id: &str,
        provider_receipt_id: &str,
    ) -> CliResult<()> {
        self.require_supervisor_lease()?;
        let delivery = message
            .deliveries
            .iter()
            .find(|delivery| delivery.member_id == member_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "provider interaction response {} has no delivery for {member_id}",
                    message.id
                ))
            })?;
        let claim_id = delivery.claim_id.as_deref().ok_or_else(|| {
            CliError::Usage(format!(
                "provider interaction response {} has no durable claim",
                message.id
            ))
        })?;
        store_conflict_as_usage(self.store.complete_team_message_delivery_claim(
            &self.run_id,
            &message.id,
            member_id,
            &self.supervisor_id,
            self.supervisor_generation,
            claim_id,
            provider_receipt_id,
            current_unix_ms_u64(),
            &now_string(),
        ))?;
        Ok(())
    }

    /// Claim queued terminal-work notifications for an idle member.
    ///
    /// When a Work the member owns reaches a terminal status (Done or
    /// Cancelled), the store may hold a queued compatibility delivery for that
    /// transition. This method claims those notification deliveries and
    /// converts each into an informational [`TeamMessageProjection`] from the Host
    /// so the member sees the transition as mail rather than as a new
    /// work assignment.
    ///
    /// Only terminal Work belonging to the stable AgentMember responsibility
    /// is eligible — this is a notification, not a
    /// handoff. No slot-occupancy fence is applied because a terminal-work
    /// notification never blocks an active execution assignment.
    pub(super) fn claim_terminal_work_notifications_for(
        &self,
        _member_id: &str,
    ) -> CliResult<Vec<TeamMessageProjection>> {
        Ok(Vec::new())
    }

    /// Claim queued mail for an idle member only when at least one queued
    /// message requires a response round (ADR 0046 §4). When a round is
    /// triggered, every queued message — including informational mail — is
    /// claimed in order so the whole batch is delivered exactly once with
    /// that round's provider receipt. Informational-only mail stays queued
    /// and durable without starting a round, which bounds peer convergence.
    pub(super) fn claim_canonical_round_messages_for(
        &self,
        member_id: &str,
    ) -> CliResult<Vec<TeamMessageProjection>> {
        let member = self
            .latest_member_run(member_id)?
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?;
        claim_canonical_messages_for_member(self, &member)
    }
}
