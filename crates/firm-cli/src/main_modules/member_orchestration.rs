use super::*;

/// Terminal outcome of one member's orchestration, for the run summary.
pub(super) struct MemberOutcome {
    pub(super) name: String,
    pub(super) role: String,
    pub(super) provider: String,
    pub(super) status: MemberRunStatus,
    pub(super) summary: String,
}

pub(super) fn provider_turn_failure_summary(provider: &str, round: u32) -> String {
    format!(
        "{provider} provider round {round} failed; inspect the provider-native session for details"
    )
}

pub(super) struct MemberRuntimeContext {
    pub(super) execution_space_id: Option<String>,
    pub(super) project_id: Option<String>,
    pub(super) project_selector: Option<String>,
    pub(super) cwd: PathBuf,
    pub(super) idle_timeout: Duration,
    pub(super) live_sink: Option<NativeSessionWakeSink>,
    pub(super) turn_leases: Arc<ActiveTurnLeasePool>,
    /// Unpersisted bearer capability that binds member-originated Role
    /// Actions to this exact live Supervisor registration. The provider sees
    /// only its own token; the Supervisor owns the token-to-identity map.
    pub(super) role_action_token: String,
}

pub(super) fn generated_member_role_action_token() -> CliResult<String> {
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

impl MemberOutcome {
    pub(super) fn new(
        member: &ProviderRuntimeProjection,
        status: MemberRunStatus,
        summary: String,
    ) -> Self {
        Self {
            name: member.name.clone(),
            role: member.role.clone(),
            provider: member.provider.clone(),
            status,
            summary,
        }
    }
}

pub(crate) struct PreparedTeamRunBody {
    pub(super) run_id: String,
    pub(super) objective: String,
    pub(super) run: AgentTeamRun,
    pub(super) members: Vec<ProviderRuntimeProjection>,
}

pub(crate) struct PreparedTeamRunStart {
    pub(super) run_id: String,
    pub(super) objective: String,
    pub(super) running: AgentTeamRun,
    pub(super) members: Vec<ProviderRuntimeProjection>,
    pub(super) ledger: Arc<TeamRunLedger>,
    pub(super) supervisor_registration: TeamSupervisorRegistration,
}

/// Validate the durable collaboration overlay, then materialize/recover only
/// machine-local AgentSessions before any provider child can be spawned.
/// TeamMembership is never a TeamRun side effect.
#[cfg(unix)]
pub(crate) fn ensure_team_runtime_fabric(
    store: &HarnessStore,
    body: &PreparedTeamRunBody,
    execution_space_id: &str,
    daemon_id: &str,
    daemon_generation: u64,
) -> CliResult<()> {
    use harness_core::agentfirm_api::{
        ActorKind, ActorRef, AgentSession, AgentSessionStatus, MutationContext,
        TeamMembershipStatus,
    };
    let daemon_actor = ActorRef {
        kind: ActorKind::Service,
        id: daemon_id.to_string(),
    };
    let timestamp = now_string();
    let canonical_members = store
        .trust_agent_members(execution_space_id)
        .map_err(CliError::Store)?;
    let team = store
        .latest_teams()?
        .remove(&body.run.agent_team_id)
        .ok_or_else(|| CliError::Usage("TeamRun references a missing AgentTeam".into()))?;
    let memberships = store.fabric_team_memberships(execution_space_id)?;
    for member in body
        .members
        .iter()
        .filter(|member| !member.is_external_interactive())
    {
        let durable = canonical_members
            .iter()
            .find(|candidate| candidate.id == member.agent_member_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "AGENT_IDENTITY_NOT_FOUND: MemberRun {} references missing canonical AgentMember {}",
                    member.id, member.agent_member_id
                ))
            })?;
        let exact_memberships = memberships
            .iter()
            .filter(|membership| {
                membership.team_id == team.id
                    && membership.agent_member_id == durable.id
                    && membership.node_id == team.node_id
                    && membership.state == TeamMembershipStatus::Active
            })
            .count();
        if exact_memberships != 1 {
            return Err(CliError::Usage(format!(
                "TEAM_MEMBERSHIP_REQUIRED: MemberRun {} requires one durable active TeamMembership, found {}",
                member.id, exact_memberships
            )));
        }
        let permission_ceiling = effective_member_permission_ceiling(
            store,
            durable.permission_ceiling,
            &body.run,
            member,
        )?;
        crate::provider_adapter::map_permission(&member.provider, permission_ceiling)
            .map_err(CliError::Usage)?;
        let current_sessions = store
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_member_id == durable.id
                    && session.lifecycle != AgentSessionStatus::Closed
            })
            .collect::<Vec<_>>();
        if current_sessions.len() > 1 {
            return Err(CliError::Usage(format!(
                "AGENT_SESSION_AMBIGUOUS: {} has multiple current sessions",
                durable.id
            )));
        }
        if let Some(session) = current_sessions.first() {
            if session.node_id != body.run.execution_node_id
                || session.provider_kind != member.provider
            {
                return Err(CliError::Usage(format!(
                    "AGENT_SESSION_RECOVERY_REQUIRED: {} is bound to another node or provider",
                    session.id
                )));
            }
            let expected_native_session = expected_agentfirm_native_session_ref(member);
            let native_session_matches = agentfirm_native_session_identity_matches(
                session.native_session_ref.as_ref(),
                expected_native_session.as_ref(),
            );
            let admission_native_session_matches =
                agentfirm_native_session_identity_matches_for_admission(
                    session.native_session_ref.as_ref(),
                    expected_native_session.as_ref(),
                );
            if !native_session_matches && !admission_native_session_matches {
                return Err(CliError::Usage(format!(
                    "AGENT_SESSION_RECOVERY_REQUIRED: {} does not match MemberRun {} native-session truth",
                    session.id, member.id
                )));
            }
            if session.node_daemon_id != daemon_id
                || session.node_daemon_generation != daemon_generation
            {
                store.reattach_agent_session_to_node_daemon(
                    &MutationContext {
                        execution_space_id: execution_space_id.to_string(),
                        authenticated_actor: daemon_actor.clone(),
                        authority_actor: None,
                        command_name: "runtime_fabric.session.reattach_node_daemon".into(),
                        idempotency_key: format!(
                            "session-daemon-reattach:{}:{}:{}",
                            session.id, session.node_daemon_generation, daemon_generation
                        ),
                        expected_version: session.version,
                        request_fingerprint: None,
                    },
                    &session.id,
                    session.runtime_generation,
                    session.node_daemon_generation,
                    daemon_id,
                    daemon_generation,
                    &timestamp,
                )?;
            }
            // A lane a NodeDaemon drain left `Interrupted` re-enters the
            // ordinary lane here, on the exact generation that just adopted it
            // and before any provider effect is prepared. Doing it later is
            // what wedged #779: the member runner publishes an Attached
            // residency before its first cycle projects the Session Active, so
            // by then the DEV-171 fence — which requires a detached, disarmed
            // lane — can no longer admit the hop the runner needs. A lane a
            // runner left `RecoveryRequired` (#755) takes the same hop once an
            // operator has reconciled it: the drain skipped it as already
            // settled, the successor just reattached it, and the Store
            // re-proves the same terminated-lane clauses under its lock.
            if matches!(
                session.lifecycle,
                AgentSessionStatus::Interrupted | AgentSessionStatus::RecoveryRequired
            ) {
                let reattached = current_agent_session(store, execution_space_id, &session.id)?;
                if resume_drained_lane_for_adoption(
                    store,
                    execution_space_id,
                    daemon_id,
                    &reattached,
                    &timestamp,
                )? == DrainedLaneResume::NotYetResumable
                {
                    eprintln!(
                        "[node-daemon] AgentSession {} stays out of the ordinary lane: it does not yet prove the dead runtime is gone; the next adoption pass retries",
                        reattached.id
                    );
                }
            }
        } else {
            let native_session_ref = expected_agentfirm_native_session_ref(member);
            store.create_agent_session(
                &MutationContext {
                    execution_space_id: execution_space_id.to_string(),
                    authenticated_actor: daemon_actor.clone(),
                    authority_actor: None,
                    command_name: "runtime_fabric.session.materialize".into(),
                    idempotency_key: format!(
                        "session:{}:{}:{}:{}",
                        durable.id,
                        body.run.execution_node_id,
                        daemon_generation,
                        member.runtime_generation
                    ),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                AgentSession {
                    id: format!(
                        "agent-session:{}:{}:{}:{}",
                        durable.id,
                        body.run.execution_node_id,
                        daemon_generation,
                        member.runtime_generation
                    ),
                    agent_member_id: durable.id.clone(),
                    node_id: body.run.execution_node_id.clone(),
                    execution_space_id: execution_space_id.to_string(),
                    node_daemon_id: daemon_id.to_string(),
                    node_daemon_generation: daemon_generation,
                    provider_kind: member.provider.clone(),
                    provider_profile_ref: member
                        .provider_profile
                        .as_ref()
                        .and_then(|profile| profile.adapter_contract_version.clone())
                        .unwrap_or_else(|| format!("{}:default", member.provider)),
                    permission_envelope_ref: format!("agent-member:{}:permission", durable.id),
                    effective_permission_ceiling: permission_ceiling,
                    workspace_cwd: member.provider_cwd_hint.clone(),
                    lifecycle: AgentSessionStatus::Cold,
                    runtime_generation: member.runtime_generation,
                    control_state: agent_session_control_state_for_profile(
                        member.provider_profile.as_ref(),
                        daemon_id,
                        daemon_generation,
                        member.runtime_generation,
                    ),
                    native_session_ref,
                    current_turn_id: None,
                    queued_input_count: 0,
                    version: 1,
                    opened_at: timestamp.clone(),
                    last_active_at: timestamp.clone(),
                    closed_at: None,
                },
            )?;
        }
    }
    Ok(())
}

/// Bind every managed member Session to the exact TeamSupervisor generation
/// after its durable lease exists and before any provider handle is opened.
/// The Store performs the quiescence and successor checks under its writer
/// lock; this helper only supplies the desired bounded projection.
#[cfg(unix)]
pub(crate) fn bind_team_runtime_supervisor(
    store: &HarnessStore,
    body: &PreparedTeamRunBody,
    execution_space_id: &str,
    daemon_id: &str,
    supervisor_id: &str,
    supervisor_generation: u64,
) -> CliResult<()> {
    use harness_core::agentfirm_api::{
        ActorKind, ActorRef, AgentSessionStatus, DriverHandoffState, MutationContext,
        NativeContinuationActivation, RuntimeActivity, RuntimeDriverRef, RuntimeResidency,
    };

    let daemon_actor = ActorRef {
        kind: ActorKind::Service,
        id: daemon_id.to_string(),
    };
    for member in body
        .members
        .iter()
        .filter(|member| !member.is_external_interactive())
    {
        let mut sessions = store
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_member_id == member.agent_member_id
                    && session.lifecycle != AgentSessionStatus::Closed
            })
            .collect::<Vec<_>>();
        if sessions.len() != 1 {
            return Err(CliError::Usage(format!(
                "AGENT_SESSION_AMBIGUOUS: supervisor binding for {} requires one current session, found {}",
                member.agent_member_id,
                sessions.len()
            )));
        }
        let session = sessions.pop().expect("one session");
        let target_driver = RuntimeDriverRef::TeamSupervisor {
            team_run_id: body.run.id.clone(),
            team_supervisor_id: supervisor_id.to_string(),
            team_supervisor_generation: supervisor_generation,
        };
        let already_bound = session.control_state.driver_ref == target_driver
            && session.control_state.execution_driver
                == member
                    .provider_profile
                    .as_ref()
                    .map(|profile| profile.execution_driver)
                    .unwrap_or(MemberExecutionDriver::HostDriven);
        let mut next = session.control_state.clone();
        next.runtime_residency = RuntimeResidency::Detached;
        next.activity = RuntimeActivity::Idle;
        next.execution_driver = member
            .provider_profile
            .as_ref()
            .map(|profile| profile.execution_driver)
            .unwrap_or(MemberExecutionDriver::HostDriven);
        next.driver_generation = if already_bound {
            session.control_state.driver_generation.max(1)
        } else {
            session
                .control_state
                .driver_generation
                .saturating_add(1)
                .max(1)
        };
        next.driver_ref = target_driver;
        next.handoff_state = DriverHandoffState::None;
        next.continuation.activation = NativeContinuationActivation::Disarmed;
        next.composition_fingerprint = member
            .provider_profile
            .as_ref()
            .and_then(|profile| profile.composition_fingerprint.clone());
        next.capability_fingerprint = member
            .provider_profile
            .as_ref()
            .and_then(|profile| profile.capability_fingerprint.clone());
        if session.control_state == next {
            continue;
        }
        store.bind_agent_session_control_state(
            &MutationContext {
                execution_space_id: execution_space_id.to_string(),
                authenticated_actor: daemon_actor.clone(),
                authority_actor: None,
                command_name: "node_daemon.team_supervisor.bind_session".into(),
                idempotency_key: format!(
                    "session-control:{}:{}:{}:{}",
                    session.id, supervisor_id, supervisor_generation, member.runtime_generation
                ),
                expected_version: session.version,
                request_fingerprint: None,
            },
            &session.id,
            session.runtime_generation,
            next,
            &now_string(),
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_team_message_fabric(
    store: &HarnessStore,
    team_run_id: &str,
    execution_space_id: &str,
    daemon_id: &str,
    daemon_generation: u64,
) -> CliResult<()> {
    let run = latest_team_run(store, team_run_id)?;
    let members = latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == run.id && member.coordination_is_active())
        .collect::<Vec<_>>();
    ensure_team_runtime_fabric(
        store,
        &PreparedTeamRunBody {
            run_id: run.id.clone(),
            objective: run.objective.clone(),
            run,
            members,
        },
        execution_space_id,
        daemon_id,
        daemon_generation,
    )
}

/// Validate the team run, filter active members, check provider compat, and
/// return the raw run + members WITHOUT reserving a supervisor or creating a
/// ledger. The caller (in-process path or daemon) builds the rest.
/// Write back a MemberRun whose provider profile was refreshed during start
/// preparation, keeping the Store's typed error.
///
/// This mapping lives in one named function so a test can pin it: a concurrent
/// Host write loses this CAS routinely, and flattening that Conflict into
/// `CliError::Usage` hides it from the adoption-hold classifier, which would
/// read an ordinary lost race as structural and wedge a healthy TeamRun
/// (DEV-149-REVIEW-02).
pub(crate) fn persist_refreshed_member_profile(
    store: &HarnessStore,
    expected: &ProviderRuntimeProjection,
    member: &ProviderRuntimeProjection,
) -> CliResult<()> {
    store
        .compare_and_append_member_run(expected, member)
        .map_err(CliError::Store)?;
    Ok(())
}

pub(crate) fn prepare_team_run_start_body(
    store: &HarnessStore,
    run_id: &str,
    _max_concurrency: usize,
) -> CliResult<PreparedTeamRunBody> {
    let run = latest_team_run(store, run_id)?;
    // Typed on purpose: a concurrent Host append makes this resolver return
    // `TEAM_RUN_CHANGED`, which is a lost race, not a defect of the TeamRun.
    team_run_execution_space_id_for_start(store, &run)?;
    if matches!(run.status, TeamRunStatus::Failed | TeamRunStatus::Cancelled) {
        return Err(CliError::Usage(format!(
            "team run {run_id} is {} and cannot attach a member supervisor",
            serde_snake_label(&run.status)
        )));
    }
    let mut members: Vec<ProviderRuntimeProjection> =
        crate::completed_run_members::members_to_drive_for_start(store, run_id, run.status)?;
    // Fail the whole start/reattach before reserving a Supervisor or moving the
    // TeamRun to running when any persistent adapter version is unreviewed.
    // The refreshed profile is still durable operator evidence; native-session
    // locators and Work/canonical delivery rows are intentionally untouched.
    for member in &mut members {
        prepare_member_provider_profile_for_start(store, &run, member)?;
    }
    Ok(PreparedTeamRunBody {
        run_id: run_id.to_string(),
        objective: run.objective.clone(),
        run,
        members,
    })
}

/// `firm team-run start`: delegate one admitted TeamRun to the machine-scoped
/// NodeDaemon. Public start surfaces never spawn a per-run daemon.
pub(crate) fn team_run_start(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    run_id: &str,
    max_concurrency: usize,
    idle_timeout: Duration,
) -> CliResult<()> {
    #[cfg(unix)]
    {
        let _ = idle_timeout;
        let delegated = delegate_team_run_to_node_daemon(store, resolved, run_id, max_concurrency)?;
        let node_id = delegated["node_id"].as_str().unwrap_or("unknown");
        let daemon_response = &delegated["daemon_response"];
        if daemon_response["already_managed"].as_bool() == Some(true) {
            let generation = daemon_response["daemon_generation"]
                .as_u64()
                .map(|generation| generation.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            println!(
                "team run {run_id}\talready managed by NodeDaemon {node_id} (gen {generation})"
            );
        } else {
            println!("team run {run_id}\tdelegated to NodeDaemon {node_id}");
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = (store, resolved, run_id, max_concurrency, idle_timeout);
        Err(CliError::Usage(
            "NodeDaemon execution currently requires Unix-domain sockets".to_string(),
        ))
    }
}

/// Validate a TeamRun locally and ask the one machine NodeDaemon to adopt it.
/// CLI and MCP share this boundary so no public control surface can spawn an
/// per-TeamRun supervisor process outside the machine NodeDaemon.
pub(crate) fn delegate_team_run_to_node_daemon(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    run_id: &str,
    max_concurrency: usize,
) -> CliResult<serde_json::Value> {
    let execution_space_id = resolved
        .execution_space_context
        .as_ref()
        .map(|space| space.id.as_str())
        .ok_or_else(|| {
            CliError::Usage(
                "team-run start requires an explicitly resolved Execution Space".to_string(),
            )
        })?;
    delegate_team_run_to_node_daemon_in_space(store, execution_space_id, run_id, max_concurrency)
}

pub(crate) fn delegate_team_run_to_node_daemon_in_space(
    store: &HarnessStore,
    execution_space_id: &str,
    run_id: &str,
    max_concurrency: usize,
) -> CliResult<serde_json::Value> {
    #[cfg(unix)]
    {
        let body = prepare_team_run_start_body(store, run_id, max_concurrency)?;
        let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
        let local_node_id = read_local_node_id()?;
        if body.run.execution_node_id != local_node_id {
            return Err(CliError::Usage(format!(
                "REMOTE_TEAM_RUN_NOT_ADOPTED: TeamRun {run_id} belongs to Node {}, local Node is {local_node_id}",
                body.run.execution_node_id
            )));
        }
        let response = match crate::supervisor_daemon::try_delegate_to_node_daemon(
            &firm_home,
            &local_node_id,
            execution_space_id,
            run_id,
        ) {
            Ok(response) => response,
            Err(error) if error.request_may_have_been_accepted() => {
                let status = crate::supervisor_daemon::daemon_status_via_socket(
                    &firm_home,
                    &local_node_id,
                );
                if let Some(reconciled) = status.as_deref().and_then(|status| {
                    crate::supervisor_daemon::reconcile_team_run_start_postcondition(
                        store,
                        status,
                        &local_node_id,
                        execution_space_id,
                        run_id,
                    )
                }) {
                    return reconciled;
                }
                return Err(CliError::Usage(format!(
                    "TEAM_RUN_START_RESULT_UNKNOWN: the request for TeamRun {run_id} may have been accepted by Node {local_node_id}, but the exact daemon/Supervisor postcondition could not be proven after transport failure: {error}; do not retry blindly—inspect daemon status and the canonical RuntimeCommand inventory, then use explicit recovery"
                )));
            }
            Err(error) => {
                return Err(CliError::Usage(format!(
                    "NODE_DAEMON_UNAVAILABLE: cannot reach Node {local_node_id} for TeamRun {run_id}: {error}; start it with `firm daemon start`"
                )))
            }
        };
        let parsed: serde_json::Value = serde_json::from_str(&response).map_err(|error| {
            CliError::Usage(format!(
                "NodeDaemon returned invalid JSON for {run_id}: {error}"
            ))
        })?;
        if parsed["ok"].as_bool() != Some(true) {
            return Err(CliError::Usage(format!(
                "NodeDaemon rejected {run_id}: {}",
                parsed["error"]
                    .as_str()
                    .unwrap_or("daemon returned no error detail")
            )));
        }
        Ok(serde_json::json!({
            "node_id": local_node_id,
            "execution_space_id": execution_space_id,
            "team_run_id": run_id,
            "daemon_response": parsed,
        }))
    }
    #[cfg(not(unix))]
    {
        let _ = (store, execution_space_id, run_id, max_concurrency);
        Err(CliError::Usage(
            "NodeDaemon execution currently requires Unix-domain sockets".to_string(),
        ))
    }
}

/// Publish the workspace facts at the last boundary before a provider thread
/// is created. The prepared roster may be arbitrarily stale: Close and Reopen
/// are durable concurrent operations, so neither can be overwritten by a
/// starter that still holds the old ProviderRuntimeProjection version.
pub(super) fn prepare_member_workspace_for_spawn(
    ledger: &TeamRunLedger,
    prepared: &ProviderRuntimeProjection,
    provider_environment_observation: &MemberWorkspaceSnapshot,
) -> CliResult<PreSpawnWorkspacePreparation> {
    prepare_member_workspace_for_spawn_with_hooks(
        ledger,
        prepared,
        provider_environment_observation,
        |_, _| Ok(()),
        |_| Ok(()),
    )
}

#[cfg(test)]
pub(super) fn prepare_member_workspace_for_spawn_with_recovery_pending_hook(
    ledger: &TeamRunLedger,
    prepared: &ProviderRuntimeProjection,
    provider_environment_observation: &MemberWorkspaceSnapshot,
    on_exact_pending: impl FnMut(&TeamMemberCloseRequest) -> CliResult<()>,
) -> CliResult<PreSpawnWorkspacePreparation> {
    prepare_member_workspace_for_spawn_with_hooks(
        ledger,
        prepared,
        provider_environment_observation,
        |_, _| Ok(()),
        on_exact_pending,
    )
}

pub(super) enum PreSpawnWorkspacePreparation {
    Ready(Box<ProviderRuntimeProjection>),
    Superseded,
    Retry,
}

/// Whether the current Supervisor may adopt an active provider lifecycle left
/// by an older lease. The member transition must predate this lease: an active
/// row written at or after acquisition belongs to the current Supervisor and
/// must never be started a second time.
pub(super) fn successor_may_take_over_active_member(
    ledger: &TeamRunLedger,
    takeover_anchor: &ProviderRuntimeProjection,
    latest: &ProviderRuntimeProjection,
) -> CliResult<bool> {
    if ledger.supervisor_generation <= 1
        || takeover_anchor.status != latest.status
        || !matches!(
            latest.status,
            MemberRunStatus::Starting | MemberRunStatus::Running
        )
    {
        return Ok(false);
    }
    let Some(last_event_ms) = takeover_anchor
        .last_event_at
        .as_deref()
        .and_then(parse_unix_ms)
    else {
        return Ok(false);
    };
    let Some(lease) = ledger.store.latest_team_supervisor_lease(&ledger.run_id)? else {
        return Ok(false);
    };
    Ok(
        lease.status == harness_core::TeamSupervisorLeaseStatus::Active
            && lease.supervisor_id == ledger.supervisor_id
            && lease.generation == ledger.supervisor_generation
            && last_event_ms < u128::from(lease.acquired_unix_ms),
    )
}

#[cfg(test)]
pub(super) fn prepare_member_workspace_for_spawn_with_hook(
    ledger: &TeamRunLedger,
    prepared: &ProviderRuntimeProjection,
    provider_environment_observation: &MemberWorkspaceSnapshot,
    before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<PreSpawnWorkspacePreparation> {
    prepare_member_workspace_for_spawn_with_hooks(
        ledger,
        prepared,
        provider_environment_observation,
        before_cas,
        |_| Ok(()),
    )
}

fn prepare_member_workspace_for_spawn_with_hooks(
    ledger: &TeamRunLedger,
    prepared: &ProviderRuntimeProjection,
    provider_environment_observation: &MemberWorkspaceSnapshot,
    mut before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
    mut on_exact_pending: impl FnMut(&TeamMemberCloseRequest) -> CliResult<()>,
) -> CliResult<PreSpawnWorkspacePreparation> {
    for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
        ledger.require_supervisor_lease()?;
        let Some(mut latest) = ledger.latest_member_run(&prepared.id)? else {
            return Ok(PreSpawnWorkspacePreparation::Superseded);
        };
        if let Some(close) = pending_member_close(&ledger.store, &latest.id)? {
            match stop_member_for_latched_close_with_pending_hook(
                ledger,
                &mut latest,
                &close,
                &mut on_exact_pending,
            ) {
                Ok(()) => return Ok(PreSpawnWorkspacePreparation::Superseded),
                Err(CliError::Store(StoreError::Conflict(_)))
                    if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES =>
                {
                    continue;
                }
                // Exhausted contention is local to this stale roster entry.
                // A later supervisor rescan/generation will reconcile it.
                Err(CliError::Store(StoreError::Conflict(_))) => {
                    return Ok(PreSpawnWorkspacePreparation::Retry)
                }
                Err(error) => return Err(error),
            }
        }
        let successor_takeover = successor_may_take_over_active_member(ledger, prepared, &latest)?;
        if !latest.coordination_is_active()
            || matches!(
                latest.status,
                MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
            )
            || (matches!(
                latest.status,
                MemberRunStatus::Starting | MemberRunStatus::Running
            ) && !successor_takeover)
        {
            return Ok(PreSpawnWorkspacePreparation::Superseded);
        }
        // Display name, refreshed provider observations, timestamps, and a
        // previous workspace observation are benign same-generation drift and
        // stay rebased from latest. Execution identity, provenance, controls,
        // lifecycle, and provider-native history are hard fences.
        if latest.id != prepared.id
            || latest.team_run_id != prepared.team_run_id
            || latest.slot_id != prepared.slot_id
            || latest.agent_member_id != prepared.agent_member_id
            || latest.role != prepared.role
            || latest.provider != prepared.provider
            || latest.model != prepared.model
            || latest.provider_controls != prepared.provider_controls
            || latest.coordination_status != prepared.coordination_status
            || latest.runtime_generation != prepared.runtime_generation
            || latest.status != prepared.status
            || latest.native_session != prepared.native_session
            || latest.provider_cwd_hint != prepared.provider_cwd_hint
            || latest.owned_paths != prepared.owned_paths
            || latest.zero_output_streak != prepared.zero_output_streak
            || latest.last_consumed_work_version != prepared.last_consumed_work_version
            || latest.started_at != prepared.started_at
            || latest.finished_at != prepared.finished_at
        {
            return Ok(PreSpawnWorkspacePreparation::Superseded);
        }
        let expected = latest.clone();
        latest.provider_environment_observation = Some(provider_environment_observation.clone());
        before_cas(attempt, &expected)?;
        match ledger.save_member_run(&expected, &latest) {
            Ok(()) => return Ok(PreSpawnWorkspacePreparation::Ready(Box::new(latest))),
            Err(CliError::Store(StoreError::Conflict(_)))
                if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES => {}
            Err(CliError::Store(StoreError::Conflict(_))) => {
                return Ok(PreSpawnWorkspacePreparation::Retry)
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded pre-spawn ProviderRuntimeProjection CAS loop returns on every path")
}

/// What one Supervisor generation proved about the TeamRun it adopted.
///
/// A Supervisor that returns `Ok(())` has not thereby converged anything: the
/// TeamRun may be exactly as `Running` and exactly as stuck as it was before
/// adoption. Naming that outcome is what lets the NodeDaemon stop re-adopting
/// an unchanged run under a fresh Supervisor generation (#704, #671). A
/// failure stays an `Err` and keeps its existing recovery-marker handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TeamRunDriveOutcome {
    /// The TeamRun left `Running`, or its canonical TeamRun/MemberRun/Work/
    /// Message/RuntimeCommand state changed under this generation. A later
    /// adoption would start from a different canonical state.
    Progressed { team_run_status: TeamRunStatus },
    /// The Supervisor returned with the TeamRun still `Running` and not one
    /// canonical row changed. Re-adopting this exact `canonical_state` can
    /// only repeat this outcome, so the daemon holds adoption until the state
    /// changes or an explicit recovery/start intent arrives.
    NoProgress {
        canonical_state: String,
        detail: String,
    },
}

/// Decide what one Supervisor generation proved, from the TeamRun status it
/// left behind and the canonical state it entered and exited on.
///
/// A generation that ends with the run still `Running` and not one canonical
/// row moved has, by definition, nothing a repeat of the same adoption could
/// improve on. Anything else is progress and the next adoption starts from a
/// different observation.
pub(crate) fn classify_team_run_drive_outcome(
    team_run_status: TeamRunStatus,
    entry_canonical_state: &str,
    exit_canonical_state: &str,
    runtime_outcome_count: usize,
) -> TeamRunDriveOutcome {
    if team_run_status == TeamRunStatus::Running && exit_canonical_state == entry_canonical_state {
        TeamRunDriveOutcome::NoProgress {
            canonical_state: exit_canonical_state.to_string(),
            detail: format!(
                "member supervisor stopped with team run still running and no canonical TeamRun, MemberRun, Work, Message or RuntimeCommand change ({runtime_outcome_count} runtime outcome(s))"
            ),
        }
    } else {
        TeamRunDriveOutcome::Progressed { team_run_status }
    }
}

pub(crate) fn drive_prepared_team_run(
    prepared: PreparedTeamRunStart,
    execution_space: Option<ExecutionSpace>,
    project_context: Option<ProjectContext>,
    max_concurrency: usize,
    idle_timeout: Duration,
    live_sink: Option<NativeSessionWakeSink>,
    serving_status: Option<Arc<Mutex<String>>>,
) -> CliResult<TeamRunDriveOutcome> {
    let PreparedTeamRunStart {
        run_id,
        objective,
        running,
        members,
        ledger,
        supervisor_registration: _supervisor_registration,
    } = prepared;
    let project_context = {
        let binding_id = running.project_binding_id.as_str();
        {
            let pinned = project::firm_home()
                .ok()
                .and_then(|home| project::context_for_id(&home, binding_id).ok().flatten());
            match pinned {
                Some(context) => Some(context),
                None => match project_context {
                    Some(context) if context.id == binding_id => Some(context),
                    _ => {
                        return Err(CliError::Usage(format!(
                            "team run {} is pinned to unavailable Project Binding {}; \
                             restore or register that binding before starting members",
                            running.id, binding_id
                        )))
                    }
                },
            }
        }
    };
    let execution_space_id = execution_space.as_ref().map(|space| space.id.clone());
    // Observe the canonical state this generation inherits before any member
    // runtime touches it. The same observation at exit is what distinguishes
    // "this adoption changed something" from "this adoption produced nothing
    // a further adoption could improve on".
    let entry_canonical_state = team_run_canonical_state_fingerprint(
        &ledger.store,
        execution_space_id.as_deref(),
        &run_id,
    )?;
    let project_id = project_context.as_ref().map(|context| context.id.clone());
    let project_selector = project_context
        .as_ref()
        .map(|context| context.project_root.to_string_lossy().into_owned());
    let mut seen_runtime_generations = HashMap::<String, u64>::new();
    let mut member_retry_not_before = HashMap::<String, Instant>::new();
    let mut member_fabric_attempts = HashMap::<String, u32>::new();
    let mut pending_members = members;
    let mut handles = HashMap::new();
    let mut outcomes = Vec::new();
    let turn_leases = Arc::new(ActiveTurnLeasePool::new(max_concurrency));
    let mut lease_lost = false;
    // Carried across passes so an idle completed-serving tick can keep the
    // projection it already proved instead of decoding the ledgers again
    // (#836).
    let mut current_run_status = None;
    let mut completed_unclosed = 0usize;
    let mut serving_idler = crate::completed_run_members::CompletedRunServingIdler::new(
        crate::completed_run_members::COMPLETED_RUN_SERVING_POLL_INTERVAL,
    );
    // Fire the GitHub CI poll on the first iteration, then every
    // GITHUB_CI_POLL_INTERVAL (issue #369 Phase 2).
    let mut last_github_ci_poll = Instant::now() - GITHUB_CI_POLL_INTERVAL;
    loop {
        if !lease_lost {
            if let Err(error) = ledger.require_supervisor_lease() {
                if error.is_supervisor_lease_lost() {
                    lease_lost = true;
                    pending_members.clear();
                } else {
                    return Err(error);
                }
            }
        }
        if !lease_lost {
            for mut member in std::mem::take(&mut pending_members) {
                if handles.contains_key(&member.id) {
                    continue;
                }
                if member_retry_not_before
                    .get(&member.id)
                    .is_some_and(|deadline| *deadline > Instant::now())
                {
                    pending_members.push(member);
                    continue;
                }
                member_retry_not_before.remove(&member.id);
                seen_runtime_generations.insert(member.id.clone(), member.runtime_generation);
                let member_ledger = Arc::clone(&ledger);
                // A declared external interactive member is driven by the user
                // in their own already-open provider session: no adapter
                // thread, no workspace snapshot, no Failed/Disconnected
                // derivation. Its deliveries stay queued until the session
                // polls its inbox and acks.
                if member.is_external_interactive() {
                    member_ledger.fold_event(
                        TeamRunEventSourceKind::Host,
                        Some(member.id.clone()),
                        "member_run",
                        &member.id,
                        "updated",
                        &format!(
                            "external interactive member {} is user-driven; supervisor does not spawn an adapter",
                            member.name
                        ),
                    )?;
                    continue;
                }
                let member_objective = objective.clone();
                let cwd = member_spawn_cwd(project_context.as_ref(), &running, &member);
                let provider_environment_observation = snapshot_member_workspace(
                    &cwd,
                    project_context.as_ref().map(|context| context.id.as_str()),
                    project_context
                        .as_ref()
                        .map(|context| context.project_root.as_path()),
                    if member
                        .provider_cwd_hint
                        .as_deref()
                        .is_some_and(|value| !value.is_empty())
                    {
                        "member_worktree"
                    } else if running
                        .execution_root
                        .as_deref()
                        .is_some_and(|value| !value.is_empty())
                    {
                        "team_execution_root"
                    } else if project_context.is_some() {
                        "project_binding_root"
                    } else {
                        "explicit_unbound"
                    },
                );
                let published_member = match prepare_member_workspace_for_spawn(
                    &member_ledger,
                    &member,
                    &provider_environment_observation,
                )? {
                    PreSpawnWorkspacePreparation::Ready(member) => *member,
                    PreSpawnWorkspacePreparation::Superseded => {
                        member_retry_not_before.remove(&member.id);
                        continue;
                    }
                    PreSpawnWorkspacePreparation::Retry => {
                        member_retry_not_before.insert(
                            member.id.clone(),
                            Instant::now() + Duration::from_millis(50),
                        );
                        pending_members.push(member);
                        continue;
                    }
                };
                member_retry_not_before.remove(&member.id);
                member = published_member;
                member_ledger.fold_event(
                    TeamRunEventSourceKind::Host,
                    Some(member.id.clone()),
                    "member_run",
                    &member.id,
                    "provider_environment_observation",
                    &format!("member workspace resolved to {}", cwd.display()),
                )?;
                // A member admitted into a live run never passed through the
                // adoption seam that materializes AgentSessions, so provision
                // it here before any provider effect is prepared (#749). An
                // original member already has its session and is untouched.
                match ensure_joined_member_runtime_fabric(&member_ledger, &mut member) {
                    Ok(JoinedMemberRuntimeFabric::AlreadyProvisioned) => {
                        member_fabric_attempts.remove(&member.id);
                    }
                    Ok(JoinedMemberRuntimeFabric::Provisioned { session_id }) => {
                        member_fabric_attempts.remove(&member.id);
                        member_ledger.fold_event(
                            TeamRunEventSourceKind::Host,
                            Some(member.id.clone()),
                            "member_run",
                            &member.id,
                            "updated",
                            &format!(
                                "member {} joined a live run; AgentSession {session_id} bound to supervisor {} generation {}",
                                member.name, ledger.supervisor_id, ledger.supervisor_generation
                            ),
                        )?;
                    }
                    Err(error) => {
                        let failure = classify_member_fabric_failure(&error);
                        if matches!(failure, MemberFabricFailure::LeaseLost) {
                            // Identical to this loop's own lease check above:
                            // quiesce the generation and leave every member
                            // exactly as the successor will find it.
                            lease_lost = true;
                            pending_members.clear();
                            break;
                        }
                        if matches!(failure, MemberFabricFailure::Transient) {
                            let attempts = {
                                let counter = member_fabric_attempts
                                    .entry(member.id.clone())
                                    .or_insert(0u32);
                                *counter += 1;
                                *counter
                            };
                            if attempts < MEMBER_FABRIC_PROVISION_ATTEMPTS {
                                member_retry_not_before.insert(
                                    member.id.clone(),
                                    Instant::now() + Duration::from_millis(50),
                                );
                                pending_members.push(member);
                                continue;
                            }
                        }
                        // A durable refusal about this member, or an
                        // attempt-scoped race that never cleared. Nothing
                        // reached a provider, so this is an ordinary failure
                        // the Host can read — never a recovery claim about an
                        // ambiguous effect. The error is journalled unchanged
                        // so its own leading code token survives for the
                        // adoption classifier.
                        let reason = error.to_string();
                        journal_member_failure(&member_ledger, &member, &reason);
                        outcomes.push(MemberOutcome::new(&member, MemberRunStatus::Failed, reason));
                        continue;
                    }
                }
                let handle_member = member.clone();
                let member_live_sink = live_sink.clone();
                let member_project_id = project_id.clone();
                let member_project_selector = project_selector.clone();
                let member_execution_space_id = execution_space_id.clone();
                let member_turn_leases = Arc::clone(&turn_leases);
                let member_role_action_token = generated_member_role_action_token()?;
                let handle = std::thread::spawn(move || {
                    run_member_orchestration(
                        &member_ledger,
                        &member_objective,
                        handle_member,
                        MemberRuntimeContext {
                            execution_space_id: member_execution_space_id,
                            project_id: member_project_id,
                            project_selector: member_project_selector,
                            cwd,
                            idle_timeout,
                            live_sink: member_live_sink,
                            turn_leases: member_turn_leases,
                            role_action_token: member_role_action_token,
                        },
                    )
                });
                handles.insert(member.id.clone(), (member, handle));
            }
        }

        let finished_member_ids = handles
            .iter()
            .filter(|(_, (_, handle))| handle.is_finished())
            .map(|(member_id, _)| member_id.clone())
            .collect::<Vec<_>>();
        for member_id in finished_member_ids {
            let Some((member, handle)) = handles.remove(&member_id) else {
                continue;
            };
            match handle.join() {
                Ok(outcome) => outcomes.push(outcome),
                Err(_) => {
                    journal_member_failure(&ledger, &member, "orchestration thread panicked");
                    outcomes.push(MemberOutcome::new(
                        &member,
                        MemberRunStatus::Failed,
                        "orchestration thread panicked".to_string(),
                    ));
                }
            }
        }

        // A Completed run with no member handle left is served only to keep
        // the Close lane's provider-loop authority; re-decoding both ledgers
        // every tick starved the daemon's own heartbeat off the machine
        // (#836). Skip the decode while their bytes are provably unchanged.
        let idle_completed_serving = handles.is_empty()
            && current_run_status == Some(TeamRunStatus::Completed)
            && completed_unclosed > 0;
        if lease_lost {
            current_run_status = None;
            completed_unclosed = 0;
            pending_members.clear();
        } else if let crate::completed_run_members::ServingObservation::Rescanned {
            members: latest_members,
            run_status,
        } = serving_idler.observe(&ledger.store, &run_id, idle_completed_serving)?
        {
            current_run_status = Some(run_status);
            completed_unclosed = 0;
            if run_status == TeamRunStatus::Completed {
                completed_unclosed = crate::completed_run_members::unclosed_managed_member_count(
                    &latest_members,
                    &run_id,
                );
                if let Some(status) = &serving_status {
                    *status.lock().unwrap_or_else(|error| error.into_inner()) =
                        crate::completed_run_members::completed_serving_label(completed_unclosed);
                }
            }
            for member in members_joined_since_last_pass(
                latest_members,
                &run_id,
                run_status,
                &seen_runtime_generations,
                |member_id| handles.contains_key(member_id),
            ) {
                if !pending_members.iter().any(|pending| {
                    pending.id == member.id
                        && pending.runtime_generation == member.runtime_generation
                }) {
                    pending_members.push(member);
                }
            }
        }
        if !pending_members.is_empty() {
            ledger.fold_event(
                TeamRunEventSourceKind::Host,
                None,
                "team_run",
                &run_id,
                "updated",
                &format!(
                    "{} member(s) joined while the supervisor was active",
                    pending_members.len()
                ),
            )?;
            if let Some(delay) = pending_members
                .iter()
                .filter_map(|member| member_retry_not_before.get(&member.id))
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .min()
                .filter(|delay| !delay.is_zero())
            {
                std::thread::sleep(delay.min(Duration::from_millis(50)));
            }
            continue;
        }

        // A TeamRun decision never closes a Member. A Completed attempt keeps
        // its Supervisor/control lane until every managed member is explicitly
        // Closed or Retired, even when a member adapter has already returned.
        // The registration Drop below then releases the lease after the last
        // member leaves. Other statuses preserve the existing empty-handle
        // exit behavior.
        if handles.is_empty() {
            if current_run_status == Some(TeamRunStatus::Completed) && completed_unclosed > 0 {
                serving_idler.wait_for_ledger_change(&ledger.store);
                continue;
            }
            break;
        }
        // GitHub linkage CI poll (issue #369 Phase 2): throttled, best-effort,
        // never fatal to the supervisor loop.
        if last_github_ci_poll.elapsed() >= GITHUB_CI_POLL_INTERVAL {
            last_github_ci_poll = Instant::now();
            match poll_team_run_github_linkages(&ledger.store, &run_id) {
                Ok(summary) if !summary.is_noop() => {
                    let mut detail = format!(
                        "github linkage poll: {} link(s) refreshed",
                        summary.links_refreshed
                    );
                    if !summary.blocked_on_failure.is_empty() {
                        detail.push_str(&format!(
                            "; held {} on red CI: {}",
                            summary.blocked_on_failure.len(),
                            summary.blocked_on_failure.join(", ")
                        ));
                    }
                    ledger.fold_event(
                        TeamRunEventSourceKind::Host,
                        None,
                        "team_run",
                        &run_id,
                        "updated",
                        &detail,
                    )?;
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("[supervisor] github linkage poll skipped: {error}");
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if lease_lost {
        return Err(supervisor_lease_lost_error(&run_id));
    }

    let current = latest_team_run(&ledger.store, &run_id)?;
    // The exit observation must be taken before this generation journals its
    // own stop event, and the fingerprint deliberately excludes that journal,
    // so "nothing changed" stays provable rather than self-refuting.
    let exit_canonical_state = team_run_canonical_state_fingerprint(
        &ledger.store,
        execution_space_id.as_deref(),
        &run_id,
    )?;
    let drive_outcome = classify_team_run_drive_outcome(
        current.status,
        &entry_canonical_state,
        &exit_canonical_state,
        outcomes.len(),
    );
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        None,
        "team_run",
        &run_id,
        "updated",
        &format!(
            "member supervisor stopped with team run still {} ({} runtime outcome(s), canonical state {})",
            serde_snake_label(&current.status),
            outcomes.len(),
            match &drive_outcome {
                TeamRunDriveOutcome::NoProgress { .. } => "unchanged",
                TeamRunDriveOutcome::Progressed { .. } => "changed",
            }
        ),
    )?;

    println!("team run {run_id}\t{}", serde_snake_label(&current.status));
    for outcome in &outcomes {
        println!(
            "  {} ({}/{})\t{}",
            outcome.name,
            outcome.role,
            outcome.provider,
            serde_snake_label(&outcome.status)
        );
        for line in outcome.summary.lines().take(3) {
            println!("    {line}");
        }
    }
    Ok(drive_outcome)
}

pub(super) enum MemberProviderStartClaim {
    Claimed(ProviderRuntimeProjection),
    Superseded(ProviderRuntimeProjection),
    Retry,
}

pub(super) fn member_requested_controls_match(
    anchor: &ProviderRuntimeProjection,
    candidate: &ProviderRuntimeProjection,
) -> bool {
    anchor.provider_controls.model.requested == candidate.provider_controls.model.requested
        && anchor.provider_controls.reasoning_effort.requested
            == candidate.provider_controls.reasoning_effort.requested
        && anchor.provider_controls.service_tier.requested
            == candidate.provider_controls.service_tier.requested
}

pub(super) fn member_runtime_anchor_matches(
    anchor: &ProviderRuntimeProjection,
    candidate: &ProviderRuntimeProjection,
) -> bool {
    candidate.id == anchor.id
        && candidate.team_run_id == anchor.team_run_id
        && candidate.slot_id == anchor.slot_id
        && candidate.agent_member_id == anchor.agent_member_id
        && candidate.role == anchor.role
        && candidate.provider == anchor.provider
        && candidate.model == anchor.model
        && member_requested_controls_match(anchor, candidate)
        // Provider identity alone is not a runtime authority boundary. A
        // permission locus, adapter revision, capability set, or execution
        // driver change resolves to a different composition and must lose the
        // start CAS rather than opening a handle from the stale snapshot.
        && candidate.provider_profile == anchor.provider_profile
        && candidate.runtime_generation == anchor.runtime_generation
        && candidate.provider_cwd_hint == anchor.provider_cwd_hint
        && candidate.owned_paths == anchor.owned_paths
        && candidate.started_at == anchor.started_at
}

pub(super) fn member_runtime_progress_matches(
    anchor: &ProviderRuntimeProjection,
    accepted: &ProviderRuntimeProjection,
    candidate: &ProviderRuntimeProjection,
    allow_owned_initial_bind: bool,
) -> bool {
    member_runtime_anchor_matches(anchor, candidate)
        && match (&accepted.native_session, &candidate.native_session) {
            (None, None) => true,
            (None, Some(_)) => allow_owned_initial_bind,
            (Some(_), None) => false,
            (Some(accepted), Some(candidate)) => {
                accepted.provider == candidate.provider
                    && accepted.execution_mode == candidate.execution_mode
                    && accepted.native_session_id == candidate.native_session_id
                    && accepted.native_locator_kind == candidate.native_locator_kind
                    && accepted.adapter_contract_version == candidate.adapter_contract_version
            }
        }
}

/// Linearization point for provider-native side effects. Close writes its
/// durable latch and coordination transition before this CAS; therefore only
/// an exact active queued/idle/disconnected generation may claim Starting.
/// Queued is the durable projection emitted by Reopen/recovery while waiting
/// for a Supervisor rescan. Once this CAS wins, Start precedes a later
/// concurrent Close, whose normal control path is responsible for stopping the
/// newly owned transport.
pub(super) fn claim_member_provider_start(
    ledger: &TeamRunLedger,
    scheduled: &ProviderRuntimeProjection,
) -> CliResult<MemberProviderStartClaim> {
    claim_member_provider_start_with_takeover_anchor_and_hook(
        ledger,
        scheduled,
        scheduled,
        |_, _| Ok(()),
    )
}

#[cfg(test)]
pub(super) fn claim_member_provider_start_with_hook(
    ledger: &TeamRunLedger,
    scheduled: &ProviderRuntimeProjection,
    mut before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<MemberProviderStartClaim> {
    claim_member_provider_start_with_takeover_anchor_and_hook(
        ledger,
        scheduled,
        scheduled,
        &mut before_cas,
    )
}

pub(super) fn claim_member_provider_start_with_takeover_anchor_and_hook(
    ledger: &TeamRunLedger,
    scheduled: &ProviderRuntimeProjection,
    takeover_anchor: &ProviderRuntimeProjection,
    mut before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<MemberProviderStartClaim> {
    for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
        ledger.require_supervisor_lease()?;
        let Some(mut latest) = ledger.latest_member_run(&scheduled.id)? else {
            return Ok(MemberProviderStartClaim::Superseded(scheduled.clone()));
        };
        if let Some(close) = pending_member_close(&ledger.store, &latest.id)? {
            match stop_member_for_latched_close(ledger, &mut latest, &close) {
                Ok(()) => return Ok(MemberProviderStartClaim::Superseded(latest)),
                Err(CliError::Store(StoreError::Conflict(_)))
                    if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES =>
                {
                    continue;
                }
                Err(CliError::Store(StoreError::Conflict(_))) => {
                    return Ok(MemberProviderStartClaim::Retry)
                }
                Err(error) => return Err(error),
            }
        }
        let successor_takeover =
            successor_may_take_over_active_member(ledger, takeover_anchor, &latest)?;
        if !latest.coordination_is_active()
            || latest.runtime_generation != scheduled.runtime_generation
            || latest.native_session != scheduled.native_session
            || latest.id != scheduled.id
            || latest.team_run_id != scheduled.team_run_id
            || latest.slot_id != scheduled.slot_id
            || latest.agent_member_id != scheduled.agent_member_id
            || latest.role != scheduled.role
            || latest.provider != scheduled.provider
            || latest.model != scheduled.model
            || latest.provider_controls != scheduled.provider_controls
            || latest.provider_cwd_hint != scheduled.provider_cwd_hint
            || latest.owned_paths != scheduled.owned_paths
            || (!matches!(
                latest.status,
                MemberRunStatus::Queued | MemberRunStatus::Idle | MemberRunStatus::Disconnected
            ) && !successor_takeover)
        {
            return Ok(MemberProviderStartClaim::Superseded(latest));
        }
        let expected = latest.clone();
        latest.status = MemberRunStatus::Starting;
        latest.last_event_at = Some(now_string());
        before_cas(attempt, &expected)?;
        match ledger.save_member_run(&expected, &latest) {
            Ok(()) => {
                // Close latches before changing coordination. Re-read after
                // the claim CAS to close the latch-between-read-and-CAS
                // window: a latch already durable here precedes provider
                // start and must be fully applied with zero native spawn.
                if let Some(close) = pending_member_close(&ledger.store, &latest.id)? {
                    match stop_member_for_latched_close(ledger, &mut latest, &close) {
                        Ok(()) => return Ok(MemberProviderStartClaim::Superseded(latest)),
                        Err(CliError::Store(StoreError::Conflict(_)))
                            if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES =>
                        {
                            continue;
                        }
                        Err(CliError::Store(StoreError::Conflict(_))) => {
                            return Ok(MemberProviderStartClaim::Retry)
                        }
                        Err(error) => return Err(error),
                    }
                }
                return Ok(MemberProviderStartClaim::Claimed(latest));
            }
            Err(CliError::Store(StoreError::Conflict(_)))
                if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES => {}
            Err(CliError::Store(StoreError::Conflict(_))) => {
                return Ok(MemberProviderStartClaim::Retry)
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded provider start claim returns on every path")
}

pub(super) fn reconcile_member_lifecycle_after_provider_error(
    ledger: &TeamRunLedger,
    member: &mut ProviderRuntimeProjection,
) -> CliResult<bool> {
    if let Some(close) = pending_member_close(&ledger.store, &member.id)? {
        return match stop_member_for_latched_close(ledger, member, &close) {
            Ok(()) => Ok(true),
            // The Close intent is durable but the provider-side postcondition
            // is not. Preserve the latch and let the ordinary error path mark
            // the exact session RecoveryRequired instead of fabricating a
            // Stopped member.
            Err(CliError::RuntimeRecoveryRequired(_)) => Ok(false),
            Err(error) => Err(error),
        };
    }
    // A lifecycle transition without this generation's pending Close is still
    // authoritative. Never journal a provider failure over Closed/Retired.
    Ok(!member.coordination_is_active()
        || matches!(
            member.status,
            MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
        ))
}
