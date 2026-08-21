use super::*;


pub(super) const PROVIDER_MEMBER_CAS_RETRIES: usize = 3;

/// Compare the immutable authority/provenance carried by a native Session
/// reference. `last_verified_at` is deliberately excluded: every successful
/// provider observation advances that timestamp, including between two turns
/// of the same live runtime. Treating the observation clock as authority drift
/// makes all reverse-RPC callbacks after the first settled turn fail closed.
pub(super) fn provider_callback_native_session_matches(
    supplied: &Option<NativeSessionRef>,
    latest: &Option<NativeSessionRef>,
) -> bool {
    match (supplied, latest) {
        (None, None) => true,
        (Some(supplied), Some(latest)) => {
            supplied.provider == latest.provider
                && supplied.execution_mode == latest.execution_mode
                && supplied.native_session_id == latest.native_session_id
                && supplied.native_locator_kind == latest.native_locator_kind
                && supplied.provider_version == latest.provider_version
                && supplied.adapter_contract_version == latest.adapter_contract_version
                && supplied.availability == latest.availability
                && supplied.supports_resume == latest.supports_resume
                && supplied.parent_native_session_id == latest.parent_native_session_id
        }
        (None, Some(_)) | (Some(_), None) => false,
    }
}

/// Provider callbacks may project only the transient `status` and
/// `last_event_at` fields while a round is in flight.  Accepting any other
/// drift here would let a stale transport overwrite lifecycle authority,
/// runtime generation, native-session provenance, or operator changes.
pub(super) fn validate_provider_callback_drift(
    supplied: &ProviderRuntimeProjection,
    latest: &ProviderRuntimeProjection,
) -> CliResult<()> {
    // Name/profile/capacity may be refreshed by same-generation operator and
    // probe paths while the provider is paused. Those fields are intentionally
    // rebased from `latest`. Everything that establishes execution identity,
    // provenance, permissions, native-session ownership, or outer-round state
    // remains frozen.
    if latest.id != supplied.id
        || latest.team_run_id != supplied.team_run_id
        || latest.slot_id != supplied.slot_id
        || latest.agent_member_id != supplied.agent_member_id
        || latest.role != supplied.role
        || latest.provider != supplied.provider
        || latest.model != supplied.model
        || latest.provider_controls != supplied.provider_controls
        || latest.coordination_status != supplied.coordination_status
        || latest.runtime_generation != supplied.runtime_generation
        || !provider_callback_native_session_matches(
            &supplied.native_session,
            &latest.native_session,
        )
        || latest.provider_cwd_hint != supplied.provider_cwd_hint
        || latest.provider_environment_observation != supplied.provider_environment_observation
        || latest.owned_paths != supplied.owned_paths
        || latest.zero_output_streak != supplied.zero_output_streak
        || latest.last_consumed_work_version != supplied.last_consumed_work_version
        || latest.started_at != supplied.started_at
        || latest.finished_at != supplied.finished_at
    {
        return Err(CliError::Usage(format!(
            "provider callback for ProviderRuntimeProjection {} crossed identity, provenance, lifecycle, native-session, or provider-control authority",
            supplied.id
        )));
    }
    Ok(())
}

#[allow(clippy::large_enum_variant)]
pub(super) enum ProviderInteractionMemberTransition {
    Applied(ProviderRuntimeProjection),
    LifecycleSuperseded,
}

/// Re-fetch and CAS a provider-interaction status projection. The callback's
/// caller can hold a round-start snapshot for arbitrarily long (including
/// across another interaction), so that snapshot is never used directly as
/// the CAS expectation. Close/retire wins over a late resume; a new runtime
/// generation and every non-transient mutation fail closed.
pub(super) fn transition_provider_interaction_member(
    ledger: &TeamRunLedger,
    supplied: &ProviderRuntimeProjection,
    desired: MemberRunStatus,
) -> CliResult<ProviderInteractionMemberTransition> {
    transition_provider_interaction_member_with_hook(ledger, supplied, desired, |_, _| Ok(()))
}

pub(super) fn transition_provider_interaction_member_with_hook(
    ledger: &TeamRunLedger,
    supplied: &ProviderRuntimeProjection,
    desired: MemberRunStatus,
    mut before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<ProviderInteractionMemberTransition> {
    for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
        let latest = ledger
            .latest_member_run(&supplied.id)?
            .ok_or_else(|| CliError::Usage(format!("member run {} not found", supplied.id)))?;
        if latest.runtime_generation != supplied.runtime_generation {
            return Err(CliError::Usage(format!(
                "provider callback for ProviderRuntimeProjection {} belongs to runtime generation {}, latest is {}",
                supplied.id, supplied.runtime_generation, latest.runtime_generation
            )));
        }
        if latest.coordination_status != supplied.coordination_status {
            if desired == MemberRunStatus::Running
                && matches!(
                    latest.coordination_status,
                    MemberCoordinationStatus::Closed | MemberCoordinationStatus::Retired
                )
            {
                return Ok(ProviderInteractionMemberTransition::LifecycleSuperseded);
            }
            return Err(CliError::Usage(format!(
                "provider callback for ProviderRuntimeProjection {} was superseded by coordination lifecycle {:?}",
                supplied.id, latest.coordination_status
            )));
        }
        validate_provider_callback_drift(supplied, &latest)?;
        let valid_status = match desired {
            MemberRunStatus::Waiting => matches!(
                latest.status,
                MemberRunStatus::Running | MemberRunStatus::Waiting
            ),
            MemberRunStatus::Running => matches!(
                latest.status,
                MemberRunStatus::Waiting | MemberRunStatus::Running
            ),
            _ => false,
        };
        if !valid_status {
            return Err(CliError::Usage(format!(
                "provider interaction cannot transition ProviderRuntimeProjection {} from {:?} to {:?}",
                supplied.id, latest.status, desired
            )));
        }
        if latest.status == desired {
            return Ok(ProviderInteractionMemberTransition::Applied(latest));
        }
        let mut next = latest.clone();
        next.status = desired;
        next.last_event_at = Some(now_string());
        before_cas(attempt, &latest)?;
        match ledger.save_member_run(&latest, &next) {
            Ok(()) => return Ok(ProviderInteractionMemberTransition::Applied(next)),
            Err(CliError::Store(StoreError::Conflict(_)))
                if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES => {}
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded provider ProviderRuntimeProjection CAS loop returns on every path")
}

/// Refresh the outer provider driver after all blocking callbacks have
/// returned. Only callback-owned transient drift is accepted. A lingering
/// unresolved Waiting projection is not a terminal round and therefore cannot
/// be overwritten by an outer Idle/Stopped write.
pub(super) fn refresh_member_after_provider_callbacks(
    ledger: &TeamRunLedger,
    round_start: &ProviderRuntimeProjection,
) -> CliResult<ProviderRuntimeProjection> {
    let latest = ledger
        .latest_member_run(&round_start.id)?
        .ok_or_else(|| CliError::Usage(format!("member run {} not found", round_start.id)))?;
    if latest.coordination_status != round_start.coordination_status
        && latest.coordination_status == MemberCoordinationStatus::Closed
        && pending_member_close(&ledger.store, &latest.id)?.is_some()
    {
        // An admitted Close durably latches the request and closes the
        // coordination plane before it asks the provider transport to stop.
        // The provider round that was already in flight must therefore be
        // allowed to observe that one exact same-generation lifecycle advance
        // and finish the RuntimeCommand/AgentSession terminal projection.
        // Normalize only coordination_status for the ordinary drift check so
        // generation, native-session, controls, provenance, Work progress, and
        // every other authority field remain frozen.
        let mut normalized = latest.clone();
        normalized.coordination_status = round_start.coordination_status;
        validate_provider_callback_drift(round_start, &normalized)?;
    } else {
        validate_provider_callback_drift(round_start, &latest)?;
    }
    if latest.status == MemberRunStatus::Waiting {
        let messages = ledger.canonical_team_messages()?;
        let message_unresolved = messages.iter().any(|request| {
            request.kind == ProviderDispatchIntent::ProviderInteractionRequest
                && request.sender_runtime_id == latest.id
                && request.deliveries.iter().any(|delivery| {
                    delivery.member_id == "host" && delivery.status == TeamDeliveryStatus::Delivered
                })
                && !messages.iter().any(|response| {
                    response.kind == ProviderDispatchIntent::ProviderInteractionResponse
                        && response.causation_id.as_deref() == Some(request.id.as_str())
                })
        });
        if message_unresolved {
            return Err(CliError::Usage(format!(
                "provider round for ProviderRuntimeProjection {} cannot finalize while a correlated provider question remains unanswered",
                latest.id
            )));
        }
    }
    Ok(latest)
}

pub(super) fn pending_member_close(
    store: &HarnessStore,
    member_run_id: &str,
) -> CliResult<Option<TeamMemberCloseRequest>> {
    Ok(store
        .latest_team_member_close_request(member_run_id)?
        .filter(|request| request.status == TeamMemberCloseStatus::Pending))
}

/// Prove that applying a durable Close latch cannot skip a provider effect.
///
/// There are only two honest cases:
/// - no provider-native session was ever bound and the runtime is still
///   detached/idle, so there is nothing to close; or
/// - the exact current runtime binding already has a verified CloseMember
///   command and the live-runtime projection is detached/idle.
///
/// A Pending Close request is operator intent, not a provider receipt. In
/// particular it must never turn a failed/unknown provider Close into a
/// Stopped MemberRun during generic error reconciliation.
pub(super) fn require_latched_close_runtime_postcondition(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
) -> CliResult<()> {
    use harness_core::agentfirm_api::{
        RuntimeActivity, RuntimeCommandKind, RuntimeCommandStatus, RuntimeEffectCertainty,
        RuntimePostconditionStatus, RuntimeResidency,
    };

    let (execution_space_id, session) = provider_session_for_member(ledger, member)?;
    let detached_idle = session.control_state.runtime_residency == RuntimeResidency::Detached
        && session.control_state.activity == RuntimeActivity::Idle
        && session.current_turn_id.is_none();
    if member.native_session.is_none() && session.native_session_ref.is_none() && detached_idle {
        return Ok(());
    }
    let expected_binding = runtime_command_binding_for_session(&session);
    let exact_close_applied = ledger
        .store
        .runtime_commands(&execution_space_id)?
        .into_iter()
        .any(|command| {
            command.command == RuntimeCommandKind::CloseMember
                && command.binding == expected_binding
                && command.status == RuntimeCommandStatus::Applied
                && command.effect_certainty == RuntimeEffectCertainty::Applied
                && command.postcondition_status == RuntimePostconditionStatus::Satisfied
        });
    if detached_idle && exact_close_applied {
        return Ok(());
    }
    Err(CliError::Usage(format!(
        "RUNTIME_COMMAND_RECOVERY_REQUIRED: pending Close for {} lacks an exact verified provider CloseMember postcondition (native_session_bound={}, residency={:?}, activity={:?}, current_turn={}, close_applied={exact_close_applied})",
        member.id,
        member.native_session.is_some() || session.native_session_ref.is_some(),
        session.control_state.runtime_residency,
        session.control_state.activity,
        session.current_turn_id.is_some(),
    )))
}

pub(super) fn stop_member_for_latched_close(
    ledger: &TeamRunLedger,
    member_row: &mut ProviderRuntimeProjection,
    close: &TeamMemberCloseRequest,
) -> CliResult<()> {
    if close.team_run_id != ledger.run_id {
        return Err(CliError::Usage(format!(
            "latched close for member {} belongs to team run {}, not {}",
            member_row.id, close.team_run_id, ledger.run_id
        )));
    }
    require_latched_close_runtime_postcondition(ledger, member_row)?;
    let session = require_provider_session_authority(ledger, &member_row.agent_member_id, false)?;
    if session.lifecycle == harness_core::agentfirm_api::AgentSessionStatus::Active
        && session.current_turn_id.is_some()
    {
        return Err(CliError::Usage(format!(
            "RUNTIME_COMMAND_RECOVERY_REQUIRED: latched Team close {} found an active provider turn without its owning live adapter",
            close.id
        )));
    }
    // A pre-spawn/capacity close has no provider effect to journal. A close
    // racing an active turn is admitted above as CancelProviderTurn. In both
    // cases the Team lifecycle may only quiesce the machine-owned Session; it
    // cannot stop it or rewrite another Team's bindings.
    if session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Idle {
        transition_provider_session_for_member(
            ledger,
            member_row,
            harness_core::agentfirm_api::AgentSessionStatus::Idle,
        )?;
    }
    cancel_unanswered_provider_messages(
        &ledger.store,
        &ledger.run_id,
        &member_row.id,
        &close.requested_by,
        &close.reason,
    )?;
    // Close ends this runtime generation: a delivery claimed by this member
    // and Supervisor generation but never provider-received can never be
    // received now. Fail those claims (same rule as the transport-disconnect
    // path) so the Work is not stranded while this generation lives; the
    // provider-native session remains execution truth and no receipt is
    // fabricated. Runs before the member row mutates so a lost lease aborts
    // the whole close application.
    ledger.fail_unreceived_work_claims_for(
        &member_row.id,
        &format!(
            "member closed before provider acceptance: {}: {}",
            close.requested_by, close.reason
        ),
    )?;
    ledger.fail_team_messages_for(
        &member_row.id,
        &format!(
            "member closed before message delivery: {}: {}",
            close.requested_by, close.reason
        ),
    )?;
    // Provider callbacks and capacity/profile observations may advance
    // projection-only fields while the live Close handshake is in flight.
    // Rebase only such same-runtime progress, and keep the complete runtime
    // identity/native-session tuple as the hard fence. A different provider
    // composition, runtime generation, or native session must never inherit
    // this Close receipt.
    let close_anchor = member_row.clone();
    let mut expected = member_row.clone();
    for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
        let mut stopped = expected.clone();
        stopped.coordination_status = MemberCoordinationStatus::Closed;
        stopped.status = MemberRunStatus::Stopped;
        stopped.finished_at = Some(now_string());
        stopped.last_event_at = Some(now_string());
        match ledger.save_member_run(&expected, &stopped) {
            Ok(()) => {
                *member_row = stopped;
                break;
            }
            Err(CliError::Store(StoreError::Conflict(_)))
                if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES =>
            {
                let latest = ledger.latest_member_run(&member_row.id)?.ok_or_else(|| {
                    CliError::Usage(format!(
                        "MEMBER_RUN_NOT_FOUND: {} disappeared while applying Close",
                        member_row.id
                    ))
                })?;
                if !member_runtime_progress_matches(&close_anchor, &close_anchor, &latest, false)
                    || latest.coordination_status != MemberCoordinationStatus::Active
                    || matches!(
                        latest.status,
                        MemberRunStatus::Completed
                            | MemberRunStatus::Failed
                            | MemberRunStatus::Stopped
                    )
                {
                    return Err(CliError::Usage(format!(
                        "RUNTIME_COMMAND_RECOVERY_REQUIRED: member {} authority changed while applying the verified Close receipt",
                        member_row.id
                    )));
                }
                let pending = pending_member_close(&ledger.store, &member_row.id)?;
                if pending.as_ref().map(|request| request.id.as_str()) != Some(close.id.as_str()) {
                    return Err(CliError::Usage(format!(
                        "RUNTIME_COMMAND_RECOVERY_REQUIRED: member {} Close latch changed while applying its provider receipt",
                        member_row.id
                    )));
                }
                expected = latest;
            }
            Err(error) => return Err(error),
        }
    }
    if member_row.status != MemberRunStatus::Stopped
        || member_row.coordination_status != MemberCoordinationStatus::Closed
    {
        return Err(CliError::Usage(format!(
            "RUNTIME_COMMAND_RECOVERY_REQUIRED: member {} Close projection exceeded the bounded CAS retry budget",
            member_row.id
        )));
    }
    // Completing the durable latch is part of the lifecycle transition, not
    // optional observability. Do it immediately after the stopped CAS so a
    // later action/event journal failure cannot strand Closed+Stopped with a
    // permanently Pending request that the roster will no longer rescan.
    store_conflict_as_usage(ledger.store.complete_team_member_close(
        &ledger.run_id,
        &member_row.id,
        &close.id,
        &now_string(),
    ))?;
    ledger.append_action(
        &member_row.id,
        "closed",
        MemberActionStatus::Succeeded,
        "member runtime closed by supervisor",
        &format!("{}: {}", close.requested_by, close.reason),
    )?;
    ledger.fold_event(
        TeamRunEventSourceKind::Member,
        Some(member_row.id.clone()),
        "member_run",
        &member_row.id,
        "updated",
        &format!("member {} closed by Host supervisor", member_row.name),
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn wait_for_idle_member_wake(
    ledger: &TeamRunLedger,
    member_row: &mut ProviderRuntimeProjection,
    controls: &ControlReceiver<MemberControlCommand>,
    mut ensure_transport_alive: impl FnMut() -> CliResult<()>,
    zero_output_streak: u32,
    last_consumed_work_version: Option<u64>,
    policy: &supervisor_wake::WakePolicy,
    backoff: &mut supervisor_wake::WakeBackoff,
) -> CliResult<IdleMemberWake> {
    transition_provider_session_for_member(
        ledger,
        member_row,
        harness_core::agentfirm_api::AgentSessionStatus::Idle,
    )?;
    let idle_since = Instant::now();
    loop {
        // A command may have passed the control-plane fence just before this
        // generation lost ownership. Recheck before reading any process-local
        // handle or touching the durable mailbox.
        ledger.require_supervisor_lease()?;
        while let Ok(command) = controls.try_recv() {
            match command {
                MemberControlCommand::Close {
                    reason,
                    requested_by,
                    reply,
                } => {
                    let close = match pending_member_close(&ledger.store, &member_row.id)? {
                        Some(close)
                            if close.reason == reason && close.requested_by == requested_by =>
                        {
                            close
                        }
                        Some(_) => {
                            let error = CliError::Usage(
                                "RUNTIME_COMMAND_RECOVERY_REQUIRED: live close differs from durable close latch"
                                    .into(),
                            );
                            let _ = reply.send(Err(CliError::Usage(error.to_string())));
                            return Err(error);
                        }
                        None => {
                            let error = CliError::Usage(
                                "RUNTIME_COMMAND_RECOVERY_REQUIRED: live close has no durable close latch"
                                    .into(),
                            );
                            let _ = reply.send(Err(CliError::Usage(error.to_string())));
                            return Err(error);
                        }
                    };
                    return Ok(IdleMemberWake::CloseRequested {
                        close,
                        reply: Some(reply),
                    });
                }
                MemberControlCommand::Interrupt { reply, .. } => {
                    let _ = reply.send(Ok(serde_json::json!({
                        "member_run_id": member_row.id,
                        "status": "idle",
                        "provider_ack": "no_active_turn",
                    })));
                }
                MemberControlCommand::Steer { reply, .. } => {
                    let _ = reply.send(Err(CliError::Usage(
                        "member is idle; assign Work or send a response-required TeamMessageProjection to start a new turn"
                            .to_string(),
                    )));
                }
            }
        }
        if let Some(close) = pending_member_close(&ledger.store, &member_row.id)? {
            return Ok(IdleMemberWake::CloseRequested { close, reply: None });
        }

        // Fence the provider transport before taking durable ownership of any
        // queued mail. If the transport died after the previous turn, resume
        // the same native session first; otherwise a message could be left in
        // the intentionally-uncertain `claimed` state before turn/start even
        // reached the provider.
        ensure_transport_alive()?;
        if let Some(claimed) = claim_canonical_work_for_member(ledger, member_row)? {
            backoff.reset();
            let expected = member_row.clone();
            member_row.status = MemberRunStatus::Running;
            member_row.finished_at = None;
            member_row.last_event_at = Some(now_string());
            ledger.save_member_run(&expected, member_row)?;
            transition_provider_session_for_member(
                ledger,
                member_row,
                harness_core::agentfirm_api::AgentSessionStatus::Active,
            )?;
            return Ok(IdleMemberWake::Work(Box::new(claimed)));
        }
        let canonical_messages = claim_canonical_messages_for_member(ledger, member_row)?;
        if !canonical_messages.is_empty() {
            backoff.reset();
            let expected = member_row.clone();
            member_row.status = MemberRunStatus::Running;
            member_row.finished_at = None;
            member_row.last_event_at = Some(now_string());
            ledger.save_member_run(&expected, member_row)?;
            transition_provider_session_for_member(
                ledger,
                member_row,
                harness_core::agentfirm_api::AgentSessionStatus::Active,
            )?;
            return Ok(IdleMemberWake::Messages(canonical_messages));
        }
        // Build pure views from the store for the decision function.
        let member_view = build_member_wake_view(
            ledger,
            member_row,
            zero_output_streak,
            last_consumed_work_version,
        )?;
        let board_view = build_board_wake_view(ledger, member_row)?;

        let decision = supervisor_wake::decide_wake(&member_view, &board_view, policy, backoff);
        match decision {
            supervisor_wake::WakeDecision::DeliverPending => {
                // Try work deliveries first (work contract prompt).
                if let Some(claimed) = ledger.claim_canonical_work_for(&member_row.id)? {
                    backoff.reset();
                    let expected = member_row.clone();
                    member_row.status = MemberRunStatus::Running;
                    member_row.finished_at = None;
                    member_row.last_event_at = Some(now_string());
                    ledger.save_member_run(&expected, member_row)?;
                    transition_provider_session_for_member(
                        ledger,
                        member_row,
                        harness_core::agentfirm_api::AgentSessionStatus::Active,
                    )?;
                    return Ok(IdleMemberWake::Work(Box::new(claimed)));
                }
                // Then try active-work continuation.
                if member_supervisor_test_idle_grace().is_none() {
                    if let Some(work) = ledger.active_work_continuation_for(&member_row.id)? {
                        // Before continuing work, deliver any pending
                        // round-triggering messages first (Host replies,
                        // peer messages). Without this, a Host reply sent
                        // between rounds is silently lost when the wake
                        // reason is ActiveWorkContinuation.
                        let pending = ledger.claim_canonical_round_messages_for(&member_row.id)?;
                        if !pending.is_empty() {
                            backoff.reset();
                            let expected = member_row.clone();
                            member_row.status = MemberRunStatus::Running;
                            member_row.finished_at = None;
                            member_row.last_event_at = Some(now_string());
                            ledger.save_member_run(&expected, member_row)?;
                            transition_provider_session_for_member(
                                ledger,
                                member_row,
                                harness_core::agentfirm_api::AgentSessionStatus::Active,
                            )?;
                            return Ok(IdleMemberWake::Messages(pending));
                        }
                        backoff.reset();
                        let expected = member_row.clone();
                        member_row.status = MemberRunStatus::Running;
                        member_row.finished_at = None;
                        member_row.last_event_at = Some(now_string());
                        ledger.save_member_run(&expected, member_row)?;
                        transition_provider_session_for_member(
                            ledger,
                            member_row,
                            harness_core::agentfirm_api::AgentSessionStatus::Active,
                        )?;
                        return Ok(IdleMemberWake::ActiveWorkContinuation(Box::new(work)));
                    }
                }
                // Then terminal-work notifications (informational messages
                // for Done / Cancelled works the member still owns) and
                // response-required messages. Deliver both in one batch so
                // members that exit after single turns (e.g. test fakes with
                // EXIT_AFTER_FIRST_TURN=1) do not lose queued follow-up
                // messages to disconnect handling between separate deliveries.
                let mut notifs = ledger.claim_terminal_work_notifications_for(&member_row.id)?;
                let mut claimed = ledger.claim_canonical_round_messages_for(&member_row.id)?;
                notifs.append(&mut claimed);
                // Deduplicate by message id: claim_canonical_round_messages_for
                // may re-claim messages that claim_terminal_work_notifications_for
                // already published (informational work notifications). Keep the
                // last entry — the claimed version — because
                // mark_message_delivered requires a durable claim_id.
                notifs.reverse();
                {
                    let mut seen = BTreeSet::new();
                    notifs.retain(|msg| seen.insert(msg.id.clone()));
                }
                notifs.reverse();
                if !notifs.is_empty() {
                    backoff.reset();
                    let expected = member_row.clone();
                    member_row.status = MemberRunStatus::Running;
                    member_row.finished_at = None;
                    member_row.last_event_at = Some(now_string());
                    ledger.save_member_run(&expected, member_row)?;
                    transition_provider_session_for_member(
                        ledger,
                        member_row,
                        harness_core::agentfirm_api::AgentSessionStatus::Active,
                    )?;
                    return Ok(IdleMemberWake::Messages(notifs));
                }
                // DeliverPending predicted but nothing claimable — fall through to Sleep.
            }
            supervisor_wake::WakeDecision::Continue(_work_id) => {
                // Active Work version changed → re-inject continuation.
                if member_supervisor_test_idle_grace().is_none() {
                    if let Some(work) = ledger.active_work_continuation_for(&member_row.id)? {
                        backoff.reset();
                        let expected = member_row.clone();
                        member_row.status = MemberRunStatus::Running;
                        member_row.finished_at = None;
                        member_row.last_event_at = Some(now_string());
                        ledger.save_member_run(&expected, member_row)?;
                        transition_provider_session_for_member(
                            ledger,
                            member_row,
                            harness_core::agentfirm_api::AgentSessionStatus::Active,
                        )?;
                        return Ok(IdleMemberWake::ActiveWorkContinuation(Box::new(work)));
                    }
                }
                // Work version changed but continuation candidate disappeared — fall through to Sleep.
            }
            supervisor_wake::WakeDecision::ClaimHint(_work_ids) => {
                // Board-discovery hint for idle members: the wake is only a
                // discovery hint; ownership starts at the atomic claim.
                // Inject a lightweight prompt so the member can discover and
                // claim eligible Works.
                if let Some(work) = ledger.active_work_continuation_for(&member_row.id)? {
                    backoff.reset();
                    let expected = member_row.clone();
                    member_row.status = MemberRunStatus::Running;
                    member_row.finished_at = None;
                    member_row.last_event_at = Some(now_string());
                    ledger.save_member_run(&expected, member_row)?;
                    transition_provider_session_for_member(
                        ledger,
                        member_row,
                        harness_core::agentfirm_api::AgentSessionStatus::Active,
                    )?;
                    return Ok(IdleMemberWake::ActiveWorkContinuation(Box::new(work)));
                }
            }
            supervisor_wake::WakeDecision::Sleep(_duration) => {
                if member_supervisor_test_idle_grace()
                    .is_some_and(|grace| idle_since.elapsed() >= grace)
                {
                    return Ok(IdleMemberWake::TestRetired);
                }
                backoff.sleep_and_tick(policy);
                continue;
            }
            supervisor_wake::WakeDecision::Degraded(reason) => {
                // Mark the member blocked so continuation stops.
                // The Host must intervene (message, steer, or recover).
                let expected = member_row.clone();
                member_row.status = MemberRunStatus::Blocked;
                member_row.finished_at = None;
                member_row.last_event_at = Some(now_string());
                ledger.save_member_run(&expected, member_row)?;
                ledger.append_action(
                    &member_row.id,
                    "degraded",
                    MemberActionStatus::Failed,
                    "member degraded — zero-output spiral",
                    &reason,
                )?;
                ledger.fold_event(
                    TeamRunEventSourceKind::Member,
                    Some(member_row.id.clone()),
                    "member_run",
                    &member_row.id,
                    "degraded",
                    &format!("member {} degraded: {reason}", member_row.name),
                )?;
                return Ok(IdleMemberWake::Degraded(reason));
            }
        }
    }
}

/// Build a pure `MemberWakeView` from store reads.
pub(super) fn build_member_wake_view(
    ledger: &TeamRunLedger,
    member_row: &ProviderRuntimeProjection,
    zero_output_streak: u32,
    last_consumed_work_version: Option<u64>,
) -> CliResult<supervisor_wake::MemberWakeView> {
    let is_idle = matches!(member_row.status, MemberRunStatus::Idle);

    let all_works = ledger.store.latest_works()?;
    let active_work = all_works.iter().find(|work| {
        work.team_run_id == ledger.run_id
            && is_active_work_continuation_candidate(work, &member_row.id, &all_works)
    });

    let delivery_count = ledger.queued_works_for(&member_row.id)?.len() as u32;

    let message_count = ledger
        .queued_messages_for(&member_row.id)?
        .iter()
        .filter(|message| message.requires_response())
        .count() as u32;

    Ok(supervisor_wake::MemberWakeView {
        member_id: member_row.id.clone(),
        status: member_row.status,
        is_idle,
        active_work_id: active_work.map(|work| work.id.clone()),
        active_work_version: active_work.map(|work| work.version),
        last_consumed_work_version,
        unconsumed_delivery_count: delivery_count,
        unconsumed_message_count: message_count,
        zero_output_streak,
    })
}

/// Build a pure `BoardWakeView` from store reads.
pub(super) fn build_board_wake_view(
    ledger: &TeamRunLedger,
    member_row: &ProviderRuntimeProjection,
) -> CliResult<supervisor_wake::BoardWakeView> {
    let all_works = ledger.store.latest_works()?;
    let stable_member_id = member_row.agent_member_id.as_str();
    let eligible_claim_work_ids: Vec<String> = all_works
        .iter()
        .filter(|work| {
            work.team_run_id == ledger.run_id
                && work.phase == WorkPhase::Open
                && work.owner_member_id.is_none()
                && work.claim_mode == harness_core::WorkClaimMode::TeamClaim
                && work.prerequisites_satisfied(all_works.iter())
                && (work.eligible_member_ids.is_empty()
                    || work
                        .eligible_member_ids
                        .iter()
                        .any(|eligible| eligible == stable_member_id))
        })
        .map(|work| work.id.clone())
        .collect();
    Ok(supervisor_wake::BoardWakeView {
        eligible_claim_work_ids,
    })
}
