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
                    || !matches!(run.status, harness_core::TeamRunStatus::Running)
                    || managed_ids.contains(&(space.id.clone(), run.id.clone()))
                {
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
                // A Running TeamRun with no Active coordination member is
                // therefore dormant, not orphaned runtime work. Re-adopting
                // it would create an unbounded Supervisor-generation loop;
                // Reopen makes the same row Active again and the next scan (or
                // explicit daemon start request) becomes eligible.
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
                        }
                        Err(error) => {
                            self.block_start_failure_if_unresolved(
                                &space.id, &store, &run.id, &error,
                            );
                            eprintln!(
                                "[node-daemon] failed to adopt {}/{}: {error}",
                                space.id, run.id
                            );
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
                    "NodeDaemon at capacity ({}/{} runs); cannot start {}/{run_id}",
                    contexts.len(),
                    self.max_concurrency,
                    space.id,
                )));
            }
        }

        let run_id = run_id.to_string();
        let max_concurrency = self.max_concurrency;
        let idle_timeout_secs = self.idle_timeout_secs;
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
        use crate::{now_string, store_conflict_as_usage};
        use harness_core::TeamRunStatus;

        let running = if body.run.status == TeamRunStatus::Planning {
            let mut running = body.run.clone();
            running.status = TeamRunStatus::Running;
            running.updated_at = now_string();
            store_conflict_as_usage(
                store.compare_and_append_team_run_lifecycle(&body.run, &running),
            )?;
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
                Duration::from_secs(idle_timeout_secs),
                Some(live_sink),
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
            finished
        };

        for mut ctx in finished {
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
    fn settle_finished_supervisor(&self, ctx: &MultiTeamContext, outcome: TeamRunDriveOutcome) {
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
}

fn team_run_has_active_member(store: &HarnessStore, run_id: &str) -> CliResult<bool> {
    Ok(crate::latest_member_runs_in_append_order(store)?
        .into_iter()
        .any(|member| {
            member.team_run_id == run_id
                && member.coordination_status == harness_core::MemberCoordinationStatus::Active
        }))
}
