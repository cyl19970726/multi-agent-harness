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
/// write is the refreshed profile itself.
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

/// What one member needed before this Supervisor generation could drive it.
pub(crate) enum JoinedMemberRuntimeFabric {
    /// The member already had exactly one current AgentSession. Nothing was
    /// written and no live control state was touched.
    AlreadyProvisioned,
    /// The member had none, so this Supervisor generation materialized one and
    /// bound it to itself.
    Provisioned { session_id: String },
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
/// It is deliberately a no-op once the member has one current session:
/// re-binding a live member would rewrite its `runtime_residency`/`activity`
/// back to Detached/Idle and lie about an attached provider handle. More than
/// one current session stays the `AGENT_SESSION_AMBIGUOUS` refusal it has
/// always been, so the one-current-session-per-AgentMember guard is unchanged.
pub(crate) fn ensure_joined_member_runtime_fabric(
    ledger: &TeamRunLedger,
    member: &mut ProviderRuntimeProjection,
) -> CliResult<JoinedMemberRuntimeFabric> {
    // A declared external interactive member is driven by the user in their
    // own already-open provider session; the Supervisor owns no session for it.
    if member.is_external_interactive() {
        return Ok(JoinedMemberRuntimeFabric::AlreadyProvisioned);
    }
    let lease = supervisor_fabric_authority(ledger)?;
    // The Host appended a new AgentTeamRun row when it admitted this member,
    // so the roster the Supervisor started with is stale by construction.
    // Scope resolution refuses a stale run outright (`TEAM_RUN_CHANGED`).
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    let execution_space_id = team_run_execution_space_id(&ledger.store, &run)?;
    if !member_needs_agent_session(&ledger.store, &execution_space_id, member)? {
        return Ok(JoinedMemberRuntimeFabric::AlreadyProvisioned);
    }
    prepare_member_provider_profile_for_start(&ledger.store, &run, member)?;
    let session_id =
        provision_member_agent_session(ledger, &lease, &run, &execution_space_id, member)?;
    Ok(JoinedMemberRuntimeFabric::Provisioned { session_id })
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
/// generation, returning the resulting session id.
///
/// Deliberately probe-free: the caller has already frozen the provider profile
/// the composition fence is derived from, and this durable half must stay
/// deterministic. Both steps are idempotent — `ensure_team_runtime_fabric`
/// adopts an existing session instead of minting a second one, and
/// `bind_team_runtime_supervisor` returns early when the control state already
/// names this generation.
pub(crate) fn provision_member_agent_session(
    ledger: &TeamRunLedger,
    lease: &TeamSupervisorLease,
    run: &AgentTeamRun,
    execution_space_id: &str,
    member: &ProviderRuntimeProjection,
) -> CliResult<String> {
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
        [session] => Ok(session.id.clone()),
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
/// Both fences are checked because the session records the NodeDaemon
/// generation that owns it: minting one beneath a daemon generation that has
/// already moved would be a durable lie about who can drive it. Every refusal
/// here is an ordinary, honest error — the member never reached a provider, so
/// there is no ambiguous effect for a recovery path to settle.
fn supervisor_fabric_authority(ledger: &TeamRunLedger) -> CliResult<TeamSupervisorLease> {
    ledger.require_supervisor_lease()?;
    let lease = ledger
        .store
        .latest_team_supervisor_lease(&ledger.run_id)?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "TEAM_SUPERVISOR_LEASE_REQUIRED: TeamRun {} has no durable Supervisor lease",
                ledger.run_id
            ))
        })?;
    if lease.status != harness_core::TeamSupervisorLeaseStatus::Active
        || lease.supervisor_id != ledger.supervisor_id
        || lease.generation != ledger.supervisor_generation
    {
        return Err(CliError::Usage(format!(
            "TEAM_SUPERVISOR_LEASE_FENCED: TeamRun {} is held by {} generation {}, not {} generation {}",
            ledger.run_id,
            lease.supervisor_id,
            lease.generation,
            ledger.supervisor_id,
            ledger.supervisor_generation
        )));
    }
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
