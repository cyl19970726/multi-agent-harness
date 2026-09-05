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
        if let Some(supervisor) = store
            .latest_team_supervisor_lease(team_run_id)?
            .filter(is_supervisor_current)
        {
            // A post-receipt provider failure deliberately drops its process
            // handle and blocks the Member before any automatic replay.  In
            // that exact state there is no live handle left to acknowledge a
            // normal CloseMember command.  The canonical detached+idle
            // AgentSession is nevertheless positive evidence that this
            // Member runtime generation has already ended.  Let the Host
            // close only that obsolete coordination generation, without
            // fabricating a provider Close receipt; ordinary attached/live
            // runtimes continue through the normal control path below.
            if member.status == MemberRunStatus::Blocked {
                if let Some(result) = close_detached_blocked_member_for_recovery(
                    store,
                    &run.id,
                    &member,
                    &supervisor,
                    &requested_by,
                    &reason,
                )? {
                    return Ok(result);
                }
            }
            // #812: after a daemon restart the re-adopted Supervisor serves an
            // unclosed member of a COMPLETED run for Close authority without
            // starting a new provider cycle. When the session proves the
            // runtime is over, close through the ordinary latch and
            // coordination write path; an Attached lane falls through to the
            // live-control close and its real provider receipt.
            if run.status == TeamRunStatus::Completed {
                if let Some(result) =
                    crate::completed_run_members::close_completed_run_member_coordination(
                        store,
                        &run.id,
                        &member,
                        &supervisor,
                        &requested_by,
                        &reason,
                    )?
                {
                    return Ok(result);
                }
            }
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
        return Err(CliError::RuntimeRecoveryRequired(
            "managed AgentSession has no current provider-loop authority; reconcile its RuntimeCommand/session state before Close"
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

/// Whether this Session sits at a terminal turn boundary: no cycle activity and
/// no turn in flight.
///
/// `Interrupted` counts alongside `Idle` here. It records only that the cycle
/// never reached its own end — typically because a NodeDaemon drain killed the
/// owned provider process group — and the caller has already proven the runtime
/// residency is `Detached`, so no live handle can be executing either way.
/// Fencing the Host's Close on the lifecycle label alone would leave a member
/// whose runtime is provably dead with no exit at all.
pub(super) fn session_is_at_terminal_turn_boundary(
    session: &harness_core::agentfirm_api::AgentSession,
) -> bool {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity};
    matches!(
        session.lifecycle,
        AgentSessionStatus::Idle | AgentSessionStatus::Interrupted
    ) && session.control_state.activity == RuntimeActivity::Idle
        && session.current_turn_id.is_none()
}

pub(super) fn close_detached_blocked_member_for_recovery(
    store: &HarnessStore,
    team_run_id: &str,
    member: &ProviderRuntimeProjection,
    supervisor: &TeamSupervisorLease,
    requested_by: &str,
    reason: &str,
) -> CliResult<Option<serde_json::Value>> {
    close_detached_blocked_member_for_recovery_with_hook(
        store,
        team_run_id,
        member,
        supervisor,
        requested_by,
        reason,
        |_| Ok(()),
    )
}

pub(super) fn close_detached_blocked_member_for_recovery_with_hook(
    store: &HarnessStore,
    team_run_id: &str,
    member: &ProviderRuntimeProjection,
    supervisor: &TeamSupervisorLease,
    requested_by: &str,
    reason: &str,
    before_terminal_cas: impl FnMut(usize) -> CliResult<()>,
) -> CliResult<Option<serde_json::Value>> {
    close_detached_blocked_member_for_recovery_with_hooks(
        store,
        team_run_id,
        member,
        supervisor,
        requested_by,
        reason,
        before_terminal_cas,
        |_| Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn close_detached_blocked_member_for_recovery_with_hooks(
    store: &HarnessStore,
    team_run_id: &str,
    member: &ProviderRuntimeProjection,
    supervisor: &TeamSupervisorLease,
    requested_by: &str,
    reason: &str,
    mut before_terminal_cas: impl FnMut(usize) -> CliResult<()>,
    mut after_terminal_cas: impl FnMut(&ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<Option<serde_json::Value>> {
    use harness_core::agentfirm_api::{
        NativeSessionAvailability as AgentNativeSessionAvailability, RuntimeCommandStatus,
        RuntimeDriverRef, RuntimeEffectCertainty, RuntimeResidency, WorkDeliveryStatus,
    };

    let ledger = TeamRunLedger::without_supervisor(store, team_run_id);
    let (execution_space_id, session) = provider_session_for_member(&ledger, member)?;
    if session.control_state.runtime_residency != RuntimeResidency::Detached {
        return Ok(None);
    }
    if !session_is_at_terminal_turn_boundary(&session) {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} session {} is not detached+idle at a terminal turn boundary",
            member.id, session.id
        )));
    }
    let native_session_matches_and_resumable = match (
        member.native_session.as_ref(),
        session.native_session_ref.as_ref(),
    ) {
        (Some(member_native), Some(session_native)) => {
            member_native.provider == session_native.provider
                && member_native.execution_mode == session_native.execution_mode
                && member_native.native_session_id == session_native.native_session_id
                && member_native.native_locator_kind == session_native.native_locator_kind
                && member_native.provider_version == session_native.provider_version
                && member_native.adapter_contract_version == session_native.adapter_contract_version
                && member_native.supports_resume
                && session_native.supports_resume
                && member_native.parent_native_session_id == session_native.parent_native_session_id
                && matches!(
                    (member_native.availability, session_native.availability),
                    (
                        harness_core::NativeSessionAvailability::Available,
                        AgentNativeSessionAvailability::Available
                    ) | (
                        harness_core::NativeSessionAvailability::Stale,
                        AgentNativeSessionAvailability::Stale
                    )
                )
        }
        _ => false,
    };
    if !native_session_matches_and_resumable {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} lacks an exact present, resumable native-session authority matching AgentSession {}",
            member.id, session.id
        )));
    }
    let exact_supervisor_driver = matches!(
        &session.control_state.driver_ref,
        RuntimeDriverRef::TeamSupervisor {
            team_run_id: driver_team_run_id,
            team_supervisor_id,
            team_supervisor_generation,
        } if driver_team_run_id == team_run_id
            && team_supervisor_id == &supervisor.supervisor_id
            && *team_supervisor_generation == supervisor.generation
    );
    if !exact_supervisor_driver
        || supervisor.node_daemon_id != session.node_daemon_id
        || supervisor.node_daemon_generation != session.node_daemon_generation
    {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} is not bound to the exact current Supervisor and NodeDaemon generations",
            member.id
        )));
    }
    require_provider_session_authority(&ledger, &member.agent_member_id, false)?;

    let ambiguous_command = store
        .runtime_commands(&execution_space_id)?
        .into_iter()
        .any(|command| {
            command.target_session_id.as_deref() == Some(session.id.as_str())
                && command.target_session_generation == Some(session.runtime_generation)
                && matches!(
                    command.status,
                    RuntimeCommandStatus::Accepted
                        | RuntimeCommandStatus::Quiesced
                        | RuntimeCommandStatus::RecoveryRequired
                )
                && command.effect_certainty == RuntimeEffectCertainty::Unknown
        });
    if ambiguous_command {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} still has an ambiguous RuntimeCommand",
            member.id
        )));
    }

    // A provider receipt proves that the old runtime consumed this exact Work
    // revision even when the adapter failed before persisting its ordinary
    // end-of-round member projection.  Carry only that coordination fact into
    // the closed generation so Reopen cannot re-inject the same Work.  A new
    // Host Message or a later Work revision remains the explicit wake event.
    let works = store.latest_works()?;
    let active_bindings = store
        .fabric_work_execution_bindings(&execution_space_id)?
        .into_iter()
        .filter(|binding| {
            binding.agent_member_id == member.agent_member_id
                && binding.agent_session_id == session.id
                && binding.agent_session_generation == session.runtime_generation
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        })
        .collect::<Vec<_>>();
    let mut bound_work_ids = BTreeSet::new();
    if active_bindings
        .iter()
        .any(|binding| !bound_work_ids.insert(binding.work_id.as_str()))
    {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} has multiple active execution bindings for one Work",
            member.id
        )));
    }
    let received_deliveries = store
        .fabric_work_deliveries(&execution_space_id)?
        .into_iter()
        .filter(|delivery| {
            delivery.recipient_agent_member_id == member.agent_member_id
                && delivery.recipient_session_id == session.id
                && delivery.recipient_session_generation == session.runtime_generation
                && delivery.status == WorkDeliveryStatus::ProviderReceived
        })
        .filter_map(|delivery| {
            works
                .iter()
                .find(|work| {
                    work.id == delivery.work_id
                        && work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str())
                        && !work.is_terminal()
                        && active_bindings.iter().any(|binding| {
                            binding.work_id == delivery.work_id
                                && binding.work_revision == delivery.work_revision
                        })
                })
                .map(|work| (work.id.clone(), delivery.work_revision))
        })
        .collect::<BTreeSet<_>>();
    if received_deliveries.len() > 1 {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} has multiple provider-received active Work revisions",
            member.id
        )));
    }
    let consumed_work_version = received_deliveries
        .into_iter()
        .next()
        .map(|(_, version)| version);

    // Latch intent first. The terminal recovery projection itself is committed
    // below through one Store writer-lock transaction that revalidates the
    // exact Supervisor/NodeDaemon, AgentSession, ambiguous-command set, and
    // MemberRun revision. No provider effect is issued on this path.
    let close = latch_detached_recovery_close_for_supervisor(
        store,
        team_run_id,
        &member.id,
        requested_by,
        reason,
        harness_core::DetachedRecoveryCloseFence {
            execution_space_id: execution_space_id.clone(),
            member_run_generation: member.runtime_generation,
            agent_session_id: session.id.clone(),
            agent_session_generation: session.runtime_generation,
            agent_session_version: session.version,
            agent_session_driver_generation: session.control_state.driver_generation,
            native_session_id: session
                .native_session_ref
                .as_ref()
                .expect("detached recovery requires native Session")
                .native_session_id
                .clone(),
            node_daemon_id: session.node_daemon_id.clone(),
            node_daemon_generation: session.node_daemon_generation,
            authorizing_supervisor_id: supervisor.supervisor_id.clone(),
            authorizing_supervisor_generation: supervisor.generation,
        },
    )?;
    let mut conflicted_expected = None;
    let closed = 'terminal_cas: {
        for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
            let latest = ledger
                .latest_member_run(&member.id)?
                .ok_or_else(|| CliError::Usage(format!("member run not found: {}", member.id)))?;
            if latest.runtime_generation != member.runtime_generation
                || latest.status != MemberRunStatus::Blocked
                || !latest.coordination_is_active()
                || !provider_callback_native_session_matches(
                    &member.native_session,
                    &latest.native_session,
                )
            {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "DETACHED_MEMBER_RECOVERY_FENCED: member {} changed after recovery admission",
                    member.id
                )));
            }
            if let Some(expected) = conflicted_expected.take() {
                if !is_same_runtime_close_drift(&expected, &latest) {
                    return Err(CliError::RuntimeRecoveryRequired(format!(
                        "DETACHED_MEMBER_RECOVERY_FENCED: member {} changed outside the admitted runtime generation",
                        member.id
                    )));
                }
            }
            let (_, current_session) = provider_session_for_member(&ledger, &latest)?;
            if current_session.version != session.version
                || current_session.lifecycle != session.lifecycle
                || current_session.control_state.runtime_residency != RuntimeResidency::Detached
                || !session_is_at_terminal_turn_boundary(&current_session)
            {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "DETACHED_MEMBER_RECOVERY_FENCED: AgentSession {} changed after recovery admission",
                    session.id
                )));
            }
            before_terminal_cas(attempt)?;

            let expected = latest;
            let mut next = expected.clone();
            let closed_at = now_string();
            next.coordination_status = MemberCoordinationStatus::Closed;
            next.status = MemberRunStatus::Stopped;
            next.last_consumed_work_version =
                consumed_work_version.or(next.last_consumed_work_version);
            // The consumed provider-received revision has ended probation.
            // Reopen must wait for new canonical Host input instead of using
            // the generic zero-output continuation predicate.
            next.zero_output_streak = 0;
            next.finished_at = Some(closed_at.clone());
            next.last_event_at = Some(closed_at);
            match store.compare_and_append_recovered_member_run_for_supervisor(
                &expected,
                &next,
                &execution_space_id,
                &current_session,
                &supervisor.supervisor_id,
                supervisor.generation,
            ) {
                Ok(()) => break 'terminal_cas next,
                Err(StoreError::Conflict(message))
                    if message.starts_with("ProviderRuntimeProjection ")
                        && message.ends_with(" changed concurrently; retry the operation")
                        && attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES =>
                {
                    conflicted_expected = Some(expected);
                }
                Err(StoreError::Conflict(message))
                    if message.starts_with("TEAM_SUPERVISOR_LEASE_LOST:")
                        || message.starts_with("TEAM_SUPERVISOR_PARENT_FENCED:") =>
                {
                    return Err(CliError::SupervisorLeaseLost(message));
                }
                Err(StoreError::Conflict(message))
                    if message.starts_with("DETACHED_MEMBER_RECOVERY_") =>
                {
                    return Err(CliError::RuntimeRecoveryRequired(message));
                }
                Err(error) => return store_conflict_as_usage(Err(error)),
            }
        }
        unreachable!("bounded detached-recovery CAS loop returns on every path")
    };
    after_terminal_cas(&closed)?;
    store_conflict_as_usage(store.complete_team_member_close(
        team_run_id,
        &member.id,
        &close.id,
        &now_string(),
    ))?;
    cancel_unanswered_provider_messages(store, team_run_id, &member.id, requested_by, reason)?;
    ledger.append_action(
        &member.id,
        "closed",
        MemberActionStatus::Succeeded,
        "detached blocked member coordination closed for recovery",
        &format!("{requested_by}: {reason}"),
    )?;
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "closed",
        &format!(
            "member {} detached runtime generation {} closed for explicit recovery",
            member.name, member.runtime_generation
        ),
    )?;
    Ok(Some(serde_json::json!({
        "member_run_id": closed.id,
        "status": "stopped",
        "coordination_status": "closed",
        "runtime": "not_live",
        "runtime_effect": "already_detached",
        "coordination_effect": "member_closed_for_recovery",
        "provider_close_receipt": "not_fabricated",
        "idempotent": false,
    })))
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
    let reopen_actor = store.exact_team_run_host_actor(team_run_id)?;
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
        store_conflict_as_usage(store.compare_and_transition_host_mode(
            &reopen_actor,
            &run,
            &next_run,
            &expected,
            &member,
        ))?;
    } else {
        store_conflict_as_usage(store.compare_and_reopen_member_run_generation(
            &reopen_actor,
            &expected,
            &member,
        ))?;
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
