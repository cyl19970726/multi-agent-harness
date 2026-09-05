//! Completed-TeamRun member serving and receipt-free member Close (#812).
//!
//! A Completed run keeps its Supervisor lease until every managed member is
//! explicitly Closed. Before a daemon restart the already-running member lanes
//! survive, so Close goes through the ordinary live-control path and returns a
//! real provider receipt. After a restart the re-adopted Supervisor spawns NO
//! member lanes: a Completed run's members can never claim Work again, and a
//! native-session resume would only race the predecessor runtime settlement.
//! Close then goes only through the coordination path in this module, which
//! reuses the detached-recovery Close on a current-or-superseded generation
//! axis, gated on the recorded predecessor evidence.

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
/// serves the run for Close authority without spawning a member lane, so there
/// is no live control handle to answer CloseMember. This reuses the
/// detached-recovery Close with the `CompletedRunMember` generation axis: the
/// exact-current generation needs no extra proof, while a superseded driver
/// generation must carry the recorded predecessor evidence. A failed proof is
/// a typed fenced error, never a silent fall-through; an Attached lane returns
/// Ok(None) so the caller uses the ordinary live-control close with its real
/// provider receipt.
pub(crate) fn close_completed_run_member_coordination(
    store: &HarnessStore,
    team_run_id: &str,
    member: &ProviderRuntimeProjection,
    supervisor: &TeamSupervisorLease,
    requested_by: &str,
    reason: &str,
) -> CliResult<Option<serde_json::Value>> {
    close_detached_blocked_member_for_recovery_with_hooks(
        store,
        team_run_id,
        member,
        supervisor,
        requested_by,
        reason,
        DetachedRecoveryCloseMode::CompletedRunMember,
        |_| Ok(()),
        |_| Ok(()),
    )
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

/// Generation evidence for the completed-run receipt-free Close (#812).
///
/// The exact-current case needs no extra proof. Anything else needs the one
/// evidence class that actually proves the predecessor's provider process
/// groups ended: a NEWER NodeDaemon generation holding the machine. A
/// successor daemon generation exists only because the session's NodeDaemon
/// generation reached `Released`, and a NodeDaemon lease reaches `Released`
/// only through a clean drain — which terminates the registered provider
/// process groups before the release and revalidates settlement at release
/// (`supervisor_daemon.rs` shutdown order) — or through an explicit
/// `daemon recover-predecessor` (the Operator's confirmed
/// `provider_process_groups_terminated_confirmed` fact).
///
/// A Supervisor lease `Released` row is deliberately NOT evidence:
/// `TeamSupervisorRegistration::drop` writes it on every Supervisor exit —
/// normal exit, lease-lost quiesce, or error — without terminating anything,
/// and even the drain writes it before process-group termination begins. The
/// same-daemon superseded-Supervisor case is therefore refused outright: no
/// daemon-lease fence and no recover-predecessor ever intervenes there. A
/// newer or foreign generation means this Close raced a successor: fail
/// closed with a typed reason, never `Ok(None)`.
pub(crate) fn require_completed_run_close_generation_evidence(
    member: &ProviderRuntimeProjection,
    session: &harness_core::agentfirm_api::AgentSession,
    supervisor: &TeamSupervisorLease,
    driver_supervisor: Option<&(String, u64)>,
    exact_current_driver: bool,
    exact_current_daemon: bool,
) -> CliResult<()> {
    if supervisor.node_daemon_id != session.node_daemon_id
        || supervisor.node_daemon_generation < session.node_daemon_generation
        || driver_supervisor.is_none_or(|(_, generation)| *generation > supervisor.generation)
    {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} session {} names a newer or foreign Supervisor/NodeDaemon generation than the current authority",
            member.id, session.id
        )));
    }
    if exact_current_driver && exact_current_daemon {
        return Ok(());
    }
    if driver_supervisor.is_none() {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "DETACHED_MEMBER_RECOVERY_FENCED: member {} session {} has no TeamSupervisor driver lineage for this TeamRun",
            member.id, session.id
        )));
    }
    if supervisor.node_daemon_generation > session.node_daemon_generation {
        // A newer daemon holds the machine, so the session's NodeDaemon
        // generation reached Released — written only by a terminating drain
        // or an Operator-confirmed predecessor recovery.
        return Ok(());
    }
    Err(CliError::RuntimeRecoveryRequired(format!(
        "DETACHED_MEMBER_RECOVERY_FENCED: member {} session {} was last driven by a superseded Supervisor generation under the still-live NodeDaemon generation {}; the predecessor Supervisor's exit does not prove provider process-group termination — drain-restart the daemon or reopen the member so its lane rebinds before Close",
        member.id, session.id, session.node_daemon_generation
    )))
}
