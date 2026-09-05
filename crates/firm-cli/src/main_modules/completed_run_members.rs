use super::*;

pub(super) const COMPLETED_RUN_SERVING_POLL_INTERVAL: Duration = Duration::from_secs(1);

pub(super) fn is_unclosed_managed_member(
    member: &ProviderRuntimeProjection,
    team_run_id: &str,
) -> bool {
    member.team_run_id == team_run_id
        && !member.is_external_interactive()
        && member.coordination_is_active()
}

pub(super) fn unclosed_managed_member_count(
    members: &[ProviderRuntimeProjection],
    team_run_id: &str,
) -> usize {
    members
        .iter()
        .filter(|member| is_unclosed_managed_member(member, team_run_id))
        .count()
}

/// Managed members whose Team-scoped runtime has not been explicitly closed.
///
/// TeamRun completion is coordination state, not provider-runtime teardown.
/// External-interactive members have no daemon-owned adapter, while Closed and
/// Retired members have already left the managed lane.
pub(super) fn unclosed_managed_members(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<Vec<ProviderRuntimeProjection>> {
    Ok(latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| is_unclosed_managed_member(member, team_run_id))
        .collect())
}

pub(super) fn completed_serving_label(unclosed_members: usize) -> String {
    format!("completed ({unclosed_members} unclosed member(s))")
}

/// Close an unclosed managed member of a COMPLETED TeamRun when its runtime is
/// provably over (#812). After a daemon restart the re-adopted Supervisor
/// serves the lane for Close authority without starting a new provider cycle,
/// so there is no live control handle to answer CloseMember. The canonical
/// detached+idle AgentSession at a terminal turn boundary — last driven by a
/// current or superseded Supervisor generation of this exact TeamRun on this
/// NodeDaemon, with the member's matching native-session authority and no
/// ambiguous RuntimeCommand — is positive evidence that this member's runtime
/// generation has already ended. The ordinary latch and coordination write
/// path then close the member without fabricating a provider Close receipt.
/// Returns Ok(None) when the lane is still Attached or the proof does not
/// hold; the caller then uses the ordinary live-control close, which returns a
/// real provider receipt.
pub(crate) fn close_completed_run_member_coordination(
    store: &HarnessStore,
    team_run_id: &str,
    member: &ProviderRuntimeProjection,
    supervisor: &TeamSupervisorLease,
    requested_by: &str,
    reason: &str,
) -> CliResult<Option<serde_json::Value>> {
    use harness_core::agentfirm_api::{
        RuntimeCommandStatus, RuntimeDriverRef, RuntimeEffectCertainty, RuntimeResidency,
    };

    let ledger = TeamRunLedger::without_supervisor(store, team_run_id);
    let (execution_space_id, session) = provider_session_for_member(&ledger, member)?;
    if session.control_state.runtime_residency != RuntimeResidency::Detached
        || !session_is_at_terminal_turn_boundary(&session)
    {
        return Ok(None);
    }
    // The recorded driver names the generation that LAST drove the session;
    // after a daemon restart that is the drained predecessor, not the current
    // Supervisor. Detached residency above already proves no live handle
    // exists, so any current-or-superseded Supervisor generation of this exact
    // TeamRun on this NodeDaemon is acceptable. A newer generation or a
    // foreign driver means this Close raced a successor: fail closed.
    let current_or_superseded_driver = matches!(
        &session.control_state.driver_ref,
        RuntimeDriverRef::TeamSupervisor {
            team_run_id: driver_team_run_id,
            team_supervisor_generation,
            ..
        } if driver_team_run_id == team_run_id
            && *team_supervisor_generation <= supervisor.generation
    );
    if !current_or_superseded_driver
        || supervisor.node_daemon_id != session.node_daemon_id
        || supervisor.node_daemon_generation < session.node_daemon_generation
    {
        return Ok(None);
    }
    let native_matches = match (
        member.native_session.as_ref(),
        session.native_session_ref.as_ref(),
    ) {
        (Some(member_native), Some(session_native)) => {
            member_native.native_session_id == session_native.native_session_id
        }
        _ => false,
    };
    if !native_matches {
        return Ok(None);
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
        return Ok(None);
    }

    let close = latch_member_close_for_supervisor(
        store,
        team_run_id,
        &member.id,
        requested_by,
        reason,
        &supervisor.supervisor_id,
        supervisor.generation,
    )?;
    cancel_unanswered_provider_messages(store, team_run_id, &member.id, requested_by, reason)?;
    let closed = mark_member_coordination_closed(store, team_run_id, &member.id)?;
    let mut closed_member = closed.clone();
    closed_member.status = MemberRunStatus::Stopped;
    closed_member.finished_at = Some(now_string());
    closed_member.last_event_at = Some(now_string());
    store_conflict_as_usage(store.compare_and_append_member_run(&closed, &closed_member))?;
    store_conflict_as_usage(store.complete_team_member_close(
        team_run_id,
        &member.id,
        &close.id,
        &now_string(),
    ))?;
    ledger.append_action(
        &member.id,
        "closed",
        MemberActionStatus::Succeeded,
        "member runtime provably over at completed-run Close; coordination closed without a provider effect",
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
    Ok(Some(serde_json::json!({
        "member_run_id": closed_member.id,
        "status": "stopped",
        "coordination_status": "closed",
        "runtime": "not_live",
        "runtime_effect": "none",
        "coordination_effect": "member_closed",
        "idempotent": false,
    })))
}

/// Members a Supervisor start/reattach should drive. A Completed run is
/// adopted only to keep its Close lane reachable: its members never claim
/// Work again, so no member lane is spawned. Spawning one would resume a
/// native session whose runtime the predecessor settlement is concurrently
/// proving terminal; the fenced resume then settles RecoveryRequired/Unknown
/// and blocks the next daemon stop (#812). Close for such members goes
/// through the completed-run coordination Close path instead.
pub(crate) fn members_to_drive_for_start(
    store: &HarnessStore,
    run_id: &str,
    run_status: TeamRunStatus,
) -> CliResult<Vec<ProviderRuntimeProjection>> {
    if run_status == TeamRunStatus::Completed {
        return Ok(Vec::new());
    }
    Ok(latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == run_id && member.coordination_is_active())
        .filter(|member| {
            !matches!(
                member.status,
                MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
            )
        })
        .collect())
}
