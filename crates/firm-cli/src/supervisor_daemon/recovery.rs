use super::*;

const SUPERVISOR_RECOVERY_REQUIRED: &str = "team_supervisor_recovery_required";
const SUPERVISOR_RECOVERED: &str = "team_supervisor_recovered";
/// One adoption of this TeamRun ended without changing any canonical state.
/// Unlike `SUPERVISOR_RECOVERY_REQUIRED` this is not a diagnosis of runtime
/// damage: it is a bounded statement that repeating the same adoption against
/// the same observed canonical state cannot produce a different result.
const SUPERVISOR_NO_PROGRESS: &str = "team_supervisor_no_progress";

/// Why automatic adoption of one TeamRun is currently held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SupervisorAdoptionHold {
    /// Nothing holds adoption.
    None,
    /// A Supervisor generation ended in a state that needs an explicit
    /// operator recovery or start intent regardless of canonical change.
    RecoveryRequired,
    /// The last adoption produced no canonical progress at this exact
    /// fingerprint. Adoption resumes as soon as canonical state differs.
    NoProgressAt(String),
}

fn supervisor_recovery_actions(
    store: &HarnessStore,
    run_id: &str,
) -> CliResult<Vec<harness_core::MemberAction>> {
    Ok(store
        .member_actions()?
        .into_iter()
        .filter(|action| {
            action.team_run_id == run_id
                && matches!(
                    action.action_type.as_str(),
                    SUPERVISOR_RECOVERY_REQUIRED | SUPERVISOR_RECOVERED | SUPERVISOR_NO_PROGRESS
                )
        })
        .collect())
}

/// Read the durable adoption outcome for one TeamRun as a hold.
///
/// This deliberately does not read only the newest row. `seq` is assigned per
/// ledger instance, so two concurrent writers can tie, and a weaker
/// `SUPERVISOR_NO_PROGRESS` row must never shadow a hard
/// `SUPERVISOR_RECOVERY_REQUIRED` diagnosis that no explicit recovery has
/// settled. The rule is therefore: any recovery requirement newer than the
/// last explicit recovery wins outright; otherwise the newest unsettled
/// no-progress observation applies. An exact tie resolves toward
/// `SUPERVISOR_RECOVERED` so an operator can always clear a hold, and toward
/// `SUPERVISOR_RECOVERY_REQUIRED` over `SUPERVISOR_NO_PROGRESS` so the
/// stronger diagnosis is never lost.
fn adoption_hold(actions: &[harness_core::MemberAction]) -> SupervisorAdoptionHold {
    let newest = |action_type: &str, status: harness_core::MemberActionStatus| {
        actions
            .iter()
            .filter(|action| action.action_type == action_type && action.status == status)
            .max_by_key(|action| action.seq)
    };
    let recovered_seq = newest(
        SUPERVISOR_RECOVERED,
        harness_core::MemberActionStatus::Succeeded,
    )
    .map(|action| action.seq);
    let unsettled = |action: &harness_core::MemberAction| {
        recovered_seq.is_none_or(|recovered| action.seq > recovered)
    };

    if newest(
        SUPERVISOR_RECOVERY_REQUIRED,
        harness_core::MemberActionStatus::Failed,
    )
    .is_some_and(unsettled)
    {
        return SupervisorAdoptionHold::RecoveryRequired;
    }
    match newest(
        SUPERVISOR_NO_PROGRESS,
        harness_core::MemberActionStatus::Failed,
    )
    .filter(|action| unsettled(action))
    {
        None => SupervisorAdoptionHold::None,
        Some(action) => match crate::canonical_state_from_evidence(&action.evidence_refs) {
            Some(fingerprint) => SupervisorAdoptionHold::NoProgressAt(fingerprint.to_string()),
            // A no-progress marker without its canonical-state binding cannot
            // prove what it was observed under. Fail closed: hold adoption
            // until an explicit recovery or start intent clears it.
            None => SupervisorAdoptionHold::RecoveryRequired,
        },
    }
}

/// Whether any durable hold currently stands, of either kind.
fn adoption_hold_stands(actions: &[harness_core::MemberAction]) -> bool {
    !matches!(adoption_hold(actions), SupervisorAdoptionHold::None)
}

/// Codes that describe this daemon generation or a lost race, never a durable
/// property of the TeamRun. They are matched as the error's own leading code
/// token, never as a substring of the whole chain: an error that merely quotes
/// a fenced code while reporting something structural must still hold.
const TRANSIENT_START_FAILURE_CODES: &[&str] = &[
    "NODE_HAS_NO_REGISTERED_PROJECT",
    "NODE_DAEMON_GENERATION_FENCED",
    "NODE_DAEMON_MACHINE_AUTHORITY_LOST",
    "SUPERVISOR_GENERATION_FENCED",
];

/// The two capacity/ownership rejections `start_supervising` writes as prose
/// rather than as a code. Matched as a prefix for the same reason.
const TRANSIENT_START_FAILURE_PREFIXES: &[&str] =
    &["NodeDaemon at capacity", "NodeDaemon already manages"];

/// The leading `CODE:` token of a message, when it has one.
fn leading_error_code(message: &str) -> Option<&str> {
    let code = message.split(':').next()?.trim();
    (!code.is_empty()
        && code.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        }))
    .then_some(code)
}

/// Start failures that say nothing durable about this TeamRun. Holding
/// adoption on them would suppress a run that a freed concurrency slot, a won
/// CAS retry or a restored daemon generation makes startable again with no
/// canonical change at all.
///
/// Classification is typed first. `CliError::Usage` is the catch-all this
/// codebase flattens Store conflicts into, so its text is consulted only for
/// the error's own leading code token: matching a substring anywhere in the
/// chain let an ordinary CAS conflict — or any error that merely mentioned a
/// fenced code — decide the outcome for a whole TeamRun.
fn start_failure_is_transient(error: &CliError) -> bool {
    match error {
        // Store contention, provider-admission contention and a lost
        // machine/Supervisor authority are properties of this attempt, never
        // of the run.
        CliError::Store(_)
        | CliError::ProviderAdmissionContention(_)
        | CliError::SupervisorLeaseLost(_)
        | CliError::ProviderProcessAdmissionClosed(_) => true,
        // Defensive, and not currently reachable from `start_supervising`:
        // that path stops at the durable Supervisor registration and the
        // Planning→Running lifecycle CAS, before any provider effect is
        // prepared, so neither variant can be the start failure being
        // classified. Should a future start step prepare an effect, an
        // accepted or ambiguous one is emphatically not a no-progress
        // observation — the unresolved-RuntimeCommand branch above already
        // owns it and writes the stronger recovery-required marker — so
        // silently converting it into a weak canonical-state hold would lose
        // that diagnosis. Kept explicit rather than left to the `_` arm.
        CliError::ProviderEffectAccepted(_) | CliError::RuntimeRecoveryRequired(_) => true,
        CliError::Usage(message) => {
            TRANSIENT_START_FAILURE_PREFIXES
                .iter()
                .any(|prefix| message.starts_with(prefix))
                || leading_error_code(message)
                    .is_some_and(|code| TRANSIENT_START_FAILURE_CODES.contains(&code))
        }
        _ => false,
    }
}

pub(super) fn team_run_has_unresolved_runtime_command(
    store: &HarnessStore,
    execution_space_id: &str,
    run_id: &str,
    node_daemon_id: &str,
    node_daemon_generation: u64,
    supervisor_id: &str,
    supervisor_generation: u64,
) -> CliResult<bool> {
    let members = crate::latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == run_id)
        .map(|member| (member.id, member.runtime_generation))
        .collect::<HashMap<_, _>>();
    Ok(store
        .runtime_commands(execution_space_id)?
        .into_iter()
        .any(|command| {
            runtime_command_matches_supervisor_authority(
                &command,
                run_id,
                node_daemon_id,
                node_daemon_generation,
                supervisor_id,
                supervisor_generation,
            ) && command
                .binding
                .target_member_run_id
                .as_ref()
                .and_then(|member_id| {
                    members
                        .get(member_id)
                        .map(|generation| (member_id, generation))
                })
                .is_some_and(|(_, generation)| {
                    command.binding.target_member_run_generation == Some(*generation)
                        && command.status
                            == harness_core::agentfirm_api::RuntimeCommandStatus::Accepted
                        && command.effect_certainty
                            == harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown
                        && command.postcondition_status
                            == harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown
                })
        }))
}

fn runtime_command_matches_supervisor_authority(
    command: &harness_core::agentfirm_api::RuntimeCommandRecord,
    run_id: &str,
    node_daemon_id: &str,
    node_daemon_generation: u64,
    supervisor_id: &str,
    supervisor_generation: u64,
) -> bool {
    command.target_node_daemon_id == node_daemon_id
        && command.target_node_daemon_generation == node_daemon_generation
        && command.binding.target_driver
            == harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
                team_run_id: run_id.to_string(),
                team_supervisor_id: supervisor_id.to_string(),
                team_supervisor_generation: supervisor_generation,
            }
}

pub(crate) fn reconcile_team_run_start_postcondition(
    store: &HarnessStore,
    status_json: &str,
    node_id: &str,
    execution_space_id: &str,
    run_id: &str,
) -> Option<CliResult<serde_json::Value>> {
    let status = serde_json::from_str::<serde_json::Value>(status_json).ok()?;
    if status["ok"].as_bool() != Some(true) || status["node_id"].as_str() != Some(node_id) {
        return None;
    }
    let instance_id = status["instance_id"].as_str()?;
    let process_id = status["process_id"].as_u64()?;
    let process_id = u32::try_from(process_id).ok()?;
    let matching_runs = status["runs"]
        .as_array()?
        .iter()
        .filter(|candidate| {
            candidate["execution_space_id"].as_str() == Some(execution_space_id)
                && candidate["run_id"].as_str() == Some(run_id)
                && candidate["status"].as_str() == Some("running")
        })
        .collect::<Vec<_>>();
    let [run] = matching_runs.as_slice() else {
        return None;
    };
    let daemon_generation = run["daemon_generation"].as_u64()?;
    let project_binding_id = run["project_binding_id"].as_str()?;
    let supervisor_id = run["supervisor_id"].as_str()?;
    let supervisor_generation = run["supervisor_generation"].as_u64()?;
    let now = current_unix_ms_u64();
    let daemon = match store.latest_node_daemon_lease(node_id) {
        Ok(Some(daemon)) => daemon,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    let supervisor = match store.latest_team_supervisor_lease(run_id) {
        Ok(Some(supervisor)) => supervisor,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    let exact = start_postcondition_matches(
        &daemon,
        &supervisor,
        node_id,
        instance_id,
        daemon_generation,
        execution_space_id,
        project_binding_id,
        run_id,
        supervisor_id,
        supervisor_generation,
        process_id,
        now,
    );
    exact.then(|| {
        Ok(serde_json::json!({
            "node_id": node_id,
            "execution_space_id": execution_space_id,
            "team_run_id": run_id,
            "daemon_response": {
                "ok": true,
                "reconciled_after_transport_error": true,
                "daemon_generation": daemon_generation,
                "supervisor_id": supervisor_id,
                "supervisor_generation": supervisor_generation,
            },
        }))
    })
}

#[allow(clippy::too_many_arguments)]
fn start_postcondition_matches(
    daemon: &harness_core::NodeDaemonLease,
    supervisor: &harness_core::TeamSupervisorLease,
    node_id: &str,
    instance_id: &str,
    daemon_generation: u64,
    execution_space_id: &str,
    project_binding_id: &str,
    run_id: &str,
    supervisor_id: &str,
    supervisor_generation: u64,
    process_id: u32,
    now: u64,
) -> bool {
    daemon.node_id == node_id
        && daemon.daemon_id == format!("node-daemon:{node_id}")
        && daemon.instance_id == instance_id
        && daemon.generation == daemon_generation
        && daemon.status == harness_core::NodeDaemonLeaseStatus::Active
        && daemon.expires_unix_ms > now
        && supervisor.team_run_id == run_id
        && supervisor.node_id == node_id
        && supervisor.node_daemon_id == daemon.daemon_id
        && supervisor.node_daemon_generation == daemon.generation
        && supervisor.execution_space_id == execution_space_id
        && supervisor.project_binding_id == project_binding_id
        && supervisor.supervisor_id == supervisor_id
        && supervisor.generation == supervisor_generation
        && supervisor.owner_process_id == process_id
        && supervisor.status == harness_core::TeamSupervisorLeaseStatus::Active
        && supervisor.expires_unix_ms > now
}

impl MultiTeamDaemon {
    /// A start that never reached a RuntimeCommand still has to leave a
    /// durable outcome. Without one, a structurally dead historical run
    /// (missing cwd, missing Team, stale permission ceiling, unreleased
    /// AgentSession) is retried on every scan and burns a Supervisor
    /// generation each time (#671).
    pub(super) fn block_start_failure_if_unresolved(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        error: &CliError,
    ) {
        if self.block_start_failure_with_unresolved_runtime_command(
            execution_space_id,
            store,
            run_id,
            error,
        ) {
            return;
        }
        if start_failure_is_transient(error) {
            return;
        }
        self.hold_adoption_without_progress(
            execution_space_id,
            store,
            run_id,
            &format!("TEAM_SUPERVISOR_START_FAILED_BEFORE_RUNTIME_COMMAND: {error}"),
            None,
        );
    }

    /// Returns true when a blocking recovery marker was written (or already
    /// stood) because an unresolved RuntimeCommand exists under this exact
    /// daemon+Supervisor authority.
    fn block_start_failure_with_unresolved_runtime_command(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        error: &CliError,
    ) -> bool {
        let daemon = store
            .latest_node_daemon_lease(&self.node_id)
            .ok()
            .flatten()
            .filter(|lease| {
                lease.daemon_id == self.daemon_id && lease.instance_id == self.instance_id
            });
        let supervisor = store
            .latest_team_supervisor_lease(run_id)
            .ok()
            .flatten()
            .filter(|lease| {
                daemon.as_ref().is_some_and(|daemon| {
                    lease.node_daemon_id == daemon.daemon_id
                        && lease.node_daemon_generation == daemon.generation
                })
            });
        let (Some(daemon), Some(supervisor)) = (daemon, supervisor) else {
            return false;
        };
        match team_run_has_unresolved_runtime_command(
            store,
            execution_space_id,
            run_id,
            &daemon.daemon_id,
            daemon.generation,
            &supervisor.supervisor_id,
            supervisor.generation,
        ) {
            Ok(false) => false,
            Ok(true) => {
                if let Err(marker_error) = self.block_team_run_after_supervisor_failure(
                    execution_space_id,
                    store,
                    run_id,
                    Some(&supervisor.supervisor_id),
                    Some(supervisor.generation),
                    error,
                ) {
                    eprintln!(
                        "[node-daemon] could not persist start-failure recovery marker for {execution_space_id}/{run_id}: {marker_error}"
                    );
                }
                true
            }
            Err(read_error) => {
                eprintln!(
                    "[node-daemon] could not inspect unresolved RuntimeCommands after start failure for {execution_space_id}/{run_id}: {read_error}"
                );
                false
            }
        }
    }

    /// Returns true when a blocking recovery marker was written because the
    /// finished Supervisor left an unresolved RuntimeCommand behind.
    pub(super) fn block_finished_supervisor_if_unresolved(
        &self,
        context: &MultiTeamContext,
    ) -> bool {
        let store = match self.store_for_space(&context.execution_space_id) {
            Ok(store) => store,
            Err(error) => {
                self.block_finished_supervisor_volatile(context, &error);
                return true;
            }
        };
        match team_run_has_unresolved_runtime_command(
            &store,
            &context.execution_space_id,
            &context.run_id,
            &self.daemon_id,
            context.daemon_generation,
            &context.supervisor_id,
            context.supervisor_generation,
        ) {
            Ok(false) => false,
            Ok(true) => {
                self.block_finished_supervisor(
                    context,
                    &store,
                    &CliError::Usage(
                        "TEAM_SUPERVISOR_EXITED_WITH_UNRESOLVED_RUNTIME_COMMAND".into(),
                    ),
                );
                true
            }
            Err(error) => {
                self.block_finished_supervisor_volatile(context, &error);
                true
            }
        }
    }

    /// Record that one adoption produced no canonical progress, bound to the
    /// exact canonical state it observed. `observed_state` is supplied by the
    /// Supervisor that measured it; when absent the state is re-observed here
    /// (the start-failure path never got far enough to measure one).
    pub(super) fn hold_adoption_without_progress(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        detail: &str,
        observed_state: Option<&str>,
    ) {
        let canonical_state = match observed_state {
            Some(state) => Ok(state.to_string()),
            None => {
                crate::team_run_canonical_state_fingerprint(store, Some(execution_space_id), run_id)
            }
        };
        let canonical_state = match canonical_state {
            Ok(canonical_state) => canonical_state,
            Err(error) => {
                // Nothing can prove this run has since changed, so the hold
                // cannot be state-keyed. Only an explicit start clears it.
                self.insert_volatile_hold(
                    execution_space_id,
                    run_id,
                    VolatileAdoptionHold::Unconditional,
                    &error,
                );
                return;
            }
        };
        if let Err(error) =
            self.record_no_progress_adoption_outcome(store, run_id, &canonical_state, detail)
        {
            // Without a durable outcome the next scan would adopt this run
            // again immediately. Hold it in-process instead — but keyed to the
            // same canonical state, so a legacy run with no Host MemberRun to
            // project a marker onto is not stranded for this daemon's whole
            // lifetime.
            self.insert_volatile_hold(
                execution_space_id,
                run_id,
                VolatileAdoptionHold::AtCanonicalState(canonical_state),
                &error,
            );
        }
    }

    fn insert_volatile_hold(
        &self,
        execution_space_id: &str,
        run_id: &str,
        hold: VolatileAdoptionHold,
        error: &CliError,
    ) {
        let liftable = matches!(hold, VolatileAdoptionHold::AtCanonicalState(_));
        self.recovery_blocked_runs
            .lock()
            .unwrap_or_else(|lock_error| lock_error.into_inner())
            .insert((execution_space_id.to_string(), run_id.to_string()), hold);
        eprintln!(
            "[node-daemon] volatile adoption hold ({}) for {execution_space_id}/{run_id} because its durable no-progress outcome could not be written: {error}",
            if liftable {
                "lifted by any canonical change"
            } else {
                "explicit start required"
            }
        );
    }

    fn record_no_progress_adoption_outcome(
        &self,
        store: &HarnessStore,
        run_id: &str,
        canonical_state: &str,
        detail: &str,
    ) -> CliResult<()> {
        match adoption_hold(&supervisor_recovery_actions(store, run_id)?) {
            // A hard recovery diagnosis outranks a no-progress observation.
            SupervisorAdoptionHold::RecoveryRequired => return Ok(()),
            // The same fingerprint is already held; appending again would only
            // grow the journal.
            SupervisorAdoptionHold::NoProgressAt(held) if held == canonical_state => return Ok(()),
            _ => {}
        }
        let host = crate::latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.team_run_id == run_id && member.role == "host")
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "TEAM_RUN_HOST_NOT_FOUND: cannot project Supervisor adoption outcome for {run_id}"
                ))
            })?;
        TeamRunLedger::without_supervisor(store, run_id).append_action_with_provider_status(
            &host.id,
            SUPERVISOR_NO_PROGRESS,
            harness_core::MemberActionStatus::Failed,
            "TeamSupervisor adoption produced no canonical progress",
            &format!(
                "{detail}. Automatic adoption is held until this TeamRun's canonical TeamRun, MemberRun, Work, Message or RuntimeCommand state changes, or an explicit operator recovery or start intent arrives."
            ),
            None,
            &[crate::canonical_state_evidence_ref(canonical_state)],
        )?;
        Ok(())
    }

    pub(super) fn block_finished_supervisor_failure(
        &self,
        context: &MultiTeamContext,
        error: &CliError,
    ) {
        match self.store_for_space(&context.execution_space_id) {
            Ok(store) => self.block_finished_supervisor(context, &store, error),
            Err(store_error) => self.block_finished_supervisor_volatile(context, &store_error),
        }
    }

    fn block_finished_supervisor(
        &self,
        context: &MultiTeamContext,
        store: &HarnessStore,
        error: &CliError,
    ) {
        if let Err(marker_error) = self.block_team_run_after_supervisor_failure(
            &context.execution_space_id,
            store,
            &context.run_id,
            Some(&context.supervisor_id),
            Some(context.supervisor_generation),
            error,
        ) {
            eprintln!(
                "[node-daemon] could not persist finished-Supervisor recovery marker for {}/{}: {marker_error}",
                context.execution_space_id, context.run_id
            );
        }
    }

    fn block_finished_supervisor_volatile(&self, context: &MultiTeamContext, error: &CliError) {
        // The Store itself is unreadable here, so there is no canonical state
        // to key the hold to. This one is deliberately unconditional.
        self.recovery_blocked_runs
            .lock()
            .unwrap_or_else(|lock_error| lock_error.into_inner())
            .insert(
                (context.execution_space_id.clone(), context.run_id.clone()),
                VolatileAdoptionHold::Unconditional,
            );
        eprintln!(
            "[node-daemon] volatile Supervisor recovery block for {}/{} because its durable projection is unavailable: {error}",
            context.execution_space_id, context.run_id
        );
    }

    pub(super) fn store_for_space(&self, execution_space_id: &str) -> CliResult<HarnessStore> {
        let space = crate::execution_space::context_for_id(&self.firm_home, execution_space_id)
            .map_err(|error| CliError::Usage(error.to_string()))?
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "Execution Space not found while reconciling Supervisor: {execution_space_id}"
                ))
            })?;
        Ok(HarnessStore::new(space.store_root))
    }

    /// Whether automatic adoption of this TeamRun is currently held.
    ///
    /// A `NoProgressAt` hold is honoured only while the canonical state still
    /// matches the fingerprint it was written under; any canonical change (new
    /// Work, new Message, a Reopen, a RuntimeCommand) re-enables adoption with
    /// no operator action at all.
    pub(super) fn team_run_adoption_is_held(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
    ) -> CliResult<bool> {
        // A run whose dead generation is still writing its outcome must not be
        // adopted; that marker would otherwise land on the live successor.
        if self
            .settling_runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(&(execution_space_id.to_string(), run_id.to_string()))
        {
            return Ok(true);
        }
        let volatile = self
            .recovery_blocked_runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(execution_space_id.to_string(), run_id.to_string()))
            .cloned();
        let hold = match volatile {
            Some(VolatileAdoptionHold::Unconditional) => return Ok(true),
            Some(VolatileAdoptionHold::AtCanonicalState(fingerprint)) => {
                SupervisorAdoptionHold::NoProgressAt(fingerprint)
            }
            None => adoption_hold(&supervisor_recovery_actions(store, run_id)?),
        };
        match hold {
            SupervisorAdoptionHold::None => Ok(false),
            SupervisorAdoptionHold::RecoveryRequired => Ok(true),
            SupervisorAdoptionHold::NoProgressAt(held) => Ok(held
                == crate::team_run_canonical_state_fingerprint(
                    store,
                    Some(execution_space_id),
                    run_id,
                )?),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn block_team_run_after_supervisor_failure(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        supervisor_id: Option<&str>,
        supervisor_generation: Option<u64>,
        error: &CliError,
    ) -> CliResult<()> {
        // A hard diagnosis is never state-liftable.
        self.recovery_blocked_runs
            .lock()
            .unwrap_or_else(|lock_error| lock_error.into_inner())
            .insert(
                (execution_space_id.to_string(), run_id.to_string()),
                VolatileAdoptionHold::Unconditional,
            );
        // A no-progress hold is a weaker statement than a recovery diagnosis,
        // so it must not suppress this marker; only an existing recovery
        // requirement already says everything this one would.
        if matches!(
            adoption_hold(&supervisor_recovery_actions(store, run_id)?),
            SupervisorAdoptionHold::RecoveryRequired
        ) {
            return Ok(());
        }
        let host = crate::latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.team_run_id == run_id && member.role == "host")
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "TEAM_RUN_HOST_NOT_FOUND: cannot project Supervisor recovery for {run_id}"
                ))
            })?;
        let failure_code = if error.is_supervisor_lease_lost() {
            "TEAM_SUPERVISOR_LEASE_LOST"
        } else if error.to_string().contains("PANICKED") {
            "TEAM_SUPERVISOR_PANICKED"
        } else if error
            .to_string()
            .contains("EXITED_WITH_UNRESOLVED_RUNTIME_COMMAND")
        {
            "TEAM_SUPERVISOR_EXITED_WITH_UNRESOLVED_RUNTIME_COMMAND"
        } else {
            "TEAM_SUPERVISOR_START_OR_RUNTIME_FAILED"
        };
        let authority = match (supervisor_id, supervisor_generation) {
            (Some(id), Some(generation)) => format!("{id} generation {generation}"),
            _ => "latest attempted Supervisor generation".to_string(),
        };
        TeamRunLedger::without_supervisor(store, run_id).append_action(
            &host.id,
            SUPERVISOR_RECOVERY_REQUIRED,
            harness_core::MemberActionStatus::Failed,
            "TeamSupervisor recovery required",
            &format!(
                "{failure_code}: {authority} stopped before a safe runnable postcondition; automatic adoption is blocked. Inspect the canonical RuntimeCommand inventory, then issue explicit operator recovery or a new start intent."
            ),
        )?;
        Ok(())
    }

    pub(super) fn clear_team_run_supervisor_recovery(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
    ) -> CliResult<()> {
        self.recovery_blocked_runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&(execution_space_id.to_string(), run_id.to_string()));
        if !adoption_hold_stands(&supervisor_recovery_actions(store, run_id)?) {
            return Ok(());
        }
        let host = crate::latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.team_run_id == run_id && member.role == "host")
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "TEAM_RUN_HOST_NOT_FOUND: cannot settle Supervisor recovery for {run_id}"
                ))
            })?;
        TeamRunLedger::without_supervisor(store, run_id).append_action(
            &host.id,
            SUPERVISOR_RECOVERED,
            harness_core::MemberActionStatus::Succeeded,
            "TeamSupervisor explicitly recovered",
            &format!(
                "Explicit start established {supervisor_id} generation {supervisor_generation}; automatic adoption may resume after this generation ends safely."
            ),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(
        seq: u64,
        action_type: &str,
        status: harness_core::MemberActionStatus,
        evidence_refs: Vec<String>,
    ) -> harness_core::MemberAction {
        harness_core::MemberAction {
            id: format!("action-{seq}"),
            seq,
            team_run_id: "run".into(),
            member_run_id: "host".into(),
            task_id: None,
            provider_call_id: None,
            action_type: action_type.into(),
            status,
            provider_status: None,
            semantic_status: None,
            title: "Supervisor".into(),
            summary: "bounded coordination diagnostic".into(),
            evidence_refs,
            started_at: "unix-ms:1".into(),
            completed_at: Some("unix-ms:1".into()),
        }
    }

    fn required(seq: u64) -> harness_core::MemberAction {
        marker(
            seq,
            SUPERVISOR_RECOVERY_REQUIRED,
            harness_core::MemberActionStatus::Failed,
            Vec::new(),
        )
    }

    fn recovered(seq: u64) -> harness_core::MemberAction {
        marker(
            seq,
            SUPERVISOR_RECOVERED,
            harness_core::MemberActionStatus::Succeeded,
            Vec::new(),
        )
    }

    fn no_progress(seq: u64, evidence: Vec<String>) -> harness_core::MemberAction {
        marker(
            seq,
            SUPERVISOR_NO_PROGRESS,
            harness_core::MemberActionStatus::Failed,
            evidence,
        )
    }

    #[test]
    fn only_unsettled_supervisor_recovery_markers_block_adoption() {
        assert!(adoption_hold_stands(&[required(1)]));
        assert_eq!(
            adoption_hold(&[required(1)]),
            SupervisorAdoptionHold::RecoveryRequired
        );
        assert!(!adoption_hold_stands(&[required(1), recovered(2)]));
        assert_eq!(
            adoption_hold(&[required(1), recovered(2)]),
            SupervisorAdoptionHold::None
        );
        assert!(!adoption_hold_stands(&[]));
        assert_eq!(adoption_hold(&[]), SupervisorAdoptionHold::None);
    }

    #[test]
    fn a_no_progress_row_never_shadows_an_unsettled_recovery_requirement() {
        let state = crate::canonical_state_evidence_ref("sha256:one");
        // Newer by seq, and even tied with it: the weaker observation must
        // never hide the hard diagnosis while no recovery has settled it.
        assert_eq!(
            adoption_hold(&[required(4), no_progress(9, vec![state.clone()])]),
            SupervisorAdoptionHold::RecoveryRequired
        );
        assert_eq!(
            adoption_hold(&[required(4), no_progress(4, vec![state.clone()])]),
            SupervisorAdoptionHold::RecoveryRequired
        );
        // Once an explicit recovery settles it, the later no-progress
        // observation is the one that applies.
        assert_eq!(
            adoption_hold(&[
                required(4),
                recovered(5),
                no_progress(6, vec![state.clone()]),
            ]),
            SupervisorAdoptionHold::NoProgressAt("sha256:one".into())
        );
        // An explicit recovery tied with the requirement still clears it, so
        // an operator can never be wedged by a seq collision.
        assert_eq!(
            adoption_hold(&[required(4), recovered(4)]),
            SupervisorAdoptionHold::None
        );
    }

    #[test]
    fn no_progress_hold_is_keyed_to_the_canonical_state_it_observed() {
        assert_eq!(
            adoption_hold(&[no_progress(
                1,
                vec![crate::canonical_state_evidence_ref("sha256:one")]
            )]),
            SupervisorAdoptionHold::NoProgressAt("sha256:one".into()),
            "the hold must name the exact canonical state it was observed under"
        );
        assert_eq!(
            adoption_hold(&[no_progress(2, Vec::new())]),
            SupervisorAdoptionHold::RecoveryRequired,
            "a no-progress marker that cannot prove its canonical state must fail closed"
        );
    }

    #[test]
    fn only_transient_start_failures_skip_a_durable_adoption_hold() {
        for transient in [
            "NodeDaemon at capacity (4/4 runs); cannot start space/run",
            "NodeDaemon already manages space/run",
            "NodeDaemon already manages space/run (settling the previous Supervisor generation; retry)",
            "NODE_HAS_NO_REGISTERED_PROJECT: Node n has no active project in Execution Space s",
            "NODE_DAEMON_GENERATION_FENCED: current lease is missing",
            "NODE_DAEMON_MACHINE_AUTHORITY_LOST: space s lost this instance lease",
            "SUPERVISOR_GENERATION_FENCED: another generation owns this run",
        ] {
            assert!(
                start_failure_is_transient(&CliError::Usage(transient.into())),
                "{transient} must not hold adoption"
            );
        }
        for structural in [
            "team run r is pinned to unavailable Project Binding p",
            "REMOTE_TEAM_RUN_NOT_ADOPTED: TeamRun r belongs to Node other",
            "TEAM_RUN_DORMANT: TeamRun r has no Active MemberRun",
            "member workspace canonical_root no longer exists",
            "AgentSession for member m was never released",
        ] {
            assert!(
                !start_failure_is_transient(&CliError::Usage(structural.into())),
                "{structural} must leave a durable adoption outcome"
            );
        }
    }

    #[test]
    fn start_failure_classification_is_typed_not_substring_matched() {
        // A CAS conflict is an ordinary lost race. It used to arrive as
        // `CliError::Usage` carrying the Store's conflict text, be classified
        // structural, and wedge a healthy run until canonical state changed.
        assert!(start_failure_is_transient(&CliError::Store(
            harness_store::StoreError::Conflict(
                "TEAM_RUN_LIFECYCLE_CONFLICT: expected version 3".into()
            )
        )));
        assert!(start_failure_is_transient(&CliError::SupervisorLeaseLost(
            "supervisor lease lost".into()
        )));
        assert!(start_failure_is_transient(
            &CliError::RuntimeRecoveryRequired("ambiguous provider effect".into())
        ));

        // A structural failure that merely quotes a fenced code somewhere in
        // its chain must still hold: only the error's own leading code counts.
        assert!(!start_failure_is_transient(&CliError::Usage(
            "member workspace canonical_root is gone (previous attempt reported SUPERVISOR_GENERATION_FENCED)"
                .into()
        )));
        assert!(!start_failure_is_transient(&CliError::Usage(
            "TEAM_RUN_HOST_NOT_FOUND: NODE_DAEMON_GENERATION_FENCED appears only in this detail"
                .into()
        )));
        assert!(!start_failure_is_transient(&CliError::Io(
            std::io::Error::new(std::io::ErrorKind::NotFound, "provider cwd is missing")
        )));
    }

    #[test]
    fn leading_error_code_reads_only_a_real_leading_code_token() {
        assert_eq!(
            leading_error_code("NODE_DAEMON_GENERATION_FENCED: detail"),
            Some("NODE_DAEMON_GENERATION_FENCED")
        );
        assert_eq!(leading_error_code("CODE_9: detail"), Some("CODE_9"));
        assert_eq!(leading_error_code("team run r is pinned"), None);
        assert_eq!(
            leading_error_code("store error: SUPERVISOR_GENERATION_FENCED"),
            None
        );
        assert_eq!(leading_error_code(""), None);
    }

    #[test]
    fn start_reconciliation_requires_one_exact_live_daemon_and_supervisor_generation() {
        let daemon = harness_core::NodeDaemonLease {
            node_id: "node".into(),
            daemon_id: "node-daemon:node".into(),
            generation: 7,
            instance_id: "instance".into(),
            status: harness_core::NodeDaemonLeaseStatus::Active,
            acquired_unix_ms: 10,
            renewed_unix_ms: 20,
            expires_unix_ms: 100,
            released_unix_ms: None,
        };
        let supervisor = harness_core::TeamSupervisorLease {
            team_run_id: "run".into(),
            node_id: "node".into(),
            node_daemon_id: daemon.daemon_id.clone(),
            node_daemon_generation: daemon.generation,
            execution_space_id: "space".into(),
            project_binding_id: "project".into(),
            supervisor_id: "supervisor".into(),
            generation: 3,
            owner_process_id: 42,
            owner_locator: "test://supervisor".into(),
            status: harness_core::TeamSupervisorLeaseStatus::Active,
            acquired_unix_ms: 10,
            heartbeat_unix_ms: 20,
            expires_unix_ms: 100,
            released_unix_ms: None,
        };
        assert!(start_postcondition_matches(
            &daemon,
            &supervisor,
            "node",
            "instance",
            7,
            "space",
            "project",
            "run",
            "supervisor",
            3,
            42,
            50,
        ));
        assert!(!start_postcondition_matches(
            &daemon,
            &supervisor,
            "node",
            "instance",
            8,
            "space",
            "project",
            "run",
            "supervisor",
            3,
            42,
            50,
        ));
        assert!(!start_postcondition_matches(
            &daemon,
            &supervisor,
            "node",
            "instance",
            7,
            "space",
            "project",
            "run",
            "supervisor",
            3,
            43,
            50,
        ));
    }

    #[test]
    fn old_daemon_or_supervisor_runtime_command_cannot_authorize_recovery_block() {
        let mut command: harness_core::agentfirm_api::RuntimeCommandRecord =
            serde_json::from_value(serde_json::json!({
                "id": "runtime-command-old-authority",
                "execution_space_id": "space",
                "target_node_id": "node",
                "target_node_daemon_id": "node-daemon:node",
                "target_node_daemon_generation": 7,
                "authenticated_actor": {"kind": "service", "id": "node-daemon:node"},
                "command": "start_cycle",
                "required_capability": "runtime.cycle.start",
                "idempotency_key": "old-authority",
                "request_fingerprint": "fingerprint",
                "status": "accepted",
                "phase": "provider_acknowledged",
                "effect_certainty": "unknown",
                "postcondition_status": "unknown",
                "binding": {
                    "target_member_run_id": "member-run",
                    "target_member_run_generation": 1,
                    "target_driver": {
                        "kind": "team_supervisor",
                        "team_run_id": "run",
                        "team_supervisor_id": "supervisor",
                        "team_supervisor_generation": 3
                    }
                },
                "precondition": {},
                "postcondition": {},
                "target_session_id": null,
                "target_session_generation": null,
                "source_record_id": null,
                "result": null,
                "failure_code": null,
                "version": 1,
                "created_at": "unix-ms:1",
                "updated_at": "unix-ms:1"
            }))
            .expect("RuntimeCommandRecord fixture");
        assert!(runtime_command_matches_supervisor_authority(
            &command,
            "run",
            "node-daemon:node",
            7,
            "supervisor",
            3,
        ));
        command.target_node_daemon_generation = 6;
        assert!(!runtime_command_matches_supervisor_authority(
            &command,
            "run",
            "node-daemon:node",
            7,
            "supervisor",
            3,
        ));
        command.target_node_daemon_generation = 7;
        command.binding.target_driver =
            harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
                team_run_id: "run".into(),
                team_supervisor_id: "supervisor".into(),
                team_supervisor_generation: 2,
            };
        assert!(!runtime_command_matches_supervisor_authority(
            &command,
            "run",
            "node-daemon:node",
            7,
            "supervisor",
            3,
        ));
    }
}
