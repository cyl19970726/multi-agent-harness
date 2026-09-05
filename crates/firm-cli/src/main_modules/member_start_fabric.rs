//! What one member needs before its first provider effect of a TeamRun.
//!
//! Adoption prepares the whole founding roster in one pass
//! (`prepare_team_run_start_body` + `ensure_team_runtime_fabric` +
//! `bind_team_runtime_supervisor`). A member admitted into an already-running
//! TeamRun by `team-run add-member` arrives long after that pass, so this
//! module owns the per-member half of it and both paths call the same code.

use super::*;

/// Refresh one member's version-specific provider profile, freeze its
/// permission enforcement into that profile, and persist the result.
///
/// This must run before any AgentSession composition fence is derived from the
/// profile: recomputing the permission mapping only after the runtime spawned
/// would leave the profile and the session's exact composition fence
/// disagreeing on the first provider effect.
///
/// Errors are ordinary refusals with no provider side effect: the probe reads
/// the adapter registry and the executable's version, and the only durable
/// write is the refreshed profile itself. That write is a CAS, so a concurrent
/// Host append loses it as `CliError::Store(Conflict)` — an attempt-scoped
/// race the caller must retry, never a verdict on the member
/// (DEV-149-REVIEW-02).
pub(crate) fn prepare_member_provider_profile_for_start(
    store: &HarnessStore,
    run: &AgentTeamRun,
    member: &mut ProviderRuntimeProjection,
) -> CliResult<()> {
    if member.is_external_interactive() {
        return Ok(());
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
        effective_member_permission_ceiling(store, permission_ceiling, run, member)?;
    apply_permission_enforcement_to_profile(&mut profile, permission_ceiling)?;
    let resolution = resolve_provider_compatibility(store, &profile, probe_error.as_deref())?;
    let refusal = provider_compatibility_block_reason(
        member,
        &profile,
        &resolution,
        "start or resume persistent Agent Team execution",
    );
    if apply_refreshed_provider_profile(member, profile) {
        persist_refreshed_member_profile(store, &expected, member)?;
    }
    if let Some(refusal) = refusal {
        return Err(CliError::Usage(refusal));
    }
    Ok(())
}

/// Members that joined, or were Reopened, while this Supervisor generation was
/// already driving, and that the current pass must therefore admit.
///
/// A Completed run never spawns another member lane (#812): its members cannot
/// claim Work again, and a resume would race the predecessor runtime
/// settlement, so the whole selection is empty there.
pub(crate) fn members_joined_since_last_pass(
    latest_members: Vec<ProviderRuntimeProjection>,
    run_id: &str,
    run_status: TeamRunStatus,
    seen_runtime_generations: &HashMap<String, u64>,
    already_driven: impl Fn(&str) -> bool,
) -> Vec<ProviderRuntimeProjection> {
    if run_status == TeamRunStatus::Completed {
        return Vec::new();
    }
    latest_members
        .into_iter()
        .filter(|member| {
            member.team_run_id == run_id
                && !member.is_external_interactive()
                && member.coordination_is_active()
                && !matches!(
                    member.status,
                    MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
                )
                && member.runtime_generation
                    > seen_runtime_generations
                        .get(&member.id)
                        .copied()
                        .unwrap_or(0)
                && !already_driven(&member.id)
        })
        .collect()
}

/// What one member needed before this Supervisor generation could drive it.
#[derive(Debug)]
pub(crate) enum JoinedMemberRuntimeFabric {
    /// The member already had exactly one current AgentSession. Nothing was
    /// written and no live control state was touched.
    AlreadyProvisioned,
    /// The member had none, so this Supervisor generation materialized one and
    /// bound it to itself.
    Provisioned { session_id: String },
}

/// What the Supervisor loop must do about a failed provisioning attempt.
///
/// `MemberRunStatus::Failed` is a one-way door — `prepare_team_run_start_body`
/// and the loop's own rescan both filter Failed out, Reopen short-circuits on
/// it, and the member's name can no longer be re-added — so only a refusal
/// that is durably about *this member* may reach it. Everything that describes
/// the attempt instead (a lost CAS, a moved fence, a changed TeamRun row) is
/// retried, and a lost Supervisor lease quiesces this generation without
/// touching member state at all.
pub(crate) enum MemberFabricFailure {
    /// This Supervisor generation no longer owns the run. It must stop driving
    /// and leave every member exactly as the successor will find it.
    LeaseLost,
    /// A property of this attempt, not of the member. Retry it.
    Transient,
    /// A durable refusal about this member.
    Structural,
}

/// How many times one member's fabric provisioning may lose an attempt-scoped
/// race before this generation gives up on it. Matches the CAS retry budget
/// the member lifecycle already uses for the same class of contention.
pub(crate) const MEMBER_FABRIC_PROVISION_ATTEMPTS: u32 = PROVIDER_MEMBER_CAS_RETRIES as u32;

/// Classify a provisioning failure typed-first, reusing the adoption path's
/// own transient-code table so the two can never drift apart.
pub(crate) fn classify_member_fabric_failure(error: &CliError) -> MemberFabricFailure {
    if error.is_supervisor_lease_lost() {
        return MemberFabricFailure::LeaseLost;
    }
    if crate::supervisor_daemon::recovery::start_failure_is_transient(error) {
        return MemberFabricFailure::Transient;
    }
    MemberFabricFailure::Structural
}

/// Give a member admitted into an already-running TeamRun the same runtime
/// fabric a founding member gets.
///
/// Adoption runs [`prepare_member_provider_profile_for_start`],
/// [`ensure_team_runtime_fabric`] and [`bind_team_runtime_supervisor`] exactly
/// once, over the roster the Supervisor was started with. `team-run
/// add-member` admits a MemberRun long after that seam has passed, so a joined
/// member reached its first provider attempt with zero AgentSessions and
/// failed with `AGENT_SESSION_AMBIGUOUS: ... found 0` — reported to the Host as
/// `runtime_recovery_required` even though nothing needed recovering (#749).
/// This runs the same functions, for exactly that one member, under the exact
/// durable authority this Supervisor holds.
///
/// The needs-a-session question is answered before any authority is consulted,
/// so a founding member that already owns its session reads two projections
/// and returns: it can neither be re-bound (which would reset its
/// `runtime_residency`/`activity` and lie about an attached provider handle)
/// nor fail on a lease or daemon fence this call never uses.
///
/// The sole caller filters external interactive members out two statements
/// earlier: they are driven by the user in their own already-open provider
/// session, and the Supervisor owns no session for them.
pub(crate) fn ensure_joined_member_runtime_fabric(
    ledger: &TeamRunLedger,
    member: &mut ProviderRuntimeProjection,
) -> CliResult<JoinedMemberRuntimeFabric> {
    // The Host appended a new AgentTeamRun row when it admitted this member,
    // so the roster the Supervisor started with is stale by construction.
    // Scope resolution refuses a stale run outright (`TEAM_RUN_CHANGED`).
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    let execution_space_id = team_run_execution_space_id(&ledger.store, &run)?;
    if !member_needs_agent_session(&ledger.store, &execution_space_id, member)? {
        return Ok(JoinedMemberRuntimeFabric::AlreadyProvisioned);
    }
    let lease = supervisor_fabric_authority(ledger)?;
    prepare_member_provider_profile_for_start(&ledger.store, &run, member)?;
    provision_member_agent_session(ledger, &lease, &run, &execution_space_id, member)
}

/// Whether this member still owes the Supervisor its one AgentSession.
///
/// More than one current session stays the `AGENT_SESSION_AMBIGUOUS` refusal it
/// has always been, so the one-current-session-per-AgentMember guard is
/// unchanged by this seam.
pub(crate) fn member_needs_agent_session(
    store: &HarnessStore,
    execution_space_id: &str,
    member: &ProviderRuntimeProjection,
) -> CliResult<bool> {
    match current_member_sessions(store, execution_space_id, member)?.len() {
        0 => Ok(true),
        1 => Ok(false),
        found => Err(ambiguous_session_error(member, execution_space_id, found)),
    }
}

/// Materialize one member's AgentSession and bind it to this Supervisor
/// generation.
///
/// The "never re-bind a member that already owns a session" rule is enforced
/// here rather than left to a doc comment, so the function is safe to call
/// directly and idempotent by construction. Past that guard it is deliberately
/// probe-free: the caller has already frozen the provider profile the
/// composition fence is derived from, and this durable half must stay
/// deterministic.
pub(crate) fn provision_member_agent_session(
    ledger: &TeamRunLedger,
    lease: &TeamSupervisorLease,
    run: &AgentTeamRun,
    execution_space_id: &str,
    member: &ProviderRuntimeProjection,
) -> CliResult<JoinedMemberRuntimeFabric> {
    if !member_needs_agent_session(&ledger.store, execution_space_id, member)? {
        return Ok(JoinedMemberRuntimeFabric::AlreadyProvisioned);
    }
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members: vec![member.clone()],
    };
    ensure_team_runtime_fabric(
        &ledger.store,
        &body,
        execution_space_id,
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )?;
    bind_team_runtime_supervisor(
        &ledger.store,
        &body,
        execution_space_id,
        &lease.node_daemon_id,
        &ledger.supervisor_id,
        ledger.supervisor_generation,
    )?;
    match current_member_sessions(&ledger.store, execution_space_id, member)?.as_slice() {
        [session] => Ok(JoinedMemberRuntimeFabric::Provisioned {
            session_id: session.id.clone(),
        }),
        rows => Err(ambiguous_session_error(
            member,
            execution_space_id,
            rows.len(),
        )),
    }
}

/// The exact durable authority this Supervisor generation may write
/// AgentSession fabric under.
///
/// `require_supervisor_lease` already proves this generation owns the run; the
/// row is re-read only to carry its NodeDaemon fence. A lease that moved in
/// between is the same loss that check reports, so it is spelled with the same
/// typed error and the same latch rather than an invented code. The daemon
/// fence is checked too because the session records the generation that owns
/// it, and minting one beneath a generation that has already moved would be a
/// durable lie about who may drive it; that refusal carries the existing
/// `NODE_DAEMON_GENERATION_FENCED` token, which the adoption classifier
/// already reads as attempt-scoped.
fn supervisor_fabric_authority(ledger: &TeamRunLedger) -> CliResult<TeamSupervisorLease> {
    ledger.require_supervisor_lease()?;
    let lease = match ledger.store.latest_team_supervisor_lease(&ledger.run_id)? {
        Some(lease)
            if lease.status == harness_core::TeamSupervisorLeaseStatus::Active
                && lease.supervisor_id == ledger.supervisor_id
                && lease.generation == ledger.supervisor_generation =>
        {
            lease
        }
        _ => {
            return Err(latch_supervisor_lease_lost(
                &ledger.supervisor_valid,
                &ledger.run_id,
                &ledger.supervisor_id,
                ledger.supervisor_generation,
                "durable lease moved or was released while provisioning member runtime fabric",
            ))
        }
    };
    let daemon = ledger
        .store
        .latest_node_daemon_lease(&lease.node_id)?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "NODE_DAEMON_GENERATION_FENCED: Node {} has no current NodeDaemon lease",
                lease.node_id
            ))
        })?;
    if daemon.status != NodeDaemonLeaseStatus::Active
        || daemon.daemon_id != lease.node_daemon_id
        || daemon.generation != lease.node_daemon_generation
    {
        return Err(CliError::Usage(format!(
            "NODE_DAEMON_GENERATION_FENCED: Node {} is served by {} generation {}, not {} generation {}",
            lease.node_id,
            daemon.daemon_id,
            daemon.generation,
            lease.node_daemon_id,
            lease.node_daemon_generation
        )));
    }
    Ok(lease)
}

fn current_member_sessions(
    store: &HarnessStore,
    execution_space_id: &str,
    member: &ProviderRuntimeProjection,
) -> CliResult<Vec<harness_core::agentfirm_api::AgentSession>> {
    Ok(store
        .fabric_agent_sessions(execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == member.agent_member_id
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .collect())
}

/// Spelled exactly like the provider-attempt guard in `runtime_effects`, so a
/// refusal reads the same wherever the one-current-session rule is enforced.
fn ambiguous_session_error(
    member: &ProviderRuntimeProjection,
    execution_space_id: &str,
    found: usize,
) -> CliError {
    CliError::Usage(format!(
        "AGENT_SESSION_AMBIGUOUS: member {} requires one current session in Execution Space {execution_space_id}, found {found}",
        member.id
    ))
}
