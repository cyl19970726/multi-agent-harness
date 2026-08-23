use super::*;

pub(super) fn steer_team_member_value(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let content = required_json_string(body, "content")?;
    let requested_by =
        optional_json_string(body, "requested_by")?.unwrap_or_else(|| "operator".to_string());
    let result = dispatch_live_member_control(
        store,
        LiveMemberControlRequest::Steer {
            team_run_id: team_run_id.to_string(),
            member_run_id: member_run_id.to_string(),
            content: content.clone(),
            requested_by: requested_by.clone(),
        },
    )?;
    let correlation_id = json_string(&result, "correlation_id");
    let causation_id = json_string(&result, "causation_id");
    let sender = TeamActorRef {
        kind: TeamActorKind::Operator,
        id: requested_by,
        display_name: None,
        authn_source: Some("http_control".to_string()),
    };
    let message = prepare_team_message_as(
        store,
        team_run_id,
        &sender,
        vec![member_run_id.to_string()],
        ProviderDispatchIntent::Control,
        &content,
        None,
        correlation_id,
        causation_id,
        TeamMessageDeliveryMode::InjectDelivered,
        None,
    )?;
    let message = publish_team_message(store, &sender, message)?;
    Ok(serde_json::json!({"control": result, "message": message}))
}

pub(super) fn interrupt_team_member_value(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let requested_by =
        optional_json_string(body, "requested_by")?.unwrap_or_else(|| "operator".to_string());
    let reason = optional_json_string(body, "reason")?
        .unwrap_or_else(|| "operator requested interruption".to_string());
    require_member_interrupt_capability(store, team_run_id, member_run_id)?;
    cancel_unanswered_provider_messages(store, team_run_id, member_run_id, &requested_by, &reason)?;
    dispatch_live_member_control(
        store,
        LiveMemberControlRequest::Interrupt {
            team_run_id: team_run_id.to_string(),
            member_run_id: member_run_id.to_string(),
            reason,
            requested_by,
        },
    )
}

pub(super) fn require_member_interrupt_capability(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
) -> CliResult<()> {
    let member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    if member.team_run_id != team_run_id {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    let profile = member.provider_profile.as_ref().ok_or_else(|| {
        CliError::Usage(format!(
            "member run {member_run_id} has no provider capability snapshot"
        ))
    })?;
    if profile.compatibility_status != ProviderCompatibilityStatus::Current {
        let version = profile.provider_version.as_deref().unwrap_or("unknown");
        return Err(CliError::Usage(format!(
            "Interrupt unavailable: {} {} in {} is not adapter-reviewed for this control",
            profile.provider, version, profile.execution_mode
        )));
    }
    if !has_active_verified_provider_capability(profile, "interrupt_current_cycle") {
        let version = profile.provider_version.as_deref().unwrap_or("unknown");
        return Err(CliError::Usage(format!(
            "Interrupt unavailable: {} {} in {} has no active verified interrupt binding",
            profile.provider, version, profile.execution_mode
        )));
    }
    Ok(())
}

pub(super) fn cancel_unanswered_provider_messages(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    requested_by: &str,
    reason: &str,
) -> CliResult<()> {
    let member_identity_id = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id && member.team_run_id == team_run_id)
        .map(|member| member.agent_member_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    // Drive the exact Host MessageDelivery through claim -> provider receipt -> ACK
    // without fabricating an answer; the blocked provider callback observes
    // the ACK and returns a native cancellation. The retired TeamMessage
    // delivery ledger is never mutated here.
    let already_cancelled = current_team_run_events_in_append_order(store, team_run_id)?
        .into_iter()
        .filter(|event| event.entity_type == "message" && event.operation == "cancelled")
        .map(|event| event.entity_id)
        .collect::<HashSet<_>>();
    for request in canonical_team_messages_for_run(store, team_run_id)?
        .into_iter()
        .filter(|message| {
            message.team_run_id == team_run_id
                && message.kind == ProviderDispatchIntent::ProviderInteractionRequest
                && (message.sender_runtime_id == member_run_id
                    || message.sender_runtime_id == member_identity_id)
        })
        .filter(|message| !already_cancelled.contains(&message.id))
    {
        acknowledge_provider_request_as_host(store, team_run_id, &request)?;
        append_team_run_event(
            store,
            team_run_id,
            0,
            TeamRunEventSourceKind::Host,
            Some(member_run_id.to_string()),
            "message",
            &request.id,
            "cancelled",
            &format!("provider question cancelled by {requested_by}: {reason}"),
        )?;
    }
    Ok(())
}

pub(super) fn close_team_member_value(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let requested_by =
        optional_json_string(body, "requested_by")?.unwrap_or_else(|| "host".to_string());
    let reason =
        optional_json_string(body, "reason")?.unwrap_or_else(|| "Host closed member".to_string());
    let run = latest_team_run(store, team_run_id)?;
    team_run_execution_space_id(store, &run)?;
    if !run.member_run_ids.iter().any(|id| id == member_run_id) {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    let member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    let external_interactive = member.is_external_interactive();
    if member.coordination_is_retired() {
        return Ok(serde_json::json!({
            "member_run_id": member.id,
            "status": serde_snake_label(&member.status),
            "coordination_status": "retired",
            "runtime": if external_interactive { "external_unmanaged" } else { "not_live" },
            "runtime_effect": "already_terminal",
            "coordination_effect": "already_retired",
            "idempotent": true,
        }));
    }
    if member.coordination_is_closed() {
        return Ok(serde_json::json!({
            "member_run_id": member.id,
            "status": serde_snake_label(&member.status),
            "coordination_status": "closed",
            "runtime": if external_interactive { "external_unmanaged" } else if member.status == MemberRunStatus::Stopped { "not_live" } else { "closing" },
            "runtime_effect": if member.status == MemberRunStatus::Stopped { "already_terminal" } else { "close_pending" },
            "coordination_effect": "already_closed",
            "idempotent": true,
        }));
    }
    if matches!(
        member.status,
        MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
    ) {
        cancel_unanswered_provider_messages(
            store,
            team_run_id,
            member_run_id,
            &requested_by,
            &reason,
        )?;
        if let Some(close) = pending_member_close(store, member_run_id)? {
            store_conflict_as_usage(store.complete_team_member_close(
                team_run_id,
                member_run_id,
                &close.id,
                &now_string(),
            ))?;
        }
        let member = mark_member_coordination_closed(store, team_run_id, member_run_id)?;
        let ledger = TeamRunLedger::without_supervisor(store, team_run_id);
        ledger.append_action(
            &member.id,
            "closed",
            MemberActionStatus::Succeeded,
            "member coordination closed after terminal runtime",
            &format!("{requested_by}: {reason}"),
        )?;
        ledger.fold_event(
            TeamRunEventSourceKind::Host,
            Some(member.id.clone()),
            "member_run",
            &member.id,
            "closed",
            &format!("member {} coordination closed", member.name),
        )?;
        return Ok(serde_json::json!({
            "member_run_id": member.id,
            "status": serde_snake_label(&member.status),
            "coordination_status": serde_snake_label(&member.coordination_status),
            "runtime": if external_interactive { "external_unmanaged" } else { "not_live" },
            "runtime_effect": if external_interactive { "none" } else { "already_terminal" },
            "coordination_effect": "already_closed",
            "idempotent": true,
        }));
    }

    let close = if member.is_external_interactive() {
        latch_member_close(store, team_run_id, member_run_id, &requested_by, &reason)?
    } else {
        if store
            .latest_team_supervisor_lease(team_run_id)?
            .filter(is_supervisor_current)
            .is_some()
        {
            return dispatch_live_member_control(
                store,
                LiveMemberControlRequest::Close {
                    team_run_id: team_run_id.to_string(),
                    member_run_id: member_run_id.to_string(),
                    reason,
                    requested_by,
                },
            );
        }
        return Err(CliError::Usage(
            "RUNTIME_COMMAND_RECOVERY_REQUIRED: managed AgentSession has no current provider-loop authority; reconcile its RuntimeCommand/session state before Close"
                .into(),
        ));
    };
    cancel_unanswered_provider_messages(store, team_run_id, member_run_id, &requested_by, &reason)?;
    let member = mark_member_coordination_closed(store, team_run_id, member_run_id)?;
    let mut member = member;
    let expected = member.clone();
    member.status = MemberRunStatus::Stopped;
    member.finished_at = Some(now_string());
    member.last_event_at = Some(now_string());
    store_conflict_as_usage(store.compare_and_append_member_run(&expected, &member))?;
    store_conflict_as_usage(store.complete_team_member_close(
        team_run_id,
        member_run_id,
        &close.id,
        &now_string(),
    ))?;
    let ledger = TeamRunLedger::without_supervisor(store, team_run_id);
    ledger.append_action(
        &member.id,
        "closed",
        MemberActionStatus::Succeeded,
        "external member coordination closed",
        &format!("{requested_by}: {reason}"),
    )?;
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "closed",
        &format!("member {} coordination closed", member.name),
    )?;
    Ok(serde_json::json!({
        "member_run_id": member.id,
        "status": "stopped",
        "coordination_status": "closed",
        "runtime": "external_unmanaged",
        "runtime_effect": "none",
        "coordination_effect": "member_closed",
        "idempotent": false,
    }))
}

/// POST /v1/team-runs/{id}/members/{m}/resume — dedicated entry for resuming
/// the recorded provider-native session. There is no state where resume is
/// meaningful but reopen is not: an active member is continued with a message
/// or steer (resume refuses it), and a terminal member is reopened through the
/// same capability gates and supervisor-start machinery.
pub(crate) fn resume_team_member_value(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let run = latest_team_run(store, team_run_id)?;
    if !run.member_run_ids.iter().any(|id| id == member_run_id) {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    let member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    if member.coordination_is_active() {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} is active; continue it with a message or steer instead of resume"
        )));
    }
    let reopen_body = serde_json::json!({
        "reopened_by": optional_json_string(body, "resumed_by")?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "operator".to_string()),
        "reason": optional_json_string(body, "reason")?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Host resumed member".to_string()),
    });
    let mut reopened = reopen_team_member_value(store, team_run_id, member_run_id, &reopen_body)?;
    if let Some(object) = reopened.as_object_mut() {
        object.insert("via".to_string(), serde_json::json!("resume"));
    }
    Ok(reopened)
}

pub(crate) fn reopen_team_member_value(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let reopened_by =
        optional_json_string(body, "reopened_by")?.unwrap_or_else(|| "host".to_string());
    let reason =
        optional_json_string(body, "reason")?.unwrap_or_else(|| "Host reopened member".to_string());
    if reopened_by.trim().is_empty() || reason.trim().is_empty() {
        return Err(CliError::Usage(
            "member reopen requires non-empty reopened_by and reason".to_string(),
        ));
    }
    let run = latest_team_run(store, team_run_id)?;
    team_run_execution_space_id(store, &run)?;
    if !run.member_run_ids.iter().any(|id| id == member_run_id) {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    if matches!(run.status, TeamRunStatus::Failed | TeamRunStatus::Cancelled) {
        return Err(CliError::Usage(format!(
            "team run {team_run_id} is {} and cannot reopen members",
            serde_snake_label(&run.status)
        )));
    }
    let mut member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    let requested_host_mode = optional_json_string(body, "host_runtime_mode")?
        .as_deref()
        .map(|mode| parse_host_runtime_mode(Some(mode)))
        .transpose()?;
    let is_host = run.host_actor.as_ref().is_some_and(|actor| {
        actor.kind == TeamActorKind::Host && actor.id == member.agent_member_id
    });
    if requested_host_mode.is_some() && !is_host {
        return Err(CliError::Usage(
            "host_runtime_mode may change only the exact Host AgentMember runtime".into(),
        ));
    }
    let target_host_mode = requested_host_mode.unwrap_or(run.host_control_mode);
    let mode_transition = is_host && target_host_mode != run.host_control_mode;
    if member.coordination_is_retired() {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} is retired; create a new ProviderRuntimeProjection instead"
        )));
    }
    if member.coordination_is_active() {
        if mode_transition {
            return Err(CliError::Usage(
                "Close and settle the Host runtime before changing host_runtime_mode".into(),
            ));
        }
        let external_interactive = member.is_external_interactive();
        let supervisor_current = store
            .latest_team_supervisor_lease(team_run_id)?
            .is_some_and(|lease| is_supervisor_current(&lease));
        return Ok(serde_json::json!({
            "member_run": member,
            "runtime_activation": if external_interactive {
                "external_user_driven"
            } else if supervisor_current {
                "already_active"
            } else {
                "team_run_start_required"
            },
            "idempotent": true,
        }));
    }
    if !matches!(
        member.status,
        MemberRunStatus::Stopped | MemberRunStatus::Completed | MemberRunStatus::Failed
    ) {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} close is not complete (runtime status {}); wait for a terminal runtime status before reopening",
            serde_snake_label(&member.status)
        )));
    }
    let transition_expected = mode_transition.then(|| member.clone());
    if mode_transition {
        member.native_session = None;
        member.provider_compatibility_block_cause = None;
        member.provider_profile = Some(match target_host_mode {
            HostControlMode::ExternalInteractive => team_member_provider_profile_for_mode(
                &member.provider,
                Some(EXECUTION_MODE_EXTERNAL_INTERACTIVE),
            ),
            HostControlMode::Managed => {
                let execution_mode = required_json_string(body, "execution_mode")?;
                if execution_mode == EXECUTION_MODE_EXTERNAL_INTERACTIVE {
                    return Err(CliError::Usage(
                        "managed Host transition requires a persistent provider execution_mode"
                            .into(),
                    ));
                }
                team_member_provider_profile_for_mode(&member.provider, Some(&execution_mode))
            }
        });
    }
    let external_interactive = member.is_external_interactive();
    let mut history_continuity = if external_interactive {
        "external_user_owned"
    } else {
        "provider_native_session"
    };
    if mode_transition {
        history_continuity = if external_interactive {
            "managed_session_preserved_as_history_external_coordination_only"
        } else {
            "external_history_not_imported_new_managed_native_session"
        };
    }
    if !external_interactive && !mode_transition {
        // Reopen is a coordination transition, but for an already-bound native
        // session it is also the Host's explicit intent to resume that exact
        // history. Freshly probe before the runtime generation changes so an
        // installed upgrade cannot hide behind a formerly Current snapshot.
        let probe_error = if member.native_session.is_some() {
            let expected = member.clone();
            let (profile, probe_error) = refreshed_team_member_provider_profile(&member)?;
            if apply_refreshed_provider_profile(&mut member, profile) {
                store_conflict_as_usage(store.compare_and_append_member_run(&expected, &member))?;
            }
            probe_error
        } else {
            None
        };
        let profile = member.provider_profile.as_ref().ok_or_else(|| {
            CliError::Usage(format!(
                "member run {member_run_id} has no provider profile and cannot prove resume support"
            ))
        })?;
        if member.native_session.is_some()
            || matches!(
                profile.compatibility_status,
                ProviderCompatibilityStatus::ReviewRequired
                    | ProviderCompatibilityStatus::Incompatible
                    | ProviderCompatibilityStatus::Unavailable
            )
        {
            let resolution =
                resolve_provider_compatibility(store, profile, probe_error.as_deref())?;
            if let Some(reason) = provider_compatibility_block_reason(
                &member,
                profile,
                &resolution,
                "reopen or resume its provider-native session",
            ) {
                return Err(CliError::Usage(reason));
            }
        }
        if !profile.supports_resume {
            return Err(CliError::Usage(format!(
                "member run {member_run_id} execution mode {} does not support resume",
                profile.execution_mode
            )));
        }
        if let Some(native_session) = member.native_session.as_ref() {
            if !native_session.supports_resume
                || matches!(
                    native_session.availability,
                    harness_core::NativeSessionAvailability::Missing
                        | harness_core::NativeSessionAvailability::Incompatible
                )
            {
                return Err(CliError::Usage(format!(
                    "member run {member_run_id} native session {} is not resumable ({})",
                    native_session.native_session_id,
                    serde_snake_label(&native_session.availability)
                )));
            }
        } else if member.status == MemberRunStatus::Stopped {
            history_continuity = "no_native_session_yet";
        } else {
            return Err(CliError::Usage(format!(
                "member run {member_run_id} has no provider-native session; reopen will not silently replace missing execution history"
            )));
        }
    }
    let expected = transition_expected.unwrap_or_else(|| member.clone());
    member.runtime_generation = member.runtime_generation.checked_add(1).ok_or_else(|| {
        CliError::Usage(format!(
            "member run {member_run_id} runtime generation overflowed"
        ))
    })?;
    member.started_at = now_string();
    member.coordination_status = MemberCoordinationStatus::Active;
    member.status = if external_interactive {
        MemberRunStatus::Idle
    } else {
        MemberRunStatus::Queued
    };
    member.finished_at = None;
    member.last_event_at = Some(now_string());
    if mode_transition {
        let mut next_run = run.clone();
        next_run.host_control_mode = target_host_mode;
        next_run.host_thread_id = optional_json_string(body, "host_thread_id")?;
        next_run.updated_at = now_string();
        store_conflict_as_usage(
            store.compare_and_transition_host_mode(&run, &next_run, &expected, &member),
        )?;
    } else {
        store_conflict_as_usage(
            store.compare_and_advance_member_run_generation(&expected, &member),
        )?;
    }

    let ledger = TeamRunLedger::without_supervisor(store, team_run_id);
    ledger.append_action(
        &member.id,
        "reopened",
        MemberActionStatus::Succeeded,
        "member coordination reopened",
        &format!(
            "{reopened_by}: {reason}; runtime generation {}",
            member.runtime_generation
        ),
    )?;
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "reopened",
        &format!(
            "member {} reopened at runtime generation {}",
            member.name, member.runtime_generation
        ),
    )?;

    let supervisor_current = store
        .latest_team_supervisor_lease(team_run_id)?
        .is_some_and(|lease| is_supervisor_current(&lease));
    Ok(serde_json::json!({
        "member_run": member,
        "runtime_activation": if external_interactive {
            "external_user_driven"
        } else if supervisor_current {
            "supervisor_rescan"
        } else {
            "team_run_start_required"
        },
        "history_continuity": history_continuity,
        "host_runtime_mode": if is_host {
            Some(serde_snake_label(&target_host_mode))
        } else {
            None
        },
        "mode_transition": mode_transition,
        "idempotent": false,
    }))
}

/// Reopen may race the final drain of the Supervisor that just closed the old
/// runtime generation. Give that owner a bounded chance to observe the higher
/// generation; if it releases its lease first, the caller must start a new
/// Supervisor. This prevents a durable `queued` reopen from falling between a
/// last rescan and lease release.
pub(crate) fn reopened_member_requires_supervisor_start(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
) -> CliResult<bool> {
    for _ in 0..40 {
        let member = latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.id == member_run_id && member.team_run_id == team_run_id)
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
        if member.is_external_interactive()
            || !member.coordination_is_active()
            || matches!(
                member.status,
                MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
            )
        {
            return Ok(false);
        }
        let supervisor_current = store
            .latest_team_supervisor_lease(team_run_id)?
            .is_some_and(|lease| is_supervisor_current(&lease));
        if !supervisor_current {
            return Ok(true);
        }
        if member.status != MemberRunStatus::Queued {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(false)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderAnswerRequest {
    #[serde(default)]
    pub(super) option_id: Option<String>,
    #[serde(default)]
    pub(super) response_text: Option<String>,
}

pub(super) fn team_run_host_actor(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<harness_core::agentfirm_api::ActorRef> {
    let run = latest_team_run(store, team_run_id)?;
    let team = latest_teams(store)?
        .remove(&run.agent_team_id)
        .ok_or_else(|| CliError::Usage("TeamRun references a missing AgentTeam".into()))?;
    Ok(harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
        id: team.host_agent_id,
    })
}

pub(super) fn authenticated_host_answer_sender(
    store: &HarnessStore,
    team_run_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    authn_source: &str,
) -> CliResult<TeamActorRef> {
    use harness_core::agentfirm_api::ActorKind;
    let run = latest_team_run(store, team_run_id)?;
    let team = latest_teams(store)?
        .remove(&run.agent_team_id)
        .ok_or_else(|| CliError::Usage("TeamRun references a missing AgentTeam".into()))?;
    if actor.kind != ActorKind::AgentMember || actor.id != team.host_agent_id {
        return Err(CliError::Usage(format!(
            "UNAUTHORIZED_ACTOR: only AgentTeam {} Host {} may answer provider questions; authenticated actor is {:?}:{}",
            team.id, team.host_agent_id, actor.kind, actor.id
        )));
    }
    let binding =
        store_conflict_as_usage(store.host_runtime_binding(team_run_id, current_unix_ms_u64()))?;
    if binding.host_agent_member_id() != actor.id {
        return Err(CliError::Usage(
            "UNAUTHORIZED_ACTOR: authenticated actor is not the exact live Host runtime binding"
                .into(),
        ));
    }
    Ok(TeamActorRef {
        kind: TeamActorKind::Host,
        id: actor.id.clone(),
        display_name: None,
        authn_source: Some(authn_source.to_string()),
    })
}

pub(crate) fn answer_provider_message_value(
    store: &HarnessStore,
    team_run_id: &str,
    message_id: &str,
    body: &serde_json::Value,
    authenticated_actor: &harness_core::agentfirm_api::ActorRef,
    authn_source: &str,
) -> CliResult<serde_json::Value> {
    answer_provider_message_value_with_hook(
        store,
        team_run_id,
        message_id,
        body,
        authenticated_actor,
        authn_source,
        || Ok(()),
    )
}

pub(super) fn answer_provider_message_value_with_hook(
    store: &HarnessStore,
    team_run_id: &str,
    message_id: &str,
    body: &serde_json::Value,
    authenticated_actor: &harness_core::agentfirm_api::ActorRef,
    authn_source: &str,
    after_response_publish: impl FnOnce() -> CliResult<()>,
) -> CliResult<serde_json::Value> {
    let request_id = message_id;
    let current_messages = canonical_team_messages_for_run(store, team_run_id)?;
    let request = current_messages
        .iter()
        .find(|message| message.id == request_id)
        .cloned()
        .ok_or_else(|| CliError::Usage(format!("interaction not found: {request_id}")))?;
    if request.team_run_id != team_run_id
        || request.kind != ProviderDispatchIntent::ProviderInteractionRequest
    {
        return Err(CliError::Usage(format!(
            "interaction {request_id} is not a provider request in team run {team_run_id}"
        )));
    }
    let request_body = ProviderInteractionRequestBody::parse_canonical_json(&request.body)
        .map_err(CliError::Usage)?;
    if !matches!(
        request_body.interaction_type,
        ProviderInteractionType::Question | ProviderInteractionType::PlanReview
    ) {
        return Err(CliError::Usage(format!(
            "provider interaction {request_id} is not a Host-answerable question or plan review"
        )));
    }
    let sender =
        authenticated_host_answer_sender(store, team_run_id, authenticated_actor, authn_source)?;
    let host_member_run_id =
        store_conflict_as_usage(store.active_host_member_binding(team_run_id))?
            .member_run
            .id;
    let answer = serde_json::from_value::<ProviderAnswerRequest>(body.clone())
        .map_err(|error| CliError::Usage(format!("invalid provider answer body: {error}")))?;
    let choice = answer.option_id.filter(|value| !value.trim().is_empty());
    let text = answer
        .response_text
        .filter(|value| !value.trim().is_empty());
    if choice.is_some() == text.is_some() {
        return Err(CliError::Usage(
            "interaction resolution requires exactly one of option_id or response_text".to_string(),
        ));
    }
    if let Some(choice) = choice.as_deref() {
        if !request_body
            .options
            .iter()
            .any(|option| option.id == choice)
        {
            return Err(CliError::Usage(format!(
                "provider interaction {request_id} does not expose option_id {choice}"
            )));
        }
    }
    if text.is_some() && !request_body.options.is_empty() {
        return Err(CliError::Usage(format!(
            "provider interaction {request_id} exposes exact options and does not accept free-form text"
        )));
    }
    let response_body = ProviderInteractionResponseBody {
        interaction_type: request_body.interaction_type,
        choice,
        text,
        session: request_body.session.clone(),
        member: request_body.member.clone(),
        generation: request_body.generation,
    };
    let response_json = response_body.to_canonical_json().map_err(CliError::Usage)?;
    let response_id = provider_interaction_response_id(request_id).map_err(CliError::Usage)?;
    let existing_response = current_messages.into_iter().find(|message| {
        message.kind == ProviderDispatchIntent::ProviderInteractionResponse
            && message.causation_id.as_deref() == Some(request_id)
    });
    if let Some(existing) = existing_response.as_ref() {
        if existing.body != response_json || existing.correlation_id != request.correlation_id {
            return Err(CliError::Usage(format!(
                "provider interaction response {request_id} was replayed with different semantics"
            )));
        }
    }
    let response = TeamMessageProjection {
        id: response_id,
        team_run_id: team_run_id.to_string(),
        work_id: request.work_id.clone(),
        source_plan_ref: request.source_plan_ref.clone(),
        sender: Some(sender.clone()),
        sender_runtime_id: match sender.kind {
            TeamActorKind::Host => host_member_run_id,
            TeamActorKind::Operator => format!("operator:{}", sender.id),
            TeamActorKind::Service => format!("service:{}", sender.id),
            _ => unreachable!("provider response authority is coordination-plane only"),
        },
        recipients: vec![TeamRecipientRef {
            kind: TeamRecipientKind::ProviderRuntimeProjection,
            id: request_body.member.clone(),
        }],
        recipient_runtime_ids: vec![request_body.member.clone()],
        kind: ProviderDispatchIntent::ProviderInteractionResponse,
        body: response_json,
        correlation_id: request.correlation_id.clone(),
        causation_id: Some(request.id.clone()),
        response_intent: Some(ProviderResponseIntent::Informational),
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: request_body.member.clone(),
            policy: TeamDeliveryPolicy::Inject,
            status: TeamDeliveryStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: None,
            failure_reason: None,
            // Stable across exact retries; the Store's semantic idempotency
            // comparison deliberately includes the initial delivery row.
            updated_at: request.created_at.clone(),
        }],
        created_at: now_string(),
    };
    let response = publish_provider_answer_response_first(
        existing_response,
        || publish_team_message(store, &sender, response),
        after_response_publish,
        || acknowledge_provider_request_as_host(store, team_run_id, &request),
    )?;
    serde_json::to_value(response).map_err(CliError::Json)
}

pub(super) fn publish_provider_answer_response_first(
    existing_response: Option<TeamMessageProjection>,
    publish_response: impl FnOnce() -> CliResult<TeamMessageProjection>,
    after_response_publish: impl FnOnce() -> CliResult<()>,
    acknowledge_request: impl FnOnce() -> CliResult<()>,
) -> CliResult<TeamMessageProjection> {
    let response = match existing_response {
        Some(existing) => existing,
        None => {
            let published = publish_response()?;
            // Response-first is the recoverable ordering. If the process dies
            // here, the stable response remains discoverable and an exact
            // retry finishes ACK without publishing a duplicate.
            after_response_publish()?;
            published
        }
    };
    acknowledge_request()?;
    Ok(response)
}

pub(super) fn acknowledge_provider_request_as_host(
    store: &HarnessStore,
    team_run_id: &str,
    request: &TeamMessageProjection,
) -> CliResult<()> {
    use harness_core::agentfirm_api::{ActorKind, ActorRef, CanonicalMessageDeliveryStatus};
    let run = latest_team_run(store, team_run_id)?;
    let host_binding =
        store_conflict_as_usage(store.host_runtime_binding(team_run_id, current_unix_ms_u64()))?;
    let host_identity = host_binding.host_agent_member_id().to_string();
    let execution_space_id = match &host_binding {
        harness_application::HostRuntimeBinding::Managed(binding) => {
            binding.agent_session.execution_space_id.clone()
        }
        harness_application::HostRuntimeBinding::ExternalInteractive(_) => {
            team_run_execution_space_id(store, &run)?
        }
    };
    let matches = store
        .fabric_message_deliveries(&execution_space_id)?
        .into_iter()
        .filter(|delivery| {
            delivery.message_id == request.id
                && delivery.recipient_agent_member_id.as_deref() == Some(host_identity.as_str())
        })
        .collect::<Vec<_>>();
    let delivery = match matches.as_slice() {
        [delivery] => delivery.clone(),
        [] => {
            return Err(CliError::Usage(format!(
                "provider interaction {} has no exact Host delivery",
                request.id
            )))
        }
        _ => {
            return Err(CliError::Usage(format!(
                "provider interaction {} has ambiguous Host deliveries",
                request.id
            )))
        }
    };
    if delivery.status == CanonicalMessageDeliveryStatus::Acknowledged {
        return Ok(());
    }
    if matches!(
        host_binding,
        harness_application::HostRuntimeBinding::ExternalInteractive(_)
    ) {
        store.acknowledge_external_message_delivery(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id,
                authenticated_actor: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: host_identity,
                },
                authority_actor: None,
                command_name: "external_host.interaction.acknowledge".into(),
                idempotency_key: format!("external-host-interaction:{}:ack", request.id),
                expected_version: 0,
                request_fingerprint: None,
            },
            &delivery.id,
            &now_string(),
        )?;
        return Ok(());
    }
    let harness_application::HostRuntimeBinding::Managed(binding) = host_binding else {
        unreachable!("external Host returned after pull-only acknowledgement")
    };
    let session = &binding.agent_session;
    let lease = &binding.node_daemon;
    let daemon = ActorRef {
        kind: ActorKind::Service,
        id: lease.daemon_id.clone(),
    };
    let claim_id = format!("host-interaction-resolve:{}", request.id);
    if delivery.status != CanonicalMessageDeliveryStatus::ProviderReceived
        || delivery.provider_receipt_id.is_none()
        || delivery.recipient_session_id.as_deref() != Some(session.id.as_str())
        || delivery.recipient_session_generation != Some(session.runtime_generation)
        || delivery.claimed_node_daemon_generation != Some(lease.generation)
    {
        return Err(CliError::Usage(format!(
            "HOST_PROVIDER_RECEIPT_REQUIRED: provider interaction {} was not genuinely received by the exact live Host AgentSession generation",
            request.id
        )));
    }
    store.acknowledge_message_delivery(
        &harness_core::agentfirm_api::MutationContext {
            execution_space_id,
            authenticated_actor: ActorRef {
                kind: ActorKind::AgentMember,
                id: host_identity,
            },
            authority_actor: Some(daemon),
            command_name: "agent_session.host_interaction.acknowledge".into(),
            idempotency_key: format!("{claim_id}:ack"),
            expected_version: 0,
            request_fingerprint: None,
        },
        &delivery.id,
        &now_string(),
    )?;
    Ok(())
}
