//! Supervisor admission passes and member driver ownership.

use super::*;

/// A superseded roster entry relinquishes all attempt-scoped retry state.
/// The next runtime generation starts with its own full provisioning budget.
pub(super) fn prepare_member_admission(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    workspace: &MemberWorkspaceSnapshot,
    retry_not_before: &mut HashMap<String, Instant>,
    fabric_attempts: &mut HashMap<String, u32>,
) -> CliResult<PreSpawnWorkspacePreparation> {
    let result = prepare_member_workspace_for_spawn(ledger, member, workspace)?;
    if matches!(result, PreSpawnWorkspacePreparation::Superseded) {
        retry_not_before.remove(&member.id);
        fabric_attempts.remove(&member.id);
    }
    Ok(result)
}

pub(crate) fn drive_prepared_team_run(
    prepared: PreparedTeamRunStart,
    execution_space: Option<ExecutionSpace>,
    project_context: Option<ProjectContext>,
    max_concurrency: usize,
    timeouts: harness_runtime_contract::CycleTimeouts,
    live_sink: Option<NativeSessionWakeSink>,
    serving_status: Option<Arc<Mutex<String>>>,
) -> CliResult<TeamRunDriveOutcome> {
    let PreparedTeamRunStart {
        run_id,
        objective,
        running,
        members,
        ledger,
        supervisor_registration: _supervisor_registration,
    } = prepared;
    let project_context = {
        let binding_id = running.project_binding_id.as_str();
        {
            let pinned = project::firm_home()
                .ok()
                .and_then(|home| project::context_for_id(&home, binding_id).ok().flatten());
            match pinned {
                Some(context) => Some(context),
                None => match project_context {
                    Some(context) if context.id == binding_id => Some(context),
                    _ => {
                        return Err(CliError::Usage(format!(
                            "team run {} is pinned to unavailable Project Binding {}; \
                             restore or register that binding before starting members",
                            running.id, binding_id
                        )))
                    }
                },
            }
        }
    };
    let execution_space_id = execution_space.as_ref().map(|space| space.id.clone());
    // Observe the canonical state this generation inherits before any member
    // runtime touches it. The same observation at exit is what distinguishes
    // "this adoption changed something" from "this adoption produced nothing
    // a further adoption could improve on".
    let entry_canonical_state = team_run_canonical_state_fingerprint(
        &ledger.store,
        execution_space_id.as_deref(),
        &run_id,
    )?;
    let project_id = project_context.as_ref().map(|context| context.id.clone());
    let project_selector = project_context
        .as_ref()
        .map(|context| context.project_root.to_string_lossy().into_owned());
    let mut seen_runtime_generations = HashMap::<String, u64>::new();
    let mut member_retry_not_before = HashMap::<String, Instant>::new();
    let mut member_fabric_attempts = HashMap::<String, u32>::new();
    let mut pending_members = members;
    let mut handles = HashMap::new();
    let mut outcomes = Vec::new();
    let turn_leases = Arc::new(ActiveTurnLeasePool::new(max_concurrency));
    let mut lease_lost = false;
    // Carried across passes so an idle completed-serving tick can keep the
    // projection it already proved instead of decoding the ledgers again
    // (#836).
    let mut current_run_status = None;
    let mut completed_unclosed = 0usize;
    let mut serving_idler = crate::completed_run_members::CompletedRunServingIdler::new(
        crate::completed_run_members::COMPLETED_RUN_SERVING_POLL_INTERVAL,
    );
    // Fire the GitHub CI poll on the first iteration, then every
    // GITHUB_CI_POLL_INTERVAL (issue #369 Phase 2).
    let mut last_github_ci_poll = Instant::now() - GITHUB_CI_POLL_INTERVAL;
    loop {
        if !lease_lost {
            if let Err(error) = ledger.require_supervisor_lease() {
                if error.is_supervisor_lease_lost() {
                    lease_lost = true;
                    pending_members.clear();
                } else {
                    return Err(error);
                }
            }
        }
        if !lease_lost {
            for mut member in std::mem::take(&mut pending_members) {
                if handles.contains_key(&member.id) {
                    continue;
                }
                if member_retry_not_before
                    .get(&member.id)
                    .is_some_and(|deadline| *deadline > Instant::now())
                {
                    pending_members.push(member);
                    continue;
                }
                member_retry_not_before.remove(&member.id);
                seen_runtime_generations.insert(member.id.clone(), member.runtime_generation);
                let member_ledger = Arc::clone(&ledger);
                // A declared external interactive member is driven by the user
                // in their own already-open provider session: no adapter
                // thread, no workspace snapshot, no Failed/Disconnected
                // derivation. Its deliveries stay queued until the session
                // polls its inbox and acks.
                if member.is_external_interactive() {
                    member_ledger.fold_event(
                        TeamRunEventSourceKind::Host,
                        Some(member.id.clone()),
                        "member_run",
                        &member.id,
                        "updated",
                        &format!(
                            "external interactive member {} is user-driven; supervisor does not spawn an adapter",
                            member.name
                        ),
                    )?;
                    continue;
                }
                let member_objective = objective.clone();
                let cwd = member_spawn_cwd(project_context.as_ref(), &running, &member);
                let provider_environment_observation = snapshot_member_workspace(
                    &cwd,
                    project_context.as_ref().map(|context| context.id.as_str()),
                    project_context
                        .as_ref()
                        .map(|context| context.project_root.as_path()),
                    if member
                        .provider_cwd_hint
                        .as_deref()
                        .is_some_and(|value| !value.is_empty())
                    {
                        "member_worktree"
                    } else if running
                        .execution_root
                        .as_deref()
                        .is_some_and(|value| !value.is_empty())
                    {
                        "team_execution_root"
                    } else if project_context.is_some() {
                        "project_binding_root"
                    } else {
                        "explicit_unbound"
                    },
                );
                let published_member = match prepare_member_admission(
                    &member_ledger,
                    &member,
                    &provider_environment_observation,
                    &mut member_retry_not_before,
                    &mut member_fabric_attempts,
                )? {
                    PreSpawnWorkspacePreparation::Ready(member) => *member,
                    PreSpawnWorkspacePreparation::Superseded => {
                        continue;
                    }
                    PreSpawnWorkspacePreparation::Retry => {
                        member_retry_not_before.insert(
                            member.id.clone(),
                            Instant::now() + Duration::from_millis(50),
                        );
                        pending_members.push(member);
                        continue;
                    }
                };
                member_retry_not_before.remove(&member.id);
                member = published_member;
                member_ledger.fold_event(
                    TeamRunEventSourceKind::Host,
                    Some(member.id.clone()),
                    "member_run",
                    &member.id,
                    "provider_environment_observation",
                    &format!("member workspace resolved to {}", cwd.display()),
                )?;
                // A member admitted into a live run never passed through the
                // adoption seam that materializes AgentSessions, so provision
                // it here before any provider effect is prepared (#749). An
                // original member already has its session and is untouched.
                match ensure_joined_member_runtime_fabric(&member_ledger, &mut member) {
                    Ok(JoinedMemberRuntimeFabric::AlreadyProvisioned) => {
                        member_fabric_attempts.remove(&member.id);
                    }
                    Ok(JoinedMemberRuntimeFabric::Provisioned { session_id }) => {
                        member_fabric_attempts.remove(&member.id);
                        member_ledger.fold_event(
                            TeamRunEventSourceKind::Host,
                            Some(member.id.clone()),
                            "member_run",
                            &member.id,
                            "updated",
                            &format!(
                                "member {} joined a live run; AgentSession {session_id} bound to supervisor {} generation {}",
                                member.name, ledger.supervisor_id, ledger.supervisor_generation
                            ),
                        )?;
                    }
                    Err(error) => {
                        let failure = classify_member_fabric_failure(&error);
                        if matches!(failure, MemberFabricFailure::LeaseLost) {
                            // Identical to this loop's own lease check above:
                            // quiesce the generation and leave every member
                            // exactly as the successor will find it.
                            lease_lost = true;
                            pending_members.clear();
                            break;
                        }
                        if matches!(failure, MemberFabricFailure::Transient) {
                            let attempts = {
                                let counter = member_fabric_attempts
                                    .entry(member.id.clone())
                                    .or_insert(0u32);
                                *counter += 1;
                                *counter
                            };
                            if attempts < MEMBER_FABRIC_PROVISION_ATTEMPTS {
                                member_retry_not_before.insert(
                                    member.id.clone(),
                                    Instant::now() + Duration::from_millis(50),
                                );
                                pending_members.push(member);
                                continue;
                            }
                        }
                        // A durable refusal about this member, or an
                        // attempt-scoped race that never cleared. Nothing
                        // reached a provider, so this is an ordinary failure
                        // the Host can read — never a recovery claim about an
                        // ambiguous effect. The error is journalled unchanged
                        // so its own leading code token survives for the
                        // adoption classifier.
                        let reason = error.to_string();
                        journal_member_failure(&member_ledger, &member, &reason);
                        outcomes.push(MemberOutcome::new(&member, MemberRunStatus::Failed, reason));
                        continue;
                    }
                }
                let handle_member = member.clone();
                let member_live_sink = live_sink.clone();
                let member_project_id = project_id.clone();
                let member_project_selector = project_selector.clone();
                let member_execution_space_id = execution_space_id.clone();
                let member_turn_leases = Arc::clone(&turn_leases);
                let member_role_action_token = generated_member_role_action_token()?;
                let handle = std::thread::spawn(move || {
                    run_member_orchestration(
                        &member_ledger,
                        &member_objective,
                        handle_member,
                        MemberRuntimeContext {
                            execution_space_id: member_execution_space_id,
                            project_id: member_project_id,
                            project_selector: member_project_selector,
                            cwd,
                            timeouts,
                            live_sink: member_live_sink,
                            turn_leases: member_turn_leases,
                            role_action_token: member_role_action_token,
                        },
                    )
                });
                handles.insert(member.id.clone(), (member, handle));
            }
        }

        let finished_member_ids = handles
            .iter()
            .filter(|(_, (_, handle))| handle.is_finished())
            .map(|(member_id, _)| member_id.clone())
            .collect::<Vec<_>>();
        for member_id in finished_member_ids {
            let Some((member, handle)) = handles.remove(&member_id) else {
                continue;
            };
            match handle.join() {
                Ok(outcome) => outcomes.push(outcome),
                Err(_) => {
                    journal_member_failure(&ledger, &member, "orchestration thread panicked");
                    outcomes.push(MemberOutcome::new(
                        &member,
                        MemberRunStatus::Failed,
                        "orchestration thread panicked".to_string(),
                    ));
                }
            }
        }

        // A Completed run with no member handle left is served only to keep
        // the Close lane's provider-loop authority; re-decoding both ledgers
        // every tick starved the daemon's own heartbeat off the machine
        // (#836). Skip the decode while their bytes are provably unchanged.
        let idle_completed_serving = handles.is_empty()
            && current_run_status == Some(TeamRunStatus::Completed)
            && completed_unclosed > 0;
        if lease_lost {
            current_run_status = None;
            completed_unclosed = 0;
            pending_members.clear();
        } else if let crate::completed_run_members::ServingObservation::Rescanned {
            members: latest_members,
            run_status,
        } = serving_idler.observe(&ledger.store, &run_id, idle_completed_serving)?
        {
            current_run_status = Some(run_status);
            completed_unclosed = 0;
            if run_status == TeamRunStatus::Completed {
                completed_unclosed = crate::completed_run_members::unclosed_managed_member_count(
                    &latest_members,
                    &run_id,
                );
                if let Some(status) = &serving_status {
                    *status.lock().unwrap_or_else(|error| error.into_inner()) =
                        crate::completed_run_members::completed_serving_label(completed_unclosed);
                }
            }
            for member in members_joined_since_last_pass(
                latest_members,
                &run_id,
                run_status,
                &seen_runtime_generations,
                |member_id| handles.contains_key(member_id),
            ) {
                if !pending_members.iter().any(|pending| {
                    pending.id == member.id
                        && pending.runtime_generation == member.runtime_generation
                }) {
                    pending_members.push(member);
                }
            }
        }
        if !pending_members.is_empty() {
            ledger.fold_event(
                TeamRunEventSourceKind::Host,
                None,
                "team_run",
                &run_id,
                "updated",
                &format!(
                    "{} member(s) joined while the supervisor was active",
                    pending_members.len()
                ),
            )?;
            if let Some(delay) = pending_members
                .iter()
                .filter_map(|member| member_retry_not_before.get(&member.id))
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .min()
                .filter(|delay| !delay.is_zero())
            {
                std::thread::sleep(delay.min(Duration::from_millis(50)));
            }
            continue;
        }

        // A TeamRun decision never closes a Member. A Completed attempt keeps
        // its Supervisor/control lane until every managed member is explicitly
        // Closed or Retired, even when a member adapter has already returned.
        // The registration Drop below then releases the lease after the last
        // member leaves. Other statuses preserve the existing empty-handle
        // exit behavior.
        if handles.is_empty() {
            if current_run_status == Some(TeamRunStatus::Completed) && completed_unclosed > 0 {
                serving_idler.wait_for_ledger_change(&ledger.store);
                continue;
            }
            break;
        }
        // GitHub linkage CI poll (issue #369 Phase 2): throttled, best-effort,
        // never fatal to the supervisor loop.
        if last_github_ci_poll.elapsed() >= GITHUB_CI_POLL_INTERVAL {
            last_github_ci_poll = Instant::now();
            match poll_team_run_github_linkages(&ledger.store, &run_id) {
                Ok(summary) if !summary.is_noop() => {
                    let mut detail = format!(
                        "github linkage poll: {} link(s) refreshed",
                        summary.links_refreshed
                    );
                    if !summary.blocked_on_failure.is_empty() {
                        detail.push_str(&format!(
                            "; held {} on red CI: {}",
                            summary.blocked_on_failure.len(),
                            summary.blocked_on_failure.join(", ")
                        ));
                    }
                    ledger.fold_event(
                        TeamRunEventSourceKind::Host,
                        None,
                        "team_run",
                        &run_id,
                        "updated",
                        &detail,
                    )?;
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("[supervisor] github linkage poll skipped: {error}");
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if lease_lost {
        return Err(supervisor_lease_lost_error(&run_id));
    }

    let current = latest_team_run(&ledger.store, &run_id)?;
    // The exit observation must be taken before this generation journals its
    // own stop event, and the fingerprint deliberately excludes that journal,
    // so "nothing changed" stays provable rather than self-refuting.
    let exit_canonical_state = team_run_canonical_state_fingerprint(
        &ledger.store,
        execution_space_id.as_deref(),
        &run_id,
    )?;
    let drive_outcome = classify_team_run_drive_outcome(
        current.status,
        &entry_canonical_state,
        &exit_canonical_state,
        outcomes.len(),
    );
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        None,
        "team_run",
        &run_id,
        "updated",
        &format!(
            "member supervisor stopped with team run still {} ({} runtime outcome(s), canonical state {})",
            serde_snake_label(&current.status),
            outcomes.len(),
            match &drive_outcome {
                TeamRunDriveOutcome::NoProgress { .. } => "unchanged",
                TeamRunDriveOutcome::Progressed { .. } => "changed",
            }
        ),
    )?;

    println!("team run {run_id}\t{}", serde_snake_label(&current.status));
    for outcome in &outcomes {
        println!(
            "  {} ({}/{})\t{}",
            outcome.name,
            outcome.role,
            outcome.provider,
            serde_snake_label(&outcome.status)
        );
        for line in outcome.summary.lines().take(3) {
            println!("    {line}");
        }
    }
    Ok(drive_outcome)
}
