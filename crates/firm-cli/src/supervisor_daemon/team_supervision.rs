//! TeamRun discovery, adoption, Supervisor start, and reap lifecycle.
//!
//! The machine authority is supplied by `machine_authority`; this module owns
//! the child Supervisor lifecycle beneath that exact daemon generation. It
//! does not own socket ingress or provider-specific transport.

use super::*;

impl MultiTeamDaemon {
    /// Scan every registered Execution Space. One broken Store is logged and
    /// isolated only before the Node is enrolled there. Every Space that has
    /// enrolled this Node participates in one machine-wide authority bundle;
    /// losing any member closes provider-effect admission for all of them.
    pub(super) fn scan_and_adopt(&self) -> CliResult<()> {
        let authority_spaces = self.ensure_node_authority_bundle()?;
        let mut managed_ids: HashSet<(String, String)> = {
            let ctx = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            ctx.iter()
                .map(|context| (context.execution_space_id.clone(), context.run_id.clone()))
                .collect()
        };

        for (space, store) in self.registered_spaces()? {
            if !authority_spaces.contains(&space.id) {
                continue;
            }
            let runs = match crate::latest_team_runs_in_append_order(&store) {
                Ok(runs) => runs,
                Err(error) => {
                    eprintln!(
                        "[node-daemon] isolating Execution Space {} after Store read failure: {error}",
                        space.id
                    );
                    continue;
                }
            };
            for run in runs {
                if run.execution_node_id != self.node_id
                    || !matches!(
                        run.status,
                        harness_core::TeamRunStatus::Running
                            | harness_core::TeamRunStatus::Completed
                    )
                    || managed_ids.contains(&(space.id.clone(), run.id.clone()))
                {
                    continue;
                }
                // A run already refused for capacity cannot start until a slot
                // frees, and every step below it — the adoption-hold read, the
                // whole-ledger member scan, the Supervisor lease read, and the
                // machine-lease renewal inside `start_supervising` — is store
                // work that competes with this daemon's own heartbeat (#836).
                if self.adoption_defers_for_capacity(&space.id, &run.id) {
                    continue;
                }
                // A durable adoption hold means the last generation already
                // proved what this canonical state produces. Honour it until
                // canonical state changes or an explicit recovery/start
                // intent clears it, instead of burning a fresh Supervisor
                // generation on the identical observation (#704, #671).
                match self.team_run_adoption_is_held(&space.id, &store, &run.id) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!(
                            "[node-daemon] cannot inspect Supervisor adoption hold for {}/{}: {error}",
                            space.id, run.id
                        );
                        continue;
                    }
                }
                // Close freezes a MemberRun without completing its TeamRun.
                // A Running or Completed TeamRun with no active managed member
                // is therefore dormant, not orphaned runtime work. Re-adopting
                // it would create an unbounded Supervisor-generation loop;
                // Reopen makes the same row Active again and the next scan (or
                // explicit daemon start request) becomes eligible. Completed
                // runs with an unclosed managed member remain eligible so a
                // daemon restart cannot strand the ordinary Close lane.
                match team_run_has_active_member(&store, &run.id) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => {
                        eprintln!(
                            "[node-daemon] cannot inspect TeamRun {} members in {}: {error}",
                            run.id, space.id
                        );
                        continue;
                    }
                }
                let now_ms = current_unix_ms_u64();
                let should_start = match store.latest_team_supervisor_lease(&run.id) {
                    Ok(None) => true,
                    Ok(Some(lease)) => {
                        lease.status != harness_core::TeamSupervisorLeaseStatus::Active
                            || lease.expires_unix_ms <= now_ms
                    }
                    Err(error) => {
                        eprintln!(
                            "[node-daemon] cannot inspect TeamRun {} in {}: {error}",
                            run.id, space.id
                        );
                        false
                    }
                };
                if should_start {
                    eprintln!(
                        "[node-daemon] adopting {}/{} on Node {}",
                        space.id, run.id, self.node_id
                    );
                    match self.start_supervising(space.clone(), store.clone(), &run.id) {
                        Ok(()) => {
                            managed_ids.insert((space.id.clone(), run.id.clone()));
                            self.clear_capacity_wait(&space.id, &run.id);
                        }
                        Err(error) => {
                            self.block_start_failure_if_unresolved(
                                &space.id, &store, &run.id, &error,
                            );
                            if refused_for_capacity(&error) {
                                // Capacity is a property of this daemon, never
                                // of the run, so the classifier above writes
                                // nothing durable. Record the wait once and
                                // let the next scan interval — or a freed
                                // slot — retry it (#836).
                                self.note_waiting_for_capacity(
                                    &space.id,
                                    &run.id,
                                    &error.to_string(),
                                );
                            } else {
                                self.clear_capacity_wait(&space.id, &run.id);
                                eprintln!(
                                    "[node-daemon] failed to adopt {}/{}: {error}",
                                    space.id, run.id
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Spawn one Team supervisor under this exact NodeDaemon generation.
    pub(super) fn start_supervising(
        &self,
        space: harness_core::ExecutionSpace,
        store: HarnessStore,
        run_id: &str,
    ) -> CliResult<()> {
        #[cfg(test)]
        ADOPTION_START_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        let _start_guard = self
            .supervisor_start_gate
            .lock()
            .map_err(|error| CliError::Usage(format!("supervisor start gate poisoned: {error}")))?;
        let authority_spaces = self.ensure_node_authority_bundle()?;
        if !authority_spaces.contains(&space.id) {
            return Err(CliError::Usage(format!(
                "NODE_HAS_NO_REGISTERED_PROJECT: Node {} has no active project in Execution Space {}",
                self.node_id, space.id
            )));
        }
        // Provider admissions are scoped to the exact Project Binding and
        // physical Execution Space. The machine daemon opens stores from the
        // space registry rather than through CLI resolution, so recover that
        // scope from the canonical TeamRun before provider preflight.
        let run_scope = crate::latest_team_run(&store, run_id)?;
        if !team_run_has_active_member(&store, run_id)? {
            return Err(CliError::Usage(format!(
                "TEAM_RUN_DORMANT: TeamRun {run_id} has no Active MemberRun; Reopen a member before runtime adoption"
            )));
        }
        let store = store.with_provider_compatibility_scope(
            run_scope.project_binding_id,
            format!("execution-space:{}", space.id),
        );
        self.ensure_node_authority(&space, &store)?;
        // Enforce the daemon-wide concurrent TeamRun limit before provider
        // effects are admitted.
        {
            let contexts = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            if contexts
                .iter()
                .any(|context| context.execution_space_id == space.id && context.run_id == run_id)
            {
                return Err(CliError::Usage(format!(
                    "NodeDaemon already manages {}/{run_id}",
                    space.id
                )));
            }
            if contexts.len() >= self.max_concurrency {
                return Err(CliError::Usage(format!(
                    "{AT_CAPACITY_REFUSAL} ({}/{} runs); cannot start {}/{run_id}",
                    contexts.len(),
                    self.max_concurrency,
                    space.id,
                )));
            }
            // A dead generation whose outcome is still being reconciled has
            // not finished writing its durable marker. Adopting now would let
            // that marker land on this live successor, so refuse with a
            // deliberately transient rejection the caller can simply retry.
            if self
                .settling_runs
                .lock()
                .map_err(|e| CliError::Usage(format!("settling lock poisoned: {e}")))?
                .contains(&(space.id.clone(), run_id.to_string()))
            {
                return Err(CliError::Usage(format!(
                    "NodeDaemon already manages {}/{run_id} (settling the previous Supervisor generation; retry)",
                    space.id
                )));
            }
        }

        let run_id = run_id.to_string();
        let max_concurrency = self.max_concurrency;
        let input_acceptance_secs = self.input_acceptance_secs;
        let native_session_wake_endpoint = Arc::clone(&self.native_session_wake_endpoint);

        // Validate and create registration outside the context lock. Store and
        // provider admission must never run while the registry mutex is held.
        let body = prepare_team_run_start_body(&store, &run_id, max_concurrency)?;
        if body.run.execution_node_id != self.node_id {
            return Err(CliError::Usage(format!(
                "REMOTE_TEAM_RUN_NOT_ADOPTED: TeamRun {run_id} belongs to Node {}, local Node is {}",
                body.run.execution_node_id, self.node_id
            )));
        }
        let project_binding_id = body.run.project_binding_id.clone();
        let daemon_generation = store
            .latest_node_daemon_lease(&self.node_id)?
            .filter(|lease| {
                lease.daemon_id == self.daemon_id && lease.instance_id == self.instance_id
            })
            .ok_or_else(|| {
                CliError::Usage("NODE_DAEMON_GENERATION_FENCED: current lease is missing".into())
            })?
            .generation;
        ensure_team_runtime_fabric(&store, &body, &space.id, &self.daemon_id, daemon_generation)?;
        let registration = TeamSupervisorRegistration::start(&store, &run_id, Some(&space.id))?;
        let supervisor_id = registration.supervisor_id.clone();
        let supervisor_generation = registration.generation;
        bind_team_runtime_supervisor(
            &store,
            &body,
            &space.id,
            &self.daemon_id,
            &registration.supervisor_id,
            registration.generation,
        )?;
        let heartbeat_valid = Arc::clone(&registration.heartbeat_valid);

        // Transition Planning→Running only after the child Supervisor is
        // admitted under this exact daemon generation.
        use crate::now_string;
        use harness_core::TeamRunStatus;

        let running = if body.run.status == TeamRunStatus::Planning {
            let mut running = body.run.clone();
            running.status = TeamRunStatus::Running;
            running.updated_at = now_string();
            // Keep the typed Store error. Flattening a CAS conflict into
            // `CliError::Usage` hides it from the adoption-hold classifier,
            // which would then read an ordinary lost race as a structural
            // defect and wedge a healthy run until canonical state changed.
            store
                .compare_and_append_team_run_lifecycle(&body.run, &running)
                .map_err(CliError::Store)?;
            running
        } else {
            body.run.clone()
        };

        let ledger = Arc::new(TeamRunLedger::new(
            &store,
            &run_id,
            &registration.supervisor_id,
            registration.generation,
            Arc::clone(&registration.heartbeat_valid),
        ));

        ledger.fold_event(
            harness_core::TeamRunEventSourceKind::Host,
            None,
            "team_run",
            &run_id,
            "updated",
            &format!(
                "member supervisor {} generation {} {} ({} unclosed member(s), max-concurrency {max_concurrency})",
                registration.supervisor_id,
                registration.generation,
                if body.run.status == TeamRunStatus::Planning {
                    "started"
                } else {
                    "reattached"
                },
                body.members.len(),
            ),
        )?;

        let prepared = PreparedTeamRunStart {
            run_id: body.run_id,
            objective: body.objective,
            running,
            members: body.members,
            ledger,
            supervisor_registration: registration,
        };

        eprintln!(
            "[node-daemon] {}/{}: serving (pid {}, gen {})",
            space.id,
            run_id,
            std::process::id(),
            prepared.supervisor_registration.generation,
        );

        let execution_space_id = space.id.clone();
        let callback_space_id = execution_space_id.clone();
        let serving_status = Arc::new(Mutex::new("running".to_string()));
        let thread_serving_status = Arc::clone(&serving_status);
        let thread = std::thread::spawn(move || {
            let live_sink = Arc::new(move |update: NativeSessionWakeUpdate| {
                let agent_member_id = match &update {
                    NativeSessionWakeUpdate::MayHaveAdvanced {
                        agent_member_id, ..
                    }
                    | NativeSessionWakeUpdate::TurnTerminal {
                        agent_member_id, ..
                    } => agent_member_id,
                };
                let endpoint = native_session_wake_endpoint
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get(agent_member_id)
                    .cloned();
                if let Some(endpoint) = endpoint {
                    if let Err(error) =
                        post_native_session_wake(&endpoint, &callback_space_id, &update)
                    {
                        if error.clears_registered_endpoint() {
                            let mut endpoints = native_session_wake_endpoint
                                .lock()
                                .unwrap_or_else(|lock_error| lock_error.into_inner());
                            if endpoints.get(agent_member_id).is_some_and(|current| {
                                current.serve_instance_id == endpoint.serve_instance_id
                                    && current.authority == endpoint.authority
                                    && current.token == endpoint.token
                            }) {
                                endpoints.remove(agent_member_id);
                            }
                        }
                        eprintln!("[node-daemon] live provider activity callback failed: {error}");
                    }
                }
            });
            drive_prepared_team_run(
                prepared,
                Some(space),
                None,
                max_concurrency,
                harness_runtime_contract::CycleTimeouts::with_input_acceptance(
                    Duration::from_secs(input_acceptance_secs),
                ),
                Some(live_sink),
                Some(thread_serving_status),
            )
        });

        // Registry mutation is the only operation under the context lock.
        {
            let mut contexts = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            contexts.push(MultiTeamContext {
                execution_space_id,
                project_binding_id,
                run_id,
                daemon_generation,
                supervisor_id,
                supervisor_generation,
                heartbeat_valid,
                serving_status,
                thread: Some(thread),
                started_at: Instant::now(),
            });
        }
        Ok(())
    }

    /// Reap finished supervisor threads and remove them from the context registry.
    ///
    /// Swapping the registry is the only step that needs the `contexts` lock.
    /// Joining a thread and reconciling its durable outcome are whole-Store
    /// scans; holding the registry lock across them head-of-line blocks every
    /// `status` and `stop` request and is what made `daemon start` miss its
    /// 60-second ready deadline (#671).
    pub(super) fn reap_finished(&self) -> CliResult<()> {
        let finished = {
            let mut contexts = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;

            let mut finished = Vec::new();
            let mut still_running = Vec::new();

            for ctx in contexts.drain(..) {
                let is_done = ctx.thread.as_ref().map(|t| t.is_finished()).unwrap_or(true);
                if is_done {
                    finished.push(ctx);
                } else {
                    still_running.push(ctx);
                }
            }

            *contexts = still_running;
            // Claim the settling window while the registry lock is still held,
            // so no explicit Start can adopt one of these runs between the
            // registry swap and the durable outcome this reap is about to
            // write for the generation that just died.
            let mut settling = self
                .settling_runs
                .lock()
                .map_err(|e| CliError::Usage(format!("settling lock poisoned: {e}")))?;
            for ctx in &finished {
                settling.insert((ctx.execution_space_id.clone(), ctx.run_id.clone()));
            }
            finished
        };

        for mut ctx in finished {
            let settled_key = (ctx.execution_space_id.clone(), ctx.run_id.clone());
            let _release = SettlingGuard {
                daemon: self,
                key: settled_key,
            };
            if let Some(thread) = ctx.thread.take() {
                match thread.join() {
                    Ok(Ok(outcome)) => self.settle_finished_supervisor(&ctx, outcome),
                    Ok(Err(e)) => {
                        self.block_finished_supervisor_failure(&ctx, &e);
                        eprintln!(
                            "[node-daemon] {}/{} error: {e}",
                            ctx.execution_space_id, ctx.run_id
                        );
                    }
                    Err(_) => {
                        self.block_finished_supervisor_failure(
                            &ctx,
                            &CliError::Usage("TEAM_SUPERVISOR_PANICKED".into()),
                        );
                        eprintln!(
                            "[node-daemon] {}/{} panicked",
                            ctx.execution_space_id, ctx.run_id
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Record what one finished Supervisor generation proved. An unresolved
    /// RuntimeCommand is the stronger diagnosis and wins; otherwise a
    /// no-progress generation leaves a durable, canonical-state-keyed hold so
    /// the next scan does not re-adopt the identical observation.
    pub(super) fn settle_finished_supervisor(
        &self,
        ctx: &MultiTeamContext,
        outcome: TeamRunDriveOutcome,
    ) {
        if self.block_finished_supervisor_if_unresolved(ctx) {
            eprintln!(
                "[node-daemon] {}/{} completed with an unresolved RuntimeCommand; adoption requires explicit recovery",
                ctx.execution_space_id, ctx.run_id
            );
            return;
        }
        match outcome {
            TeamRunDriveOutcome::Progressed { .. } => {
                eprintln!(
                    "[node-daemon] {}/{} completed",
                    ctx.execution_space_id, ctx.run_id
                );
            }
            TeamRunDriveOutcome::NoProgress {
                canonical_state,
                detail,
            } => {
                match self.store_for_space(&ctx.execution_space_id) {
                    Ok(store) => self.hold_adoption_without_progress(
                        &ctx.execution_space_id,
                        &store,
                        &ctx.run_id,
                        &detail,
                        Some(&canonical_state),
                    ),
                    Err(error) => self.block_finished_supervisor_failure(ctx, &error),
                }
                eprintln!(
                    "[node-daemon] {}/{} completed without canonical progress; adoption is held until canonical state changes",
                    ctx.execution_space_id, ctx.run_id
                );
            }
        }
    }

    /// TeamRuns this daemon currently drives. Registry-lock only; no store IO.
    fn managed_run_count(&self) -> usize {
        self.contexts
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    /// True while this TeamRun stays deferred at `--max-concurrency` (#836).
    ///
    /// The deferral ends at the next scan interval or as soon as the managed
    /// run count differs from the count observed when the refusal was
    /// recorded, whichever comes first — so a freed slot is retried on the
    /// very next pass rather than after the interval.
    pub(super) fn adoption_defers_for_capacity(
        &self,
        execution_space_id: &str,
        run_id: &str,
    ) -> bool {
        let occupancy = self.managed_run_count();
        self.capacity_waits
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(execution_space_id.to_string(), run_id.to_string()))
            .is_some_and(|wait| wait.occupancy == occupancy && Instant::now() < wait.not_before)
    }

    /// Record one at-capacity refusal, refreshing an existing wait in place.
    ///
    /// The log line is written only on the first observation of a waiting
    /// episode, or when occupancy changed and the refusal therefore says
    /// something new: a permanently over-subscribed daemon must not reprint
    /// the same refusal on every scan.
    fn note_waiting_for_capacity(&self, execution_space_id: &str, run_id: &str, detail: &str) {
        let occupancy = self.managed_run_count();
        let now = Instant::now();
        let key = (execution_space_id.to_string(), run_id.to_string());
        let mut waits = self
            .capacity_waits
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(existing) = waits.get_mut(&key) {
            if existing.occupancy == occupancy {
                existing.not_before = now + self.scan_interval;
                existing.detail = detail.to_string();
                return;
            }
        }
        waits.insert(
            key,
            CapacityWait {
                occupancy,
                not_before: now + self.scan_interval,
                since: now,
                detail: detail.to_string(),
            },
        );
        eprintln!("[node-daemon] {execution_space_id}/{run_id} waiting_for_capacity: {detail}");
    }

    /// Drop any recorded wait: the run either started or was refused for a
    /// reason capacity does not explain.
    fn clear_capacity_wait(&self, execution_space_id: &str, run_id: &str) {
        self.capacity_waits
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&(execution_space_id.to_string(), run_id.to_string()));
    }
}

/// Releases one settling claim however the reap of that context ends, so a
/// panic while reconciling an outcome cannot strand the run.
struct SettlingGuard<'daemon> {
    daemon: &'daemon MultiTeamDaemon,
    key: (String, String),
}

impl Drop for SettlingGuard<'_> {
    fn drop(&mut self) {
        self.daemon
            .settling_runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.key);
    }
}

/// Adoption attempts that actually entered `start_supervising`. Test-only: it
/// is what proves an at-capacity run is retried per scan tick, not per pass.
#[cfg(test)]
pub(super) static ADOPTION_START_ATTEMPTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// The at-capacity refusal, recognised by its own leading prefix exactly as
/// the adoption classifier recognises it.
fn refused_for_capacity(error: &CliError) -> bool {
    matches!(error, CliError::Usage(message) if message.starts_with(AT_CAPACITY_REFUSAL))
}

fn team_run_has_active_member(store: &HarnessStore, run_id: &str) -> CliResult<bool> {
    Ok(crate::latest_member_runs_in_append_order(store)?
        .into_iter()
        .any(|member| crate::completed_run_members::is_unclosed_managed_member(&member, run_id)))
}
