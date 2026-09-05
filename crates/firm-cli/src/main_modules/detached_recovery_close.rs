//! The receipt-free detached-recovery Close and its admission modes.
//!
//! Split out of `http_member_control.rs` so the HTTP member-control seam and
//! this fenced, receipt-free close path each stay one readable module. Two
//! admission modes share the same preconditions and terminal CAS: the DEV-184
//! exact-generation Blocked recovery, and the #812 completed-run Close whose
//! generation evidence lives in `completed_run_members.rs`.

use super::*;

/// Admission axis for the receipt-free detached-recovery Close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DetachedRecoveryCloseMode {
    /// DEV-184: a Blocked member bound to the exact current Supervisor and
    /// NodeDaemon generation.
    BlockedMemberExactGeneration,
    /// #812: an unclosed member of a Completed TeamRun. After a daemon
    /// restart the recorded driver may be a superseded Supervisor/NodeDaemon
    /// generation; the Close then requires the recorded predecessor evidence
    /// (the driver Supervisor generation's Released lease) before the runtime
    /// counts as provably over.
    CompletedRunMember,
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
        DetachedRecoveryCloseMode::BlockedMemberExactGeneration,
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
    mode: DetachedRecoveryCloseMode,
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
    // The same proof `team-run recover` evaluates (handoff, continuation,
    // queued input, ambiguous command), so the two verbs agree (GitHub #841).
    if let Some(blocker) = lane_termination_blocker(store, &execution_space_id, &session)? {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} does not prove its runtime gone: {blocker}",
            member.id
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
    let driver_supervisor = match &session.control_state.driver_ref {
        RuntimeDriverRef::TeamSupervisor {
            team_run_id: driver_team_run_id,
            team_supervisor_id,
            team_supervisor_generation,
        } if driver_team_run_id == team_run_id => {
            Some((team_supervisor_id.clone(), *team_supervisor_generation))
        }
        _ => None,
    };
    let exact_current_driver =
        driver_supervisor
            .as_ref()
            .is_some_and(|(driver_id, driver_generation)| {
                driver_id == &supervisor.supervisor_id
                    && *driver_generation == supervisor.generation
            });
    let exact_current_daemon = supervisor.node_daemon_id == session.node_daemon_id
        && supervisor.node_daemon_generation == session.node_daemon_generation;
    match mode {
        DetachedRecoveryCloseMode::BlockedMemberExactGeneration => {
            if !exact_current_driver || !exact_current_daemon {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "DETACHED_MEMBER_RECOVERY_FENCED: member {} is not bound to the exact current Supervisor and NodeDaemon generations",
                    member.id
                )));
            }
        }
        DetachedRecoveryCloseMode::CompletedRunMember => {
            crate::completed_run_members::require_completed_run_close_generation_evidence(
                member,
                &session,
                supervisor,
                driver_supervisor.as_ref(),
                exact_current_driver,
                exact_current_daemon,
            )?;
        }
    }
    match mode {
        DetachedRecoveryCloseMode::BlockedMemberExactGeneration => {
            require_provider_session_authority(&ledger, &member.agent_member_id, false)?;
        }
        DetachedRecoveryCloseMode::CompletedRunMember => {
            // The session may still name the settled predecessor NodeDaemon
            // generation (proven by the evidence gate above), so the
            // live-daemon half of the authority proof cannot hold here.
            require_provider_session_authority_for_settled_generation(
                &ledger,
                &member.agent_member_id,
            )?;
        }
    }

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
    let close = match mode {
        DetachedRecoveryCloseMode::BlockedMemberExactGeneration => {
            latch_detached_recovery_close_for_supervisor(
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
            )?
        }
        DetachedRecoveryCloseMode::CompletedRunMember => {
            // The store-side detached-recovery fence is typed for the DEV-184
            // exact-generation Blocked case only. The completed-run Close
            // carries its CLI-side proofs (above) and the same terminal-CAS
            // revalidation (below) under an ordinary Supervisor latch.
            latch_member_close_for_supervisor(
                store,
                team_run_id,
                &member.id,
                requested_by,
                reason,
                &supervisor.supervisor_id,
                supervisor.generation,
            )?
        }
    };
    let mut conflicted_expected = None;
    let closed = 'terminal_cas: {
        for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
            let latest = ledger
                .latest_member_run(&member.id)?
                .ok_or_else(|| CliError::Usage(format!("member run not found: {}", member.id)))?;
            let status_admitted = match mode {
                DetachedRecoveryCloseMode::BlockedMemberExactGeneration => {
                    latest.status == MemberRunStatus::Blocked
                }
                DetachedRecoveryCloseMode::CompletedRunMember => !matches!(
                    latest.status,
                    MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
                ),
            };
            if latest.runtime_generation != member.runtime_generation
                || !status_admitted
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
            if mode == DetachedRecoveryCloseMode::CompletedRunMember {
                // Re-verify the generation evidence against the session as it
                // stands inside this CAS attempt, so a successor that changed
                // session driver or daemon state between the CLI-side proof
                // and the terminal write is fenced here, fail-closed (#812).
                let current_driver = match &current_session.control_state.driver_ref {
                    RuntimeDriverRef::TeamSupervisor {
                        team_run_id: driver_team_run_id,
                        team_supervisor_id,
                        team_supervisor_generation,
                    } if driver_team_run_id == team_run_id => {
                        Some((team_supervisor_id.clone(), *team_supervisor_generation))
                    }
                    _ => None,
                };
                let current_exact_driver =
                    current_driver
                        .as_ref()
                        .is_some_and(|(driver_id, driver_generation)| {
                            driver_id == &supervisor.supervisor_id
                                && *driver_generation == supervisor.generation
                        });
                let current_exact_daemon = supervisor.node_daemon_id
                    == current_session.node_daemon_id
                    && supervisor.node_daemon_generation == current_session.node_daemon_generation;
                crate::completed_run_members::require_completed_run_close_generation_evidence(
                    &latest,
                    &current_session,
                    supervisor,
                    current_driver.as_ref(),
                    current_exact_driver,
                    current_exact_daemon,
                )?;
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
    let action_detail = match mode {
        DetachedRecoveryCloseMode::BlockedMemberExactGeneration => {
            "detached blocked member coordination closed for recovery"
        }
        DetachedRecoveryCloseMode::CompletedRunMember => {
            "completed-run member coordination closed without a provider effect"
        }
    };
    ledger.append_action(
        &member.id,
        "closed",
        MemberActionStatus::Succeeded,
        action_detail,
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
