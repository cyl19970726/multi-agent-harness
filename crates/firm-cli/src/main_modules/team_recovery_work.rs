use super::*;

/// Per-member recovery classification returned by the pure decision function.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(super) enum MemberRecoveryPath {
    /// Member is already active with a current supervisor lease.
    AlreadyActive,
    /// Native session exists and supports resume — reopen the member.
    ResumeCompatible,
    /// Session is incompatible or missing — rebind Works to a new generation.
    RebindIncompatible { reason: String },
    /// Member is in a terminal state (retired/completed/failed).
    Terminal { reason: String },
}

/// Pure function: classify a member's recovery path. Does not perform I/O or
/// mutation. Unit-testable across every edge case without a store.
pub(super) fn classify_member_recovery_path(
    member: &ProviderRuntimeProjection,
    supervisor_current: bool,
) -> MemberRecoveryPath {
    // Retired members are permanently dead.
    if member.coordination_is_retired() {
        return MemberRecoveryPath::Terminal {
            reason: "member is retired".to_string(),
        };
    }
    // Already active — running coordination, regardless of supervisor.
    if member.coordination_is_active() {
        return MemberRecoveryPath::AlreadyActive;
    }
    // Terminal runtime status without an active coordinator.
    if matches!(
        member.status,
        MemberRunStatus::Completed | MemberRunStatus::Failed
    ) && !supervisor_current
    {
        return MemberRecoveryPath::Terminal {
            reason: format!(
                "member is {} with no active supervisor",
                serde_snake_label(&member.status)
            ),
        };
    }
    // Closed/stopped members need inspection.
    if !member.coordination_is_active()
        && matches!(
            member.status,
            MemberRunStatus::Stopped
                | MemberRunStatus::Completed
                | MemberRunStatus::Failed
                | MemberRunStatus::Idle
                | MemberRunStatus::Queued
        )
    {
        // External interactive members are always resumable (even if Stopped).
        if member.is_external_interactive() {
            return MemberRecoveryPath::ResumeCompatible;
        }
        // Check native session resumability.
        if let Some(native_session) = member.native_session.as_ref() {
            if native_session.supports_resume
                && !matches!(
                    native_session.availability,
                    harness_core::NativeSessionAvailability::Missing
                        | harness_core::NativeSessionAvailability::Incompatible
                )
            {
                // Also check provider profile.
                if let Some(profile) = member.provider_profile.as_ref() {
                    if profile.supports_resume {
                        return MemberRecoveryPath::ResumeCompatible;
                    }
                }
            }
            return MemberRecoveryPath::RebindIncompatible {
                reason: format!(
                    "native session {} is not resumable (availability: {})",
                    native_session.native_session_id,
                    serde_snake_label(&native_session.availability)
                ),
            };
        }
        // No native session for a non-external member: if stopped, rebind; otherwise can resume.
        if member.status == MemberRunStatus::Stopped {
            return MemberRecoveryPath::RebindIncompatible {
                reason: "no native session and member is stopped".to_string(),
            };
        }
        return MemberRecoveryPath::ResumeCompatible;
    }
    MemberRecoveryPath::Terminal {
        reason: format!(
            "member status {} coordination {} not recoverable",
            serde_snake_label(&member.status),
            serde_snake_label(&member.coordination_status)
        ),
    }
}

/// Recover a team run without minting new ids: reconcile deliveries, reopen
/// compatible sessions, rebind incompatible ones. Always reads current state
/// first; never creates new TeamRun ids or Work ids.
pub(super) fn team_run_recover(
    store: &HarnessStore,
    team_run_id: &str,
    json: bool,
) -> CliResult<serde_json::Value> {
    let run = latest_team_run(store, team_run_id)?;
    let mut members: Vec<ProviderRuntimeProjection> = latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == team_run_id)
        .collect();
    let works: Vec<Work> = store
        .latest_works()?
        .into_iter()
        .filter(|work| work.team_run_id == team_run_id)
        .collect();
    let deliveries = store.current_work_deliveries_for_team_run(team_run_id)?;
    let supervisor = store.latest_team_supervisor_lease(team_run_id)?;
    let supervisor_current = supervisor.as_ref().is_some_and(is_supervisor_current);

    // Legacy reader: Mission Log tail (ADR 0051). Printed only when the
    // Team carries legacy Mission provenance; post-DEV-35 mission-less Teams
    // have no Mission Log to re-read. When present this stays before provider
    // probing or any recovery mutation so a recovering Host re-reads durable
    // judgment before acting on provider-native state.
    if !json {
        if let Some(mission_id) = team_run_mission_id(store, &run)? {
            println!("── mission log (last 3) ──");
            match store.mission_log_tail(&mission_id, 3) {
                Ok(entries) => println!("{}", format_mission_log_entries_text(&entries)),
                Err(error) => println!("mission log unavailable: {error}"),
            }
            println!();
        }
    }

    // Recovery must not mutate a ProviderRuntimeProjection generation, reconcile a delivery,
    // or rebound Work until every candidate that would reopen/rebind has an
    // adapter-reviewed installed version. Historical native-session locators
    // remain untouched even when this gate records a refreshed profile.
    for member in &mut members {
        let recoverable_candidate = !member.coordination_is_active()
            && !member.coordination_is_retired()
            && (matches!(
                member.status,
                MemberRunStatus::Stopped | MemberRunStatus::Idle | MemberRunStatus::Queued
            ) || (supervisor_current
                && matches!(
                    member.status,
                    MemberRunStatus::Completed | MemberRunStatus::Failed
                )));
        if !recoverable_candidate || member.is_external_interactive() {
            continue;
        }
        let expected = member.clone();
        let (mut profile, probe_error) = refreshed_team_member_provider_profile(member)?;
        let permission_ceiling = store
            .all_trust_agent_members()?
            .into_iter()
            .find(|candidate| candidate.id == member.agent_member_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "AGENT_IDENTITY_NOT_FOUND: MemberRun {} references missing AgentMember {}",
                    member.id, member.agent_member_id
                ))
            })?
            .permission_ceiling;
        let permission_ceiling =
            effective_member_permission_ceiling(store, permission_ceiling, &run, member)?;
        apply_permission_enforcement_to_profile(&mut profile, permission_ceiling)?;
        let resolution = resolve_provider_compatibility(store, &profile, probe_error.as_deref())?;
        let refusal = provider_compatibility_block_reason(
            member,
            &profile,
            &resolution,
            "recover, reopen, or rebound durable Work",
        );
        if apply_refreshed_provider_profile(member, profile) {
            store_conflict_as_usage(store.compare_and_append_member_run(&expected, member))?;
        }
        if let Some(refusal) = refusal {
            return Err(CliError::Usage(refusal));
        }
    }

    // ── Phase 1: orientation (read-only) ────────────────────────────
    if !json {
        println!(
            "team run: {}\tstatus={}\tobjective={}",
            run.id,
            serde_snake_label(&run.status),
            run.objective
        );
        println!(
            "supervisor: {}\tcurrent={}",
            supervisor
                .as_ref()
                .map(|s| format!("{} gen={}", s.supervisor_id, s.generation))
                .unwrap_or_else(|| "none".to_string()),
            supervisor_current
        );
        if !supervisor_current {
            let ready = works
                .iter()
                .filter(|work| work.is_claim_ready(&works))
                .count();
            let diagnosis = match supervisor.as_ref() {
                Some(lease) => supervisor_lease_live_diagnosis(lease).1,
                None => "no lease".to_string(),
            };
            eprintln!(
                "[WARNING] no live supervisor: {}. {} ready work(s) undelivered. Run: harness team-run start --id {}",
                diagnosis, ready, team_run_id
            );
        }
        let (open, done, cancelled) = (
            works.iter().filter(|w| !w.is_terminal()).count(),
            works.iter().filter(|w| w.is_accepted()).count(),
            works
                .iter()
                .filter(|w| w.resolution == Some(WorkResolution::Cancelled))
                .count(),
        );
        println!(
            "works: {} total  {} open  {} done  {} cancelled",
            works.len(),
            open,
            done,
            cancelled
        );
        for member in &members {
            let member_works: Vec<&Work> = works
                .iter()
                .filter(|w| w.owner_member_id.as_deref() == Some(member.agent_member_id.as_str()))
                .collect();
            let path = classify_member_recovery_path(member, supervisor_current);
            println!(
                "  {} ({}): status={} coordination={} recovery={} works={}",
                member.name,
                member.provider,
                serde_snake_label(&member.status),
                serde_snake_label(&member.coordination_status),
                serde_snake_label(&path),
                member_works.len()
            );
        }
    }

    // ── Gather recovery plan ─────────────────────────────────────────
    let recovery_plan: Vec<(&ProviderRuntimeProjection, MemberRecoveryPath)> = members
        .iter()
        .map(|member| {
            (
                member,
                classify_member_recovery_path(member, supervisor_current),
            )
        })
        .collect();

    let unrecoverable: Vec<_> = recovery_plan
        .iter()
        .filter(|(_, path)| matches!(path, MemberRecoveryPath::Terminal { .. }))
        .collect();
    if !unrecoverable.is_empty() {
        let blocked: Vec<String> = unrecoverable
            .iter()
            .map(|(member, path)| {
                let reason = match path {
                    MemberRecoveryPath::Terminal { reason } => reason.as_str(),
                    _ => "unknown",
                };
                format!("{}: {}", member.id, reason)
            })
            .collect();
        let msg = format!(
            "UNRECOVERABLE_MEMBERS: {} member(s) cannot be recovered: {}; run `harness team-run status --id {}` for details",
            blocked.len(),
            blocked.join("; "),
            team_run_id
        );
        if json {
            return Err(CliError::Usage(msg));
        }
        eprintln!("{msg}");
        std::process::exit(1);
    }

    // ── Phase 2: canonical delivery diagnostics ──────────────────────
    // Recovery never interprets or mutates legacy delivery projections.
    // claims. Canonical claim reconciliation belongs to the exact
    // NodeDaemon/AgentSession RuntimeCommand authority; this operator command
    // reports those facts and preserves the no-blind-replay fence.
    let canonical_claimed_deliveries = deliveries
        .iter()
        .filter(|delivery| {
            delivery.status == harness_core::agentfirm_api::WorkDeliveryStatus::Claimed
        })
        .count() as u64;
    let canonical_provider_received_deliveries = deliveries
        .iter()
        .filter(|delivery| {
            delivery.status == harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
        })
        .count() as u64;
    let reconciled = 0_u64;
    let now_str = now_string();

    // ── Phase 3: reopen compatible sessions ──────────────────────────
    let mut reopened = 0u64;
    let rebound = 0u64;
    let mut skipped = 0u64;
    let ledger = TeamRunLedger::without_supervisor(store, team_run_id);
    for (member, path) in &recovery_plan {
        match path {
            MemberRecoveryPath::AlreadyActive => {
                skipped += 1;
            }
            MemberRecoveryPath::ResumeCompatible => {
                // Reopen the member using the existing reopen path.
                let mut reopened_member = (*member).clone();
                reopened_member.runtime_generation =
                    reopened_member.runtime_generation.saturating_add(1);
                reopened_member.started_at = now_str.clone();
                reopened_member.coordination_status = MemberCoordinationStatus::Active;
                reopened_member.status = if reopened_member.is_external_interactive() {
                    MemberRunStatus::Idle
                } else {
                    MemberRunStatus::Queued
                };
                reopened_member.finished_at = None;
                reopened_member.last_event_at = Some(now_str.clone());
                store_conflict_as_usage(
                    store.compare_and_advance_member_run_generation(member, &reopened_member),
                )?;
                ledger.append_action(
                    &member.id,
                    "recovered",
                    MemberActionStatus::Succeeded,
                    "member recovered and reopened",
                    &format!(
                        "host: recovered after supervisor death; runtime generation {}",
                        reopened_member.runtime_generation
                    ),
                )?;
                ledger.fold_event(
                    TeamRunEventSourceKind::Host,
                    Some(member.id.clone()),
                    "member_run",
                    &member.id,
                    "recovered",
                    &format!(
                        "member {} recovered at runtime generation {}",
                        member.name, reopened_member.runtime_generation
                    ),
                )?;
                reopened += 1;
            }
            MemberRecoveryPath::RebindIncompatible { reason } => {
                // Runtime recovery advances only the MemberRun generation.
                // Work responsibility remains the stable AgentMember /
                // TeamMembership; the scheduler later admits the new exact
                // runtime through WorkExecutionBinding.
                let mut recovered_member = (*member).clone();
                recovered_member.runtime_generation =
                    recovered_member.runtime_generation.saturating_add(1);
                recovered_member.started_at = now_str.clone();
                recovered_member.coordination_status = MemberCoordinationStatus::Active;
                recovered_member.status = MemberRunStatus::Queued;
                recovered_member.finished_at = None;
                recovered_member.last_event_at = Some(now_str.clone());
                store_conflict_as_usage(
                    store.compare_and_advance_member_run_generation(member, &recovered_member),
                )?;
                ledger.append_action(
                    &member.id,
                    "recovered",
                    MemberActionStatus::Succeeded,
                    "member recovered with stable Work responsibility",
                    &format!(
                        "host: recovered after supervisor death; runtime generation {} ({reason})",
                        recovered_member.runtime_generation
                    ),
                )?;
                ledger.fold_event(
                    TeamRunEventSourceKind::Host,
                    Some(member.id.clone()),
                    "member_run",
                    &member.id,
                    "recovered",
                    &format!(
                        "member {} recovered at runtime generation {} with stable Work responsibility",
                        member.name, recovered_member.runtime_generation
                    ),
                )?;
                if !json {
                    println!(
                        "  {} ({}): recovered generation without mutating Work responsibility ({})",
                        member.name, member.provider, reason
                    );
                }
                reopened += 1;
            }
            MemberRecoveryPath::Terminal { .. } => {
                // Already checked above.
            }
        }
    }

    let supervisor_diagnosis = supervisor.as_ref().map(|lease| {
        let (live, diagnosis) = supervisor_lease_live_diagnosis(lease);
        let heartbeat_age_s = current_unix_ms_u64().saturating_sub(lease.heartbeat_unix_ms) / 1000;
        serde_json::json!({
            "live": live,
            "diagnosis": diagnosis,
            "owner_process_id": lease.owner_process_id,
            "owner_pid_alive": pid_exists_libc(lease.owner_process_id),
            "heartbeat_unix_ms": lease.heartbeat_unix_ms,
            "heartbeat_age_s": heartbeat_age_s,
            "expires_unix_ms": lease.expires_unix_ms,
        })
    });
    let report = serde_json::json!({
        "team_run_id": team_run_id,
        "status": serde_snake_label(&run.status),
        "supervisor_current": supervisor_current,
        "supervisor_diagnosis": supervisor_diagnosis,
        "members": members.len(),
        "works_total": works.len(),
        "reconciled_deliveries": reconciled,
        "canonical_claimed_deliveries": canonical_claimed_deliveries,
        "canonical_provider_received_deliveries": canonical_provider_received_deliveries,
        "reopened": reopened,
        "rebound_works": rebound,
        "skipped": skipped,
    });
    if !json {
        println!(
            "recovery complete: reopened={} rebound_works={} reconciled_deliveries={} skipped={}",
            reopened, rebound, reconciled, skipped
        );
    }
    Ok(report)
}

/// Recover a running attempt only after the operator has independently stopped
/// every provider process. This is not cooperative interruption: the explicit
/// CLI flag is an auditable attestation used when the foreground orchestrator
/// disappeared before it could journal terminal state.
pub(super) fn recover_interrupted_team_run(
    store: &HarnessStore,
    team_run_id: &str,
    reason: &str,
    cancelled_by: &str,
) -> CliResult<AgentTeamRun> {
    if reason.trim().is_empty() || cancelled_by.trim().is_empty() {
        return Err(CliError::Usage(
            "interrupted recovery requires non-empty --reason and --cancelled-by".to_string(),
        ));
    }
    let current = latest_team_run(store, team_run_id)?;
    if current.status != TeamRunStatus::Running {
        return Err(CliError::Usage(format!(
            "--confirm-provider-stopped is only valid for a running team run; {} is {}",
            current.id,
            serde_snake_label(&current.status)
        )));
    }

    let ledger = TeamRunLedger::without_supervisor(store, team_run_id);
    let members = latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == team_run_id)
        .collect::<Vec<_>>();
    for member in members {
        if matches!(
            member.status,
            MemberRunStatus::Completed
                | MemberRunStatus::Failed
                | MemberRunStatus::Stopped
                | MemberRunStatus::Blocked
        ) {
            continue;
        }
        let mut stopped = member.clone();
        stopped.coordination_status = MemberCoordinationStatus::Closed;
        stopped.status = MemberRunStatus::Stopped;
        stopped.last_event_at = Some(now_string());
        stopped.finished_at = Some(now_string());
        ledger.save_member_run(&member, &stopped)?;
        ledger.append_action(
            &member.id,
            "interrupted",
            MemberActionStatus::Cancelled,
            "provider execution stopped",
            reason,
        )?;
        ledger.fold_event(
            TeamRunEventSourceKind::Host,
            Some(member.id.clone()),
            "member_run",
            &member.id,
            "updated",
            &format!(
                "member {} marked stopped after provider interruption",
                member.name
            ),
        )?;
    }

    let mut cancelled = current.clone();
    cancelled.status = TeamRunStatus::Cancelled;
    cancelled.updated_at = now_string();
    store_conflict_as_usage(store.compare_and_append_team_run_lifecycle(&current, &cancelled))?;
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        None,
        "team_run",
        team_run_id,
        "updated",
        &format!(
            "team run recovered as cancelled after {cancelled_by} confirmed provider processes stopped: {reason}"
        ),
    )?;
    Ok(cancelled)
}

/// Parse a team message kind from its snake_case wire name.
#[cfg(test)]
pub(super) fn parse_team_message_kind(s: &str) -> CliResult<ProviderDispatchIntent> {
    let kind = serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown provider dispatch intent `{s}` (message|control)"
        ))
    })?;
    if !matches!(
        kind,
        ProviderDispatchIntent::Message | ProviderDispatchIntent::Control
    ) {
        return Err(CliError::Usage(
            "provider interaction intents are runtime-owned; use message|control".to_string(),
        ));
    }
    Ok(kind)
}

/// Parse an explicit team message response intent from its snake_case wire
/// name (HTTP API and MCP tool surface; the CLI spells the same two values as
/// --response-required and --informational).
#[cfg(any())]
pub(super) fn parse_team_message_response_intent(s: &str) -> CliResult<ProviderResponseIntent> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown team message response intent `{s}` (informational|response_required)"
        ))
    })
}

pub(super) fn team_message_kind_label(kind: &ProviderDispatchIntent) -> &'static str {
    match kind {
        ProviderDispatchIntent::Message => "message",
        ProviderDispatchIntent::Control => "control",
        ProviderDispatchIntent::ProviderInteractionRequest => "provider_interaction_request",
        ProviderDispatchIntent::ProviderInteractionResponse => "provider_interaction_response",
    }
}

/// The snake_case wire label of a serde `rename_all = "snake_case"` enum, for
/// human-readable CLI output.
pub(super) fn serde_snake_label<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(label)) => label,
        _ => "unknown".to_string(),
    }
}

pub(super) fn parse_work_claim_mode(value: &str) -> CliResult<WorkClaimMode> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown Work claim mode `{value}` (host_assign|team_claim)"
        ))
    })
}

pub(super) fn parse_work_priority(value: &str) -> CliResult<WorkPriority> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown Work priority `{value}` (low|normal|high|urgent)"
        ))
    })
}

pub(super) fn work_priority_rank(priority: WorkPriority) -> u8 {
    match priority {
        WorkPriority::Low => 0,
        WorkPriority::Normal => 1,
        WorkPriority::High => 2,
        WorkPriority::Urgent => 3,
    }
}

pub(super) fn parse_work_phase(value: &str) -> CliResult<WorkPhase> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown Work phase `{value}` (open|active|review|closed)"
        ))
    })
}

pub(super) fn parse_work_condition(value: &str) -> CliResult<WorkCondition> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown Work condition `{value}` (normal|blocked|on_hold)"
        ))
    })
}

pub(super) fn parse_work_resolution(value: &str) -> CliResult<WorkResolution> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown Work resolution `{value}` (accepted|cancelled|failed)"
        ))
    })
}

pub(super) fn work_lifecycle_label(work: &Work) -> String {
    match (work.phase, work.condition, work.resolution) {
        (_, WorkCondition::Blocked, _) => "blocked".to_string(),
        (_, WorkCondition::OnHold, _) => "on_hold".to_string(),
        (WorkPhase::Closed, _, Some(resolution)) => serde_snake_label(&resolution),
        (phase, _, _) => serde_snake_label(&phase),
    }
}

pub(super) fn required_work_version(args: &[String]) -> CliResult<u64> {
    required(args, "--expected-version")?
        .parse::<u64>()
        .map_err(|_| CliError::Usage("--expected-version must be an integer".to_string()))
}

pub(super) fn work_causation(args: &[String]) -> Option<WorkCausationRef> {
    value(args, "--caused-by-message-id").map(|id| WorkCausationRef {
        kind: "team_message".to_string(),
        id,
    })
}

pub(super) fn host_work_context(
    store: &HarnessStore,
    team_run_id: &str,
    args: &[String],
) -> CliResult<WorkCommandContext> {
    let host_actor = store.exact_team_run_host_actor(team_run_id)?;
    Ok(WorkCommandContext {
        event_id: value(args, "--event-id").unwrap_or_else(|| generated_id("work-event")),
        performed_by_actor: TeamActorRef {
            display_name: None,
            authn_source: Some("local_cli_exact_team_host".to_string()),
            ..host_actor.clone()
        },
        authority_actor: Some(host_actor),
        causation_ref: work_causation(args),
        idempotency_key: value(args, "--idempotency-key")
            .unwrap_or_else(|| generated_id("work-command")),
        created_at: now_string(),
        duplicate_ok: has_flag(args, "--duplicate-ok"),
    })
}

pub(super) fn host_work_context_for_work(
    store: &HarnessStore,
    work_id: &str,
    args: &[String],
) -> CliResult<WorkCommandContext> {
    let work = store
        .latest_works()?
        .into_iter()
        .find(|work| work.id == work_id)
        .ok_or_else(|| CliError::Usage(format!("Work not found: {work_id}")))?;
    host_work_context(store, &work.team_run_id, args)
}

pub(super) fn migration_host_work_context(args: &[String]) -> WorkCommandContext {
    WorkCommandContext {
        event_id: value(args, "--event-id").unwrap_or_else(|| generated_id("work-event")),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Host,
            id: value(args, "--actor").unwrap_or_else(|| "migration-host".to_string()),
            display_name: None,
            authn_source: Some("local_cli_migration".to_string()),
        },
        authority_actor: None,
        causation_ref: work_causation(args),
        idempotency_key: value(args, "--idempotency-key")
            .unwrap_or_else(|| generated_id("work-command")),
        created_at: now_string(),
        duplicate_ok: has_flag(args, "--duplicate-ok"),
    }
}

pub(super) fn member_work_context(
    args: &[String],
    team_run_id: &str,
    member_run_id: &str,
) -> CliResult<WorkCommandContext> {
    let bound_member = env::var("FIRM_MEMBER_RUN_ID")
        .or_else(|_| env::var("HARNESS_MEMBER_RUN_ID"))
        .map_err(|_| {
            CliError::Usage(
                "member Work commands require the bound FIRM_MEMBER_RUN_ID runtime environment"
                    .to_string(),
            )
        })?;
    if bound_member != member_run_id {
        return Err(CliError::Usage(format!(
            "bound ProviderRuntimeProjection is {bound_member}, not {member_run_id}"
        )));
    }
    if let Ok(bound_team) =
        env::var("FIRM_TEAM_RUN_ID").or_else(|_| env::var("HARNESS_TEAM_RUN_ID"))
    {
        if bound_team != team_run_id {
            return Err(CliError::Usage(format!(
                "bound TeamRun is {bound_team}, not {team_run_id}"
            )));
        }
    }
    Ok(WorkCommandContext {
        event_id: value(args, "--event-id").unwrap_or_else(|| generated_id("work-event")),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.to_string(),
            display_name: None,
            authn_source: Some("bound_runtime_env".to_string()),
        },
        authority_actor: None,
        causation_ref: work_causation(args),
        idempotency_key: value(args, "--idempotency-key")
            .unwrap_or_else(|| generated_id("work-command")),
        created_at: now_string(),
        duplicate_ok: false,
    })
}

pub(super) fn roll_up_target_work_delegations(
    store: &HarnessStore,
    work: &Work,
    args: &[String],
) -> CliResult<Vec<WorkDelegation>> {
    let mut context = host_work_context(store, &work.team_run_id, args)?;
    context.event_id = generated_id("delegation-rollup");
    context.idempotency_key = format!("delegation-rollup:{}:{}", work.id, work.version);
    Ok(store.transition_work_and_roll_up_delegation(&work.id, context)?)
}

// ---------------------------------------------------------------------------
// Decision-shaped board reads (issue #305).
//
// The full `work list`/`work show` JSON dump is the right shape for a member
// picking up its own Work, but it is the wrong shape for a Host that only
// needs to decide what to do next: measured on the first live work-board run,
// those two reads averaged 12.6K chars each across 22 calls (277K chars,
// 19.4% of the Host's entire tool output) to answer questions that need only
// a handful of numbers -- how many Works are unassigned/ready, how many
// members are idle, how many submissions are waiting on review. `--brief`,
// `--since`, and `board-summary` below are additive projections over the same
// authoritative store reads; the full JSON array remains the default.
// ---------------------------------------------------------------------------

/// One `--brief` line: `<work-id>  <status>  <owner-agent-member-id|unassigned>
/// v<version>  <title>`, title hard-truncated to 60 chars (by `char`, not
/// byte, so multibyte titles never split mid-character). Plain text, no JSON
/// wrapper -- this is the compact projection `work list --brief` prints one
/// of per Work.
pub(super) fn format_work_brief_line(work: &Work) -> String {
    let owner = work.owner_member_id.as_deref().unwrap_or("unassigned");
    let title: String = work.title.chars().take(60).collect();
    format!(
        "{}  {}  {}  v{}  {}",
        work.id,
        work_lifecycle_label(work),
        owner,
        work.version,
        title
    )
}

/// Per-run monotonic cursor for `work list --since`: each Work id mapped to
/// the 1-based position of its most recent [`WorkOperation`] within this team
/// run's operations, numbered in store append (causal) order.
///
/// `work_operations.jsonl` is the sole mutation path for every Work row and
/// every append is serialized under the store's write lock, so this order is
/// a genuine per-run total order -- the "monotonic per-run operation
/// sequence" a delta cursor needs. Two alternatives were considered and
/// rejected: `Work::version` restarts at 1 for every Work, so it is not
/// comparable across Works in one run; `updated_at` is millisecond-resolution
/// and can tie under fast scripted mutation (concurrent or same-millisecond
/// writes), which would make "changed after" ambiguous. `--since <cursor>`
/// therefore means "Works whose latest WorkOperation sorts after `cursor` in
/// this run's append order", and a `list` call made with `--since` reports
/// the new `next_since` watermark so a Host wake->decide->act loop can chain
/// calls without redundantly re-reading unchanged Works.
pub(super) fn work_operation_cursors(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<BTreeMap<String, u64>> {
    let mut cursors = BTreeMap::new();
    for (index, operation) in store
        .work_operations()?
        .into_iter()
        .filter(|operation| operation.event.team_run_id == team_run_id)
        .enumerate()
    {
        cursors.insert(operation.work.id, (index + 1) as u64);
    }
    Ok(cursors)
}

/// Bucket one member into the `board-summary` per-member line. Reads BOTH
/// signals the summary promises: owned Work content and ProviderRuntimeProjection process
/// state.
///
/// A member owning any Work in `review` is `awaiting-review` -- a submission
/// is waiting on the Host's accept/request-changes decision -- regardless of
/// process state; that decision is the whole reason this bucket exists.
/// Otherwise a member owning an `in_progress` Work, or whose MemberRunStatus
/// is `Running`/`Starting`, is `working`. Everything else (idle, queued,
/// waiting on a provider interaction, disconnected, blocked, or terminal) is
/// `idle`: none of those states are "a Host decision is pending on this
/// member" and the summary has only three buckets to spend.
pub(super) fn member_board_state<'a>(
    member: &ProviderRuntimeProjection,
    owned_works: impl Iterator<Item = &'a Work>,
) -> &'static str {
    let mut awaiting_review = false;
    let mut owns_in_progress = false;
    for work in owned_works {
        awaiting_review |= work.phase == WorkPhase::Review;
        owns_in_progress |= work.phase == WorkPhase::Active;
    }
    if awaiting_review {
        "awaiting-review"
    } else if owns_in_progress
        || matches!(
            member.status,
            MemberRunStatus::Running | MemberRunStatus::Starting
        )
    {
        "working"
    } else {
        "idle"
    }
}

/// `team-run board-summary` -- a single plain-text projection built from one
/// `latest_works` read and one `latest_member_runs_in_append_order` read:
/// counts by lifecycle axis, assigned vs unassigned, claim-ready count (reusing
/// [`Work::is_claim_ready`], the same readiness rule the claim path
/// enforces), and one `member_board_state` line per active member. Contract:
/// the whole string stays under 500 chars for an ordinary run so a Host can
/// afford it on every wake (see the module-level comment above for the
/// measured cost this replaces). There is no `--json` form: the entire point
/// is a bounded plain-text read, and a JSON wrapper would tax the same
/// budget it exists to protect.
pub(super) fn team_run_board_summary_text(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<String> {
    let run = latest_team_run(store, team_run_id)?;
    team_run_execution_space_id(store, &run)?;
    let works: Vec<Work> = store
        .latest_works()?
        .into_iter()
        .filter(|work| work.team_run_id == team_run_id)
        .collect();

    let mut open = 0u64;
    let mut active = 0u64;
    let mut blocked = 0u64;
    let mut review = 0u64;
    let mut accepted = 0u64;
    let mut cancelled = 0u64;
    let mut assigned = 0u64;
    let mut unassigned = 0u64;
    for work in &works {
        match work.phase {
            WorkPhase::Open => open += 1,
            WorkPhase::Active => active += 1,
            WorkPhase::Review => review += 1,
            WorkPhase::Closed => {}
        }
        blocked += u64::from(work.condition == WorkCondition::Blocked);
        accepted += u64::from(work.resolution == Some(WorkResolution::Accepted));
        cancelled += u64::from(work.resolution == Some(WorkResolution::Cancelled));
        if work.owner_member_id.is_some() {
            assigned += 1;
        } else {
            unassigned += 1;
        }
    }
    let ready = works
        .iter()
        .filter(|work| work.is_claim_ready(&works))
        .count();

    let members: Vec<ProviderRuntimeProjection> = latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == team_run_id && member.coordination_is_active())
        .collect();

    let mut lines = vec![
        format!(
            "open={open} active={active} blocked={blocked} review={review} accepted={accepted} cancelled={cancelled}"
        ),
        format!("assigned={assigned} unassigned={unassigned} ready={ready}"),
    ];
    for member in &members {
        let owned = works.iter().filter(|work| {
            work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str())
        });
        lines.push(format!(
            "{}: {}",
            member.name,
            member_board_state(member, owned)
        ));
    }
    let supervisor = store.latest_team_supervisor_lease(team_run_id)?;
    match supervisor {
        Some(ref lease) => {
            let supervisor_current = is_supervisor_current(lease);
            let pid_alive = pid_exists_libc(lease.owner_process_id);
            let heartbeat_age_s =
                current_unix_ms_u64().saturating_sub(lease.heartbeat_unix_ms) / 1000;
            lines.push(format!(
                "supervisor: gen={} pid={} alive={} hb_age={}s current={}",
                lease.generation,
                lease.owner_process_id,
                pid_alive,
                heartbeat_age_s,
                supervisor_current
            ));
            if !supervisor_current || !pid_alive {
                let ready = works.iter().filter(|w| w.is_claim_ready(&works)).count();
                lines.push(format!(
                    "[WARNING] no live supervisor: {} ready work(s) undelivered. Run: harness team-run start --id {}",
                    ready, team_run_id
                ));
            }
        }
        None => {
            lines.push("supervisor: none".to_string());
            let ready = works.iter().filter(|w| w.is_claim_ready(&works)).count();
            lines.push(format!(
                "[WARNING] no supervisor lease. {} ready work(s) undelivered. Run: harness team-run start --id {}",
                ready, team_run_id
            ));
        }
    }
    Ok(lines.join("\n"))
}

/// Parse `owner/repo#N` into `(owner, repo, number)` for the `--github-issue`
/// and `--github-pr` flags. `N` must be a positive integer.
pub(super) fn parse_github_ref(raw: &str) -> CliResult<(String, String, u64)> {
    let (repo_ref, number_ref) = raw.rsplit_once('#').ok_or_else(|| {
        CliError::Usage(format!(
            "--github-issue/--github-pr expects owner/repo#N, got {raw:?} (e.g. octocat/hello-world#42)"
        ))
    })?;
    let number = number_ref
        .parse::<u64>()
        .map_err(|_| CliError::Usage(format!("invalid GitHub issue/PR number in {raw:?}")))?;
    if number == 0 {
        return Err(CliError::Usage(format!(
            "GitHub issue/PR number must be positive, got {raw:?}"
        )));
    }
    let (owner, repo) = repo_ref.split_once('/').ok_or_else(|| {
        CliError::Usage(format!(
            "--github-issue/--github-pr expects owner/repo#N, got {raw:?} (missing '/')"
        ))
    })?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return Err(CliError::Usage(format!(
            "--github-issue/--github-pr expects owner/repo#N, got {raw:?}"
        )));
    }
    Ok((owner.to_string(), repo.to_string(), number))
}

/// Run `gh <args...>` and parse its stdout as JSON.
///
/// The `gh` CLI must be installed and authenticated; any failure is surfaced
/// with the underlying `gh` stderr so a member can diagnose (missing binary,
/// expired auth, network) without guessing.
pub(super) fn gh_json(args: &[&str]) -> CliResult<serde_json::Value> {
    let output = Command::new("gh").args(args).output().map_err(|error| {
        CliError::Usage(format!(
            "could not run `gh {}`: {error} (is the GitHub CLI installed?)",
            args.join(" ")
        ))
    })?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "`gh {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        CliError::Usage(format!(
            "`gh {}` returned non-JSON output: {error}",
            args.join(" ")
        ))
    })
}

/// Best-effort PR CI summary from `gh pr checks`. `gh pr view` already proved
/// the link and auth above, so "no checks reported" (e.g. a PR merged without
/// CI) yields `(None, None)` rather than failing the whole submit.
pub(super) fn github_pr_ci_summary(
    owner: &str,
    repo: &str,
    number: u64,
) -> (Option<String>, Option<String>) {
    let Ok(value) = gh_json(&[
        "pr",
        "checks",
        &number.to_string(),
        "--repo",
        &format!("{owner}/{repo}"),
        "--json",
        "name,state,link",
    ]) else {
        return (None, None);
    };
    let Some(checks) = value.as_array() else {
        return (None, None);
    };
    if checks.is_empty() {
        return (None, None);
    }
    let mut saw_pending = false;
    for check in checks {
        let state = check
            .get("state")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if state == "FAILURE" {
            let ci_url = check
                .get("link")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            return (Some("failure".to_string()), Some(ci_url));
        }
        if state == "PENDING" || state == "IN_PROGRESS" || state == "STARTUP_FAILURE" {
            saw_pending = true;
        }
    }
    if saw_pending {
        return (
            Some("pending".to_string()),
            checks
                .first()
                .and_then(|check| check.get("link"))
                .and_then(|value| value.as_str())
                .map(str::to_string),
        );
    }
    (
        Some("success".to_string()),
        checks
            .first()
            .and_then(|check| check.get("link"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    )
}

/// Build the [`GitHubLink`] snapshot for `work create --github-issue
/// owner/repo#N`, fetching the live issue state from the GitHub API.
pub(super) fn github_issue_link(raw: &str) -> CliResult<GitHubLink> {
    let (owner, repo, number) = parse_github_ref(raw)?;
    let value = gh_json(&[
        "issue",
        "view",
        &number.to_string(),
        "--repo",
        &format!("{owner}/{repo}"),
        "--json",
        "state,url",
    ])?;
    Ok(GitHubLink {
        kind: GitHubLinkKind::Issue,
        owner,
        repo,
        number,
        url: value
            .get("url")
            .and_then(|value| value.as_str())
            .ok_or_else(|| CliError::Usage("gh issue view returned no url".to_string()))?
            .to_string(),
        status: value
            .get("state")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        ci_status: None,
        ci_url: None,
    })
}

/// Build the [`GitHubLink`] snapshot for `work submit --github-pr
/// owner/repo#N`: PR state from `gh pr view` plus a best-effort CI summary
/// from `gh pr checks`.
pub(super) fn github_pr_link(raw: &str) -> CliResult<GitHubLink> {
    let (owner, repo, number) = parse_github_ref(raw)?;
    let value = gh_json(&[
        "pr",
        "view",
        &number.to_string(),
        "--repo",
        &format!("{owner}/{repo}"),
        "--json",
        "state,url",
    ])?;
    let (ci_status, ci_url) = github_pr_ci_summary(&owner, &repo, number);
    Ok(GitHubLink {
        kind: GitHubLinkKind::PullRequest,
        owner,
        repo,
        number,
        url: value
            .get("url")
            .and_then(|value| value.as_str())
            .ok_or_else(|| CliError::Usage("gh pr view returned no url".to_string()))?
            .to_string(),
        status: value
            .get("state")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        ci_status,
        ci_url,
    })
}

/// Poll interval for the supervisor loop's GitHub CI refresh (issue #369
/// Phase 2). Long enough that a team run does not hammer the GitHub API;
/// the poll is best-effort and skipped entirely when `gh` is unavailable.
pub(super) const GITHUB_CI_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// What one `poll-github-ci` pass observed (issue #369 Phase 2).
#[derive(Default)]
pub(crate) struct GithubPollSummary {
    pub works_checked: usize,
    pub links_refreshed: usize,
    pub auto_submitted: Vec<String>,
    /// Work(s) whose linked PR merged but whose CI was `failure`; left for the
    /// Host to decide instead of auto-submitting a red submission.
    pub blocked_on_failure: Vec<String>,
    /// Work(s) whose declared gates all pass after this poll (meaning they are
    /// ready for `work accept`).
    pub gate_ready: Vec<String>,
    pub gh_unavailable: bool,
}

impl GithubPollSummary {
    pub fn is_noop(&self) -> bool {
        self.links_refreshed == 0
            && self.auto_submitted.is_empty()
            && self.blocked_on_failure.is_empty()
            && self.gate_ready.is_empty()
    }
}

/// Refresh the stored GitHub linkage snapshot for every Work on the run that
/// carries a pull-request link (issue #369 Phase 2): the daemon calls this on
/// `GITHUB_CI_POLL_INTERVAL`, and `team-run work poll-github-ci` triggers it
/// on demand.
///
/// - CI status/`ci_url` are re-fetched from `gh pr checks` and persisted only
///   when they changed, so a steady-state poll never churns Work versions.
/// - When a linked PR is observed `MERGED` and the Work is `in_progress` (and
///   not on red CI), the Work is auto-submitted to `review`; Host acceptance
///   still moves it to `done`.
/// - `gh` missing/unauthenticated is a soft skip: stored snapshots are kept.
pub(crate) fn poll_team_run_github_linkages(
    store: &HarnessStore,
    run_id: &str,
) -> CliResult<GithubPollSummary> {
    let mut summary = GithubPollSummary::default();
    if !gh_available() {
        summary.gh_unavailable = true;
        return Ok(summary);
    }
    let works = store.latest_works()?;
    for work in works {
        if work.team_run_id != run_id || work.is_terminal() {
            continue;
        }
        let pr_links = work
            .github_links
            .iter()
            .filter(|link| link.kind == GitHubLinkKind::PullRequest)
            .cloned()
            .collect::<Vec<_>>();
        if pr_links.is_empty() {
            continue;
        }
        summary.works_checked += 1;
        let mut refreshed_links = work.github_links.clone();
        let mut changed = false;
        for link in &pr_links {
            let raw = format!("{}/{}#{}", link.owner, link.repo, link.number);
            let Ok(fresh) = github_pr_link(&raw) else {
                // gh call failed (network/auth/unknown PR): keep the snapshot.
                continue;
            };
            if let Some(stored) = refreshed_links.iter_mut().find(|candidate| {
                candidate.kind == fresh.kind
                    && candidate.owner == fresh.owner
                    && candidate.repo == fresh.repo
                    && candidate.number == fresh.number
            }) {
                if *stored != fresh {
                    *stored = fresh.clone();
                    changed = true;
                    summary.links_refreshed += 1;
                }
            } else {
                refreshed_links.push(fresh.clone());
                changed = true;
                summary.links_refreshed += 1;
            }
            // A merge observation may auto-submit even when the link fields
            // themselves changed, so evaluate against the fresh link.
            let merged_and_green = fresh.status.as_deref() == Some("MERGED")
                && fresh.ci_status.as_deref() != Some("failure");
            if merged_and_green && work.phase == WorkPhase::Active {
                let context = github_poll_host_context(store, run_id, &work.id)?;
                let result = format!(
                    "auto-submitted by GitHub merge observation: PR {}/{}#{} merged; CI: {}",
                    fresh.owner,
                    fresh.repo,
                    fresh.number,
                    fresh.ci_status.as_deref().unwrap_or("unknown")
                );
                store
                    .submit_work_on_pr_merge(
                        &work.id,
                        work.version,
                        &result,
                        refreshed_links.clone(),
                        context,
                    )
                    .map_err(|error| {
                        CliError::Usage(format!("github poll auto-submit failed: {error}"))
                    })?;
                summary.auto_submitted.push(work.id.clone());
                changed = false; // transition already persisted the snapshot
                break;
            }
            if fresh.status.as_deref() == Some("MERGED")
                && fresh.ci_status.as_deref() == Some("failure")
                && work.phase == WorkPhase::Active
                && !summary.blocked_on_failure.contains(&work.id)
            {
                summary.blocked_on_failure.push(work.id.clone());
            }
        }
        if changed {
            let context = github_poll_host_context(store, run_id, &work.id)?;
            store
                .update_work_github_links(&work.id, work.version, refreshed_links, context)
                .map_err(|error| {
                    CliError::Usage(format!("github poll link update failed: {error}"))
                })?;
        }
    }
    Ok(summary)
}

/// `gh` binary presence check for the poll; auth is validated per call.
pub(super) fn gh_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Exact Host-authority context for daemon/poll store mutations (issue #369
/// Phase 2). Each operation gets its own generated idempotency key.
pub(super) fn github_poll_host_context(
    store: &HarnessStore,
    run_id: &str,
    work_id: &str,
) -> CliResult<WorkCommandContext> {
    let host_actor = store.exact_team_run_host_actor(run_id)?;
    Ok(WorkCommandContext {
        event_id: generated_id("github-poll-event"),
        performed_by_actor: TeamActorRef {
            display_name: Some(format!("GitHub CI poll for {run_id}")),
            authn_source: Some("supervisor_daemon".to_string()),
            ..host_actor.clone()
        },
        authority_actor: Some(host_actor),
        causation_ref: None,
        idempotency_key: generated_id(&format!("github-poll-{work_id}")),
        created_at: now_string(),
        duplicate_ok: false,
    })
}
