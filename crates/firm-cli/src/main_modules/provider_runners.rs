use super::*;

/// One persistent member supervisor. Provider transports and turns are
/// disposable generations beneath the durable ProviderRuntimeProjection/native-session
/// binding. A transport that disappears after binding is journaled as
/// disconnected and resumed; it does not silently turn the Member into a
/// terminal failure.
pub(super) fn run_member_orchestration(
    ledger: &TeamRunLedger,
    objective: &str,
    member: ProviderRuntimeProjection,
    context: MemberRuntimeContext,
) -> MemberOutcome {
    // Belt and braces: the supervisor drain already skips declared external
    // interactive members; if one ever reaches this loop it must leave with
    // its current status, never an adapter error or a Failed row.
    if member.is_external_interactive() {
        return MemberOutcome::new(
            &member,
            member.status,
            "external interactive member is user-driven; Harness does not drive it".to_string(),
        );
    }
    let hard_anchor = member.clone();
    let mut accepted = member.clone();
    let mut generation = 0u64;
    loop {
        let mut current = ledger
            .latest_member_run(&member.id)
            .ok()
            .flatten()
            .unwrap_or_else(|| member.clone());
        if let Err(error) = ledger.require_supervisor_lease() {
            return MemberOutcome::new(&current, current.status, error.to_string());
        }
        // The immutable anchor fences identity, operator intent, workspace
        // authority, and runtime generation. The rolling accepted snapshot is
        // narrower: it lets this same generation retain its own provider bind
        // and effective receipts across a disconnect, but never adopts a
        // session replacement observed only at a later loop entry.
        if !member_runtime_anchor_matches(&hard_anchor, &current) {
            return MemberOutcome::new(
                &current,
                current.status,
                "scheduled member runtime was superseded before provider start".to_string(),
            );
        }
        let pending_close = match pending_member_close(&ledger.store, &current.id) {
            Ok(close) => close,
            Err(error) => {
                return MemberOutcome::new(&current, MemberRunStatus::Failed, error.to_string())
            }
        };
        if let Some(close) = pending_close {
            if let Err(error) = stop_member_for_latched_close(ledger, &mut current, &close) {
                return MemberOutcome::new(&current, MemberRunStatus::Failed, error.to_string());
            }
            return MemberOutcome::new(
                &current,
                MemberRunStatus::Stopped,
                "member runtime closed by Host".to_string(),
            );
        }
        if !current.coordination_is_active()
            || matches!(
                current.status,
                MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
            )
        {
            return MemberOutcome::new(
                &current,
                current.status,
                "member runtime lifecycle superseded provider start".to_string(),
            );
        }
        if !member_runtime_progress_matches(&hard_anchor, &accepted, &current, false) {
            return MemberOutcome::new(
                &current,
                current.status,
                "scheduled member native session was replaced before provider start".to_string(),
            );
        }
        // Same-generation status, timestamps, provider observations/effective
        // controls, and turn-progress fields are durable progress. Rebase them
        // exactly; the short-window start claim below still CASes the complete
        // fresh row and therefore loses to Close or any later mutation.
        accepted = current.clone();
        // Re-probe every transport generation. This is the last fence before
        // capacity checks, ProviderWorkDispatch claim, process spawn, or native-session
        // attach, so a binary upgraded to an unreviewed version cannot fall
        // into the reconnect loop or replay a rebound Work.
        let compatibility_boundary = if current.native_session.is_some() {
            ProviderCompatibilityBlockBoundary::ResumePersistentExecution
        } else {
            ProviderCompatibilityBlockBoundary::StartPersistentExecution
        };
        match provider_compatibility_start_gate(ledger, &mut current, compatibility_boundary) {
            Ok(Some(outcome)) => return outcome,
            Ok(None) => {}
            Err(error) if error.is_provider_compatibility_blocked() => {
                let expected = current.clone();
                current.status = MemberRunStatus::Blocked;
                let reason = error.to_string();
                let _ = ledger.save_member_run(&expected, &current);
                return MemberOutcome::new(&current, MemberRunStatus::Blocked, reason);
            }
            Err(error) => {
                return MemberOutcome::new(&current, MemberRunStatus::Failed, error.to_string())
            }
        }
        // Capacity preflight runs once, before the adapter claims anything.
        // A blocked member returns here, so its Assignment stays `queued` and
        // is still deliverable after the provider recovers.
        if generation == 0 {
            match provider_capacity_start_gate(ledger, &mut current, &context.cwd) {
                Ok(Some(outcome)) => return outcome,
                Ok(None) => {}
                Err(error) => {
                    // The guard must never invent a failure of its own: a
                    // ledger write problem is journalled, not turned into a
                    // provider verdict.
                    ledger
                        .fold_event(
                            TeamRunEventSourceKind::Host,
                            Some(current.id.clone()),
                            "member_run",
                            &current.id,
                            "capacity_preflight_error",
                            &format!("capacity preflight could not be recorded: {error}"),
                        )
                        .ok();
                }
            }
        }
        let start_claim =
            match successor_may_take_over_active_member(ledger, &hard_anchor, &current) {
                Ok(true) => claim_member_provider_start_with_takeover_anchor_and_hook(
                    ledger,
                    &current,
                    &hard_anchor,
                    |_, _| Ok(()),
                ),
                Ok(false) => claim_member_provider_start(ledger, &current),
                Err(error) => Err(error),
            };
        current = match start_claim {
            Ok(MemberProviderStartClaim::Claimed(starting)) => starting,
            Ok(MemberProviderStartClaim::Superseded(latest)) => {
                return MemberOutcome::new(
                    &latest,
                    latest.status,
                    "member provider start was superseded by lifecycle control".to_string(),
                )
            }
            Ok(MemberProviderStartClaim::Retry) => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(error) => {
                return MemberOutcome::new(&current, MemberRunStatus::Failed, error.to_string())
            }
        };
        accepted = current.clone();
        generation += 1;
        let execution_mode = current
            .provider_profile
            .as_ref()
            .map(|profile| profile.execution_mode.as_str());
        let result = match crate::runtime_adapter::shared_team_runtime_kind(
            &current.provider,
            execution_mode,
        ) {
            Some(crate::runtime_adapter::SharedTeamRuntimeKind::Kimi) => {
                run_kimi_member_shared(ledger, objective, &current, &context)
            }
            Some(crate::runtime_adapter::SharedTeamRuntimeKind::Codex) => {
                run_codex_member_shared(ledger, objective, &current, &context)
            }
            Some(crate::runtime_adapter::SharedTeamRuntimeKind::Claude) => {
                run_claude_agent_sdk_team_member_shared(ledger, objective, &current, &context)
            }
            Some(crate::runtime_adapter::SharedTeamRuntimeKind::Pi) => {
                run_pi_team_member(ledger, objective, &current, &context)
            }
            None => Err(CliError::Usage(format!(
                "team member adapter not implemented for provider {}",
                current.provider
            ))),
        };
        match result {
            Ok(outcome) => {
                if outcome.status == MemberRunStatus::Stopped {
                    if let Ok(Some(close)) = pending_member_close(&ledger.store, &current.id) {
                        // Same stranded-claim rule as stop_member_for_latched_close:
                        // a claim this member never got provider-accepted must
                        // become re-claimable instead of wedging the Work. If the
                        // claims cannot be failed safely (for example the lease
                        // just moved), leave the Close latch pending so the next
                        // Supervisor generation applies it with the same rule.
                        if ledger
                            .fail_unreceived_work_claims_for(
                                &current.id,
                                &format!(
                                    "member closed before provider acceptance: {}: {}",
                                    close.requested_by, close.reason
                                ),
                            )
                            .is_ok()
                        {
                            let _ = ledger.store.complete_team_member_close(
                                &ledger.run_id,
                                &current.id,
                                &close.id,
                                &now_string(),
                            );
                        }
                    }
                }
                return outcome;
            }
            Err(error) => {
                let reason = error.to_string();
                eprintln!("[member-runtime-error] {}: {reason}", current.id);
                let mut latest = ledger
                    .latest_member_run(&current.id)
                    .ok()
                    .flatten()
                    .unwrap_or(current);
                if error.is_supervisor_lease_lost() {
                    return MemberOutcome::new(&latest, latest.status, reason);
                }
                match reconcile_member_lifecycle_after_provider_error(ledger, &mut latest) {
                    Ok(true) => {
                        return MemberOutcome::new(
                            &latest,
                            latest.status,
                            "provider start was superseded by member lifecycle control".to_string(),
                        )
                    }
                    Ok(false) => {}
                    Err(close_error) => {
                        return MemberOutcome::new(
                            &latest,
                            MemberRunStatus::Failed,
                            close_error.to_string(),
                        )
                    }
                }
                // Provider/process effects that may already have crossed the
                // boundary are never auto-resumed. The durable
                // RuntimeCommand is the recovery inventory; surface that
                // state once and stop the supervisor instead of creating an
                // unbounded reconnect loop that could repeat the effect.
                if reason.contains("RUNTIME_COMMAND_RECOVERY_REQUIRED")
                    || reason.contains("ambiguous in-flight RuntimeCommand")
                {
                    let _ = transition_provider_session_for_member(
                        ledger,
                        &latest,
                        harness_core::agentfirm_api::AgentSessionStatus::RecoveryRequired,
                    );
                    let expected = latest.clone();
                    latest.status = MemberRunStatus::Blocked;
                    latest.finished_at = None;
                    latest.last_event_at = Some(now_string());
                    if ledger.save_member_run(&expected, &latest).is_ok() {
                        let _ = ledger.append_action(
                            &latest.id,
                            "runtime_recovery_required",
                            MemberActionStatus::Failed,
                            "provider effect requires operator reconciliation",
                            &reason,
                        );
                        let _ = ledger.fold_event(
                            TeamRunEventSourceKind::Member,
                            Some(latest.id.clone()),
                            "member_run",
                            &latest.id,
                            "recovery_required",
                            &format!(
                                "member {} stopped before replaying an uncertain provider effect",
                                latest.name
                            ),
                        );
                    }
                    return MemberOutcome::new(&latest, MemberRunStatus::Blocked, reason);
                }
                if latest.native_session.is_none() {
                    journal_member_failure(ledger, &latest, &reason);
                    // Pre-bind: mail queued for this member can never be
                    // delivered. Fail every pending delivery so the backlog
                    // reaches a resolved state (observable via inbox).
                    let _ = ledger.fail_team_messages_for(&latest.id, &reason);
                    return MemberOutcome::new(&latest, MemberRunStatus::Failed, reason);
                }
                if !member_runtime_progress_matches(&hard_anchor, &accepted, &latest, true) {
                    return MemberOutcome::new(
                        &latest,
                        latest.status,
                        "provider result was superseded by a different runtime authority"
                            .to_string(),
                    );
                }
                if let Err(failure_error) =
                    ledger.fail_unreceived_work_claims_for(&latest.id, &reason)
                {
                    return MemberOutcome::new(
                        &latest,
                        MemberRunStatus::Failed,
                        format!(
                            "provider transport disconnected, but its unreceived Work claim could not be failed safely: {failure_error}"
                        ),
                    );
                }
                // Post-bind transport disconnect: fail queued mail in the
                // same atomic generation so the inbox is consistent.
                if let Err(failure_error) = ledger.fail_team_messages_for(&latest.id, &reason) {
                    return MemberOutcome::new(
                        &latest,
                        MemberRunStatus::Failed,
                        format!(
                            "provider transport disconnected, but its queued TeamMessageProjection could not be failed safely: {failure_error}"
                        ),
                    );
                }
                journal_member_disconnected(ledger, &latest, generation, &reason);
                accepted = ledger
                    .latest_member_run(&latest.id)
                    .ok()
                    .flatten()
                    .unwrap_or(latest);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

/// Drive one interactive Codex Team Member through one app-server process and
/// native thread. Bounded `codex exec` remains a Dynamic Workflow substrate,
/// not an alternative Agent Team Member mode.
pub(super) fn run_codex_member_shared(
    ledger: &TeamRunLedger,
    objective: &str,
    member: &ProviderRuntimeProjection,
    context: &MemberRuntimeContext,
) -> CliResult<MemberOutcome> {
    use crate::runtime_adapter::TeamRuntimeAdapter as _;

    ledger.require_supervisor_lease()?;
    let mut member_row = member.clone();
    let profile = member_row.provider_profile.clone().ok_or_else(|| {
        CliError::Usage(format!(
            "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
            member_row.id
        ))
    })?;
    let envelope = member_work_collaboration_envelope(
        ledger,
        context.execution_space_id.as_deref(),
        context.project_id.as_deref(),
        context.project_selector.as_deref(),
        &member_row,
        None,
    )?;
    let collaboration_env = envelope.environment(&context.role_action_token);
    let provider_session = require_member_provider_session_authority(ledger, &member_row, false)?;
    let permission_mapping = crate::provider_adapter::map_permission(
        &provider_session.provider_kind,
        provider_session.effective_permission_ceiling,
    )
    .map_err(CliError::Usage)?;
    let process_effect = prepare_provider_process_effect(ledger, &member_row)?;
    if let Err(error) = crate::runtime_adapter::preflight_profile_effect(
        &profile,
        &process_effect.target_session,
        crate::runtime_adapter_contract::SemanticCapability::OpenOrResume,
    ) {
        settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
        return Err(error);
    }
    let app_server = match codex_app_server::CodexAppServerClient::spawn(
        &context.cwd,
        codex_app_server::CodexAppServerSpawnOptions {
            model: member.model.as_deref(),
            reasoning_effort: member
                .provider_controls
                .reasoning_effort
                .requested
                .as_deref(),
            service_tier: member.provider_controls.service_tier.requested.as_deref(),
            resume_thread_id: member
                .native_session
                .as_ref()
                .map(|session| session.native_session_id.as_str()),
            member_name: &member.name,
            collaboration_env: &collaboration_env,
            plan_mode: false,
            sandbox: permission_mapping.native_sandbox.as_str(),
            approval_policy: permission_mapping.native_approval.as_str(),
        },
    ) {
        Ok(client) => client,
        Err(error) => {
            settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
            return Err(error);
        }
    };
    let actual_model = app_server.model().to_string();
    let actual_effort = app_server.reasoning_effort().map(str::to_string);
    let actual_tier = app_server.service_tier().map(str::to_string);
    let bound_native_session =
        native_session_ref(&member_row, app_server.thread_id(), "codex_rollout");
    let callback_member = {
        let mut member = member_row.clone();
        member.native_session = Some(bound_native_session.clone());
        member.provider_controls.model.mark_effective(
            Some(actual_model.clone()),
            "confirmed by codex app-server thread start/resume response",
        );
        if member
            .provider_controls
            .reasoning_effort
            .requested
            .is_some()
            || actual_effort.is_some()
        {
            member.provider_controls.reasoning_effort.mark_effective(
                actual_effort.clone(),
                "confirmed by codex app-server thread start/resume response",
            );
        }
        if member.provider_controls.service_tier.requested.is_some() || actual_tier.is_some() {
            member.provider_controls.service_tier.mark_effective(
                actual_tier.clone(),
                "confirmed by codex app-server thread start/resume response",
            );
        }
        member
    };
    let callback_member_id = callback_member.id.clone();
    let mut adapter = crate::codex_team_runtime::CodexTeamRuntime::new(app_server)
        .with_provider_request_handler(move |client, frame| {
            let reply = handle_codex_provider_request(ledger, &callback_member, frame)?;
            let request_id = frame
                .get("id")
                .ok_or_else(|| CliError::Usage("Codex reverse request omitted id".to_string()))?;
            client.respond(request_id, reply.result.clone())?;
            complete_provider_interaction_reply(
                ledger,
                &callback_member_id,
                &reply,
                &format!("codex-app-server-reverse:{request_id}"),
            )
        });
    adapter.bind_authority_session(process_effect.target_session.clone(), &profile)?;
    let binding = runtime_command_binding_for_session(&process_effect.target_session);
    let resume_ref = member
        .native_session
        .as_ref()
        .map(|session| session.native_session_id.as_str());
    let open_observation = match crate::runtime_adapter_contract::RuntimeAdapter::open_or_resume(
        &mut adapter,
        crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &process_effect.target_session.node_daemon_id,
            target_node_daemon_generation: process_effect.target_session.node_daemon_generation,
        },
        resume_ref,
    ) {
        Ok(observation) => observation,
        Err(error) => {
            settle_provider_effect(
                ledger,
                &process_effect,
                false,
                None,
                Some(error.to_string()),
            )?;
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: Codex open/resume could not be verified after spawn: {error}"
            )));
        }
    };
    settle_provider_effect(
        ledger,
        &process_effect,
        true,
        Some(serde_json::json!({
            "provider": "codex",
            "phase": "runtime_attached",
            "observation": open_observation,
        })),
        None,
    )?;
    transition_provider_session_runtime_control(
        ledger,
        &member_row,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        harness_core::agentfirm_api::RuntimeActivity::Idle,
    )?;
    let expected = member_row.clone();
    debug_assert_eq!(
        adapter.native_session_locator(),
        bound_native_session.native_session_id
    );
    debug_assert_eq!(adapter.native_locator_kind(), "codex_rollout");
    member_row.native_session = Some(bound_native_session);
    member_row.provider_controls.model.mark_effective(
        Some(actual_model),
        "confirmed by codex app-server thread start/resume response",
    );
    if member_row
        .provider_controls
        .reasoning_effort
        .requested
        .is_some()
        || actual_effort.is_some()
    {
        member_row
            .provider_controls
            .reasoning_effort
            .mark_effective(
                actual_effort,
                "confirmed by codex app-server thread start/resume response",
            );
    }
    if member_row
        .provider_controls
        .service_tier
        .requested
        .is_some()
        || actual_tier.is_some()
    {
        member_row.provider_controls.service_tier.mark_effective(
            actual_tier,
            "confirmed by codex app-server thread start/resume response",
        );
    }
    member_row.status = MemberRunStatus::Idle;
    member_row.last_event_at = Some(now_string());
    ledger.save_member_run(&expected, &member_row)?;
    let (live_control, registration) =
        register_live_member_control(&member_row, &context.role_action_token, 16);
    crate::runtime_adapter::run_team_member_with_adapter(
        ledger,
        objective,
        &mut member_row,
        context,
        &mut adapter,
        &live_control,
        Some(registration),
    )
}

pub(super) fn claude_agent_sdk_runner_path(cwd: &Path) -> CliResult<PathBuf> {
    if let Ok(explicit) = std::env::var("FIRM_CLAUDE_MEMBER_RUNNER")
        .or_else(|_| std::env::var("HARNESS_CLAUDE_MEMBER_RUNNER"))
    {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        return Err(CliError::Usage(format!(
            "FIRM_CLAUDE_MEMBER_RUNNER points at {}, which is not a file",
            path.display()
        )));
    }
    let current_executable = std::env::current_exe()
        .ok()
        .map(|path| fs::canonicalize(&path).unwrap_or(path));
    claude_agent_sdk_runner_path_from(cwd, current_executable.as_deref())
}

pub(super) fn claude_agent_sdk_runner_path_from(
    cwd: &Path,
    current_executable: Option<&Path>,
) -> CliResult<PathBuf> {
    const RELATIVE: &str = "apps/claude-member-runner/bin/claude-member-runner.mjs";
    let mut bases = Vec::new();
    if let Some(executable) = current_executable {
        bases.extend(executable.ancestors().map(Path::to_path_buf));
    }
    bases.extend(cwd.ancestors().map(Path::to_path_buf));
    let mut visited = HashSet::new();
    for base in bases
        .into_iter()
        .filter(|base| visited.insert(base.clone()))
    {
        let candidate = base.join(RELATIVE);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    // Fail explicitly rather than silently degrading to the `-p` path: a member
    // that quietly loses its control channel is exactly the failure this mode
    // exists to remove. Since `claude_agent_sdk` is the only Claude Agent Team
    // mode, this message tells a first-time runner how to repair the host.
    Err(CliError::Usage(format!(
        "claude_agent_sdk runner not found. Looked for `{RELATIVE}` from {} and \
         the installed Harness binary, and HARNESS_CLAUDE_MEMBER_RUNNER is unset.\n\
         Fix one of:\n  \
         - install Star Harness with `pnpm star-harness:install`\n  \
         - run from a checkout that contains the runner, or point \
         HARNESS_CLAUDE_MEMBER_RUNNER at it\n  \
         - install its dependency once: pnpm add @anthropic-ai/claude-agent-sdk",
        cwd.display()
    )))
}

/// Drive one Agent-SDK-backed Claude member.
///
/// Harness owns coordination; the runner owns exactly one provider-native
/// session. Coordination records stay provider-neutral while lifecycle control
/// is backed by the SDK transport.
pub(super) fn run_claude_agent_sdk_team_member_shared(
    ledger: &TeamRunLedger,
    objective: &str,
    member: &ProviderRuntimeProjection,
    context: &MemberRuntimeContext,
) -> CliResult<MemberOutcome> {
    use crate::runtime_adapter::TeamRuntimeAdapter as _;

    ledger.require_supervisor_lease()?;
    let mut member_row = member.clone();
    let profile = member_row.provider_profile.clone().ok_or_else(|| {
        CliError::Usage(format!(
            "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
            member_row.id
        ))
    })?;
    let runner = claude_agent_sdk_runner_path(&context.cwd)?;
    let envelope = member_work_collaboration_envelope(
        ledger,
        context.execution_space_id.as_deref(),
        context.project_id.as_deref(),
        context.project_selector.as_deref(),
        &member_row,
        None,
    )?;
    let process_effect = prepare_provider_process_effect(ledger, &member_row)?;
    if let Err(error) = crate::runtime_adapter::preflight_profile_effect(
        &profile,
        &process_effect.target_session,
        crate::runtime_adapter_contract::SemanticCapability::OpenOrResume,
    ) {
        settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
        return Err(error);
    }
    let environment = envelope
        .environment(&context.role_action_token)
        .into_iter()
        .map(|(key, value)| {
            (
                std::ffi::OsString::from(key),
                std::ffi::OsString::from(value),
            )
        })
        .collect();
    let mut adapter = match crate::claude_team_runtime::ClaudeTeamRuntime::spawn(
        crate::claude_team_runtime::ClaudeTeamRuntimeConfig {
            runner_path: runner,
            cwd: context.cwd.clone(),
            team_run_id: ledger.run_id.clone(),
            member_run_id: member.id.clone(),
            member_name: member.name.clone(),
            role_label: member.role.clone(),
            owned_paths: member.owned_paths.clone(),
            model: member.model.clone(),
            effort: member.provider_controls.reasoning_effort.requested.clone(),
            permission_mode: claude_team_permission_mode().to_string(),
            allowed_tools: None,
            disallowed_tools: None,
            setting_sources: vec!["project".to_string(), "user".to_string()],
            resume_session_id: member
                .native_session
                .as_ref()
                .map(|session| session.native_session_id.clone()),
            environment,
        },
    ) {
        Ok(adapter) => adapter,
        Err(error) => {
            settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
            return Err(error);
        }
    };
    adapter.bind_authority_session(process_effect.target_session.clone(), &profile)?;
    let binding = runtime_command_binding_for_session(&process_effect.target_session);
    let resume_ref = member
        .native_session
        .as_ref()
        .map(|session| session.native_session_id.as_str());
    let open_observation = match crate::runtime_adapter_contract::RuntimeAdapter::open_or_resume(
        &mut adapter,
        crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &process_effect.target_session.node_daemon_id,
            target_node_daemon_generation: process_effect.target_session.node_daemon_generation,
        },
        resume_ref,
    ) {
        Ok(observation) => observation,
        Err(error) => {
            settle_provider_effect(
                ledger,
                &process_effect,
                false,
                None,
                Some(error.to_string()),
            )?;
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: Claude open/resume could not be verified after spawn: {error}"
            )));
        }
    };
    settle_provider_effect(
        ledger,
        &process_effect,
        true,
        Some(serde_json::json!({
            "provider": "claude",
            "phase": "runtime_attached",
            "observation": open_observation,
        })),
        None,
    )?;
    transition_provider_session_runtime_control(
        ledger,
        &member_row,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        harness_core::agentfirm_api::RuntimeActivity::Idle,
    )?;
    let expected = member_row.clone();
    member_row.native_session = Some(native_session_ref(
        &member_row,
        adapter.native_session_locator(),
        adapter.native_locator_kind(),
    ));
    if member_row
        .provider_controls
        .service_tier
        .requested
        .is_some()
    {
        member_row
            .provider_controls
            .service_tier
            .mark_unsupported("Claude Agent SDK exposes no reviewed service-tier control");
    }
    member_row.status = MemberRunStatus::Idle;
    member_row.last_event_at = Some(now_string());
    ledger.save_member_run(&expected, &member_row)?;
    let (live_control, registration) =
        register_live_member_control(&member_row, &context.role_action_token, 16);
    crate::runtime_adapter::run_team_member_with_adapter(
        ledger,
        objective,
        &mut member_row,
        context,
        &mut adapter,
        &live_control,
        Some(registration),
    )
}

pub(super) fn claude_team_permission_mode() -> &'static str {
    "bypassPermissions"
}

pub(super) fn run_kimi_member_shared(
    ledger: &TeamRunLedger,
    objective: &str,
    member: &ProviderRuntimeProjection,
    context: &MemberRuntimeContext,
) -> CliResult<MemberOutcome> {
    use crate::runtime_adapter::TeamRuntimeAdapter as _;
    use std::cell::RefCell;
    use std::rc::Rc;

    ledger.require_supervisor_lease()?;
    let mut member_row = member.clone();
    let profile = member_row.provider_profile.clone().ok_or_else(|| {
        CliError::Usage(format!(
            "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
            member_row.id
        ))
    })?;
    let envelope = member_work_collaboration_envelope(
        ledger,
        context.execution_space_id.as_deref(),
        context.project_id.as_deref(),
        context.project_selector.as_deref(),
        &member_row,
        None,
    )?;
    let collaboration_env = envelope.environment(&context.role_action_token);
    let provider_session = require_member_provider_session_authority(ledger, &member_row, false)?;
    crate::provider_adapter::map_permission(
        &provider_session.provider_kind,
        provider_session.effective_permission_ceiling,
    )
    .map_err(CliError::Usage)?;
    let process_effect = prepare_provider_process_effect(ledger, &member_row)?;
    if let Err(error) = crate::runtime_adapter::preflight_profile_effect(
        &profile,
        &process_effect.target_session,
        crate::runtime_adapter_contract::SemanticCapability::OpenOrResume,
    ) {
        settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
        return Err(error);
    }
    let client = match kimi_acp::KimiAcpClient::spawn(
        &context.cwd,
        member.model.as_deref(),
        member
            .provider_controls
            .reasoning_effort
            .requested
            .as_deref(),
        member
            .native_session
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        &collaboration_env,
    ) {
        Ok(client) => client,
        Err(error) => {
            settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
            return Err(error);
        }
    };
    if client.provider_version() != profile.provider_version.as_deref() {
        let detail = format!(
            "KIMI_PROVIDER_VERSION_DRIFT: profile={:?}, handshake={:?}",
            profile.provider_version,
            client.provider_version()
        );
        settle_provider_effect(ledger, &process_effect, false, None, Some(detail.clone()))?;
        return Err(CliError::Usage(format!(
            "RUNTIME_COMMAND_RECOVERY_REQUIRED: {detail}"
        )));
    }
    let effective_model = client.model().map(str::to_string);
    let effective_effort = client.effort().map(str::to_string);
    let session_id = client
        .session_id()
        .filter(|session| !session.trim().is_empty())
        .ok_or_else(|| {
            CliError::Usage(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: Kimi ACP handshake omitted session id"
                    .to_string(),
            )
        })?
        .to_string();
    let mut callback_member = member_row.clone();
    let bound_native_session =
        native_session_ref(&callback_member, &session_id, "kimi_code_session");
    callback_member.native_session = Some(bound_native_session.clone());
    if let Some(model) = effective_model.clone() {
        callback_member
            .provider_controls
            .model
            .mark_effective(Some(model), "acknowledged by Kimi ACP model configuration");
    }
    if let Some(effort) = effective_effort.clone() {
        callback_member
            .provider_controls
            .reasoning_effort
            .mark_effective(
                Some(effort),
                "acknowledged by Kimi ACP thinking configuration",
            );
    } else {
        callback_member.provider_controls.reasoning_effort =
            harness_core::ProviderControlValue::requested(
                callback_member
                    .provider_controls
                    .reasoning_effort
                    .requested
                    .clone(),
            );
    }
    if callback_member
        .provider_controls
        .service_tier
        .requested
        .is_some()
    {
        callback_member
            .provider_controls
            .service_tier
            .mark_unsupported("Kimi ACP exposes no reviewed service-tier control");
    }
    let callback_member_id = callback_member.id.clone();
    let pending_replies = Rc::new(RefCell::new(
        HashMap::<String, ProviderInteractionReply>::new(),
    ));
    let request_replies = Rc::clone(&pending_replies);
    let written_replies = Rc::clone(&pending_replies);
    let request_member = callback_member.clone();
    let written_member_id = callback_member_id.clone();
    let mut adapter = crate::kimi_team_runtime::KimiTeamRuntime::new(
        client,
        move |frame| {
            let reply = handle_kimi_provider_request(ledger, &request_member, frame)?;
            let key = frame
                .get("id")
                .map(|id| id.to_string())
                .ok_or_else(|| CliError::Usage("Kimi reverse request omitted id".to_string()))?;
            let result = reply.result.clone();
            request_replies.borrow_mut().insert(key, reply);
            Ok(result)
        },
        move |frame| {
            let key = frame
                .get("id")
                .map(|id| id.to_string())
                .ok_or_else(|| CliError::Usage("Kimi reverse request omitted id".to_string()))?;
            let reply = written_replies.borrow_mut().remove(&key).ok_or_else(|| {
                CliError::Usage(format!(
                    "KIMI_PROVIDER_REPLY_UNKNOWN: native write completed without pending reply {key}"
                ))
            })?;
            complete_provider_interaction_reply(
                ledger,
                &written_member_id,
                &reply,
                &format!("kimi-acp-reverse:{key}"),
            )
        },
    );
    adapter.bind_authority_session(process_effect.target_session.clone(), &profile)?;
    let binding = runtime_command_binding_for_session(&process_effect.target_session);
    let resume_ref = member
        .native_session
        .as_ref()
        .map(|session| session.native_session_id.as_str());
    let open_observation = match crate::runtime_adapter_contract::RuntimeAdapter::open_or_resume(
        &mut adapter,
        crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &process_effect.target_session.node_daemon_id,
            target_node_daemon_generation: process_effect.target_session.node_daemon_generation,
        },
        resume_ref,
    ) {
        Ok(observation) => observation,
        Err(error) => {
            settle_provider_effect(
                ledger,
                &process_effect,
                false,
                None,
                Some(error.to_string()),
            )?;
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: Kimi open/resume could not be verified after spawn: {error}"
            )));
        }
    };
    settle_provider_effect(
        ledger,
        &process_effect,
        true,
        Some(serde_json::json!({
            "provider": "kimi",
            "phase": "runtime_attached",
            "observation": open_observation,
        })),
        None,
    )?;
    transition_provider_session_runtime_control(
        ledger,
        &member_row,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        harness_core::agentfirm_api::RuntimeActivity::Idle,
    )?;
    let expected = member_row.clone();
    debug_assert_eq!(adapter.native_session_locator(), session_id);
    debug_assert_eq!(adapter.native_locator_kind(), "kimi_code_session");
    member_row.native_session = Some(bound_native_session);
    if let Some(model) = effective_model {
        member_row
            .provider_controls
            .model
            .mark_effective(Some(model), "acknowledged by Kimi ACP model configuration");
    }
    if let Some(effort) = effective_effort {
        member_row
            .provider_controls
            .reasoning_effort
            .mark_effective(
                Some(effort),
                "acknowledged by Kimi ACP thinking configuration",
            );
    } else {
        member_row.provider_controls.reasoning_effort =
            harness_core::ProviderControlValue::requested(
                member_row
                    .provider_controls
                    .reasoning_effort
                    .requested
                    .clone(),
            );
    }
    if member_row
        .provider_controls
        .service_tier
        .requested
        .is_some()
    {
        member_row
            .provider_controls
            .service_tier
            .mark_unsupported("Kimi ACP exposes no reviewed service-tier control");
    }
    member_row.status = MemberRunStatus::Idle;
    member_row.last_event_at = Some(now_string());
    ledger.save_member_run(&expected, &member_row)?;
    let (live_control, registration) =
        register_live_member_control(&member_row, &context.role_action_token, 16);
    crate::runtime_adapter::run_team_member_with_adapter(
        ledger,
        objective,
        &mut member_row,
        context,
        &mut adapter,
        &live_control,
        Some(registration),
    )
}
