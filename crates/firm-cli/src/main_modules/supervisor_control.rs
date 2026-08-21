use super::*;


/// Bounded number of consecutive transient renewal failures the heartbeat
/// tolerates before treating the durable lease as lost. With the default
/// 15s TTL and a ~1s heartbeat cadence the retry window stays well inside
/// the TTL, so a recovering store never costs the lease.
pub(super) const MAX_TRANSIENT_SUPERVISOR_RENEWAL_FAILURES: usize = 3;

/// Fixed identity + retry policy for one supervisor lease heartbeat thread.
#[derive(Clone, Debug)]
pub(super) struct SupervisorHeartbeatPolicy {
    pub(super) team_run_id: String,
    pub(super) supervisor_id: String,
    pub(super) generation: u64,
    pub(super) ttl_ms: u64,
    pub(super) heartbeat_interval_ms: u64,
    pub(super) max_transient_failures: usize,
}

impl SupervisorHeartbeatPolicy {
    /// Bounded exponential backoff for transient renewal failures. Capped at
    /// a quarter of the TTL (or 3s), so retries can never starve the durable
    /// lease to expiry before the consecutive-failure bound latches.
    pub(super) fn backoff_ms_for(&self, consecutive_failures: usize) -> u64 {
        let max_backoff_ms = (self.ttl_ms / 4).min(3_000);
        let shift = consecutive_failures.saturating_sub(1).min(20) as u32;
        self.heartbeat_interval_ms
            .saturating_mul(1u64 << shift)
            .min(max_backoff_ms)
    }
}

/// A renewal error is terminal when the current generation can never renew
/// again: a parent NodeDaemon fence, a superseded/moved lease, or the durable
/// lease row being gone. Those latch immediately. Every other StoreError (Io,
/// LockTimeout, Json, unexpected Conflict) is treated as transient — the store
/// emits those under lock contention or IO hiccups — and the bounded retry
/// loop converts a genuinely persistent failure into a latch anyway.
pub(super) fn is_terminal_supervisor_renewal_error(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::Conflict(message)
            if message.starts_with("TEAM_SUPERVISOR_PARENT_FENCED:")
                || message.contains("is no longer owned by")
                || message.contains("has no Supervisor lease to renew")
    )
}

/// stderr diagnostic shared by every heartbeat retry/recovery/latch decision;
/// names the run, supervisor generation, the error, and the action taken.
pub(super) fn supervisor_heartbeat_diagnostic(
    team_run_id: &str,
    supervisor_id: &str,
    generation: u64,
    error: &str,
    action: &str,
) -> String {
    format!(
        "team run {team_run_id} supervisor {supervisor_id} generation {generation} \
         heartbeat renewal failed: {error}; action={action}"
    )
}

/// Drive one supervisor lease heartbeat until stopped or lease-loss latched.
///
/// A transient renewal error (lock contention, IO, corrupt read) no longer
/// kills the thread: it is retried with bounded exponential backoff, and
/// lease-loss is latched only after `max_transient_failures` consecutive
/// failures or on a terminal fence (parent fenced / generation superseded /
/// lease row gone), which latches immediately. Every retry, recovery, and
/// latch writes a stderr diagnostic naming run, generation, error, and action.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_supervisor_heartbeat_loop(
    policy: &SupervisorHeartbeatPolicy,
    heartbeat_stop: &AtomicBool,
    heartbeat_valid: &AtomicBool,
    authority_gate: &Mutex<()>,
    mut renew: impl FnMut() -> Result<(), StoreError>,
    failure_marker: impl Fn() -> Option<PathBuf>,
) {
    let mut consecutive_transient_failures = 0usize;
    while !heartbeat_stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(policy.heartbeat_interval_ms));
        if heartbeat_stop.load(Ordering::Acquire) {
            break;
        }
        // Serialize the complete renewal-or-loss decision with live Close
        // admission. If renewal fails, loss is latched before a Close can
        // claim the old generation; if Close won the gate, its Store
        // transaction is the earlier linearization point.
        let _authority_guard = authority_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Test injection seam (reused from the inline loop): while the marker
        // exists, simulate a transient store failure so the loop must survive
        // and keep renewing once the marker is removed.
        let result = match failure_marker() {
            Some(_marker) => Err(StoreError::Io(std::io::Error::other(
                "test-injected transient heartbeat renewal/store failure",
            ))),
            None => renew(),
        };
        match result {
            Ok(()) => {
                if consecutive_transient_failures > 0 {
                    eprintln!(
                        "{}",
                        supervisor_heartbeat_diagnostic(
                            &policy.team_run_id,
                            &policy.supervisor_id,
                            policy.generation,
                            "renewal recovered",
                            &format!(
                                "recovered_after_{consecutive_transient_failures}_consecutive_transient_failures"
                            ),
                        )
                    );
                }
                consecutive_transient_failures = 0;
            }
            Err(error) if is_terminal_supervisor_renewal_error(&error) => {
                let _ = latch_supervisor_lease_lost_and_mark(
                    heartbeat_valid,
                    &policy.team_run_id,
                    &policy.supervisor_id,
                    policy.generation,
                    &error.to_string(),
                    None,
                );
                eprintln!(
                    "{}",
                    supervisor_heartbeat_diagnostic(
                        &policy.team_run_id,
                        &policy.supervisor_id,
                        policy.generation,
                        &error.to_string(),
                        "latched_lease_loss_terminal",
                    )
                );
                break;
            }
            Err(error) => {
                consecutive_transient_failures += 1;
                let backoff_ms = policy.backoff_ms_for(consecutive_transient_failures);
                eprintln!(
                    "{}",
                    supervisor_heartbeat_diagnostic(
                        &policy.team_run_id,
                        &policy.supervisor_id,
                        policy.generation,
                        &error.to_string(),
                        &format!(
                            "retry_backoff_{backoff_ms}ms_attempt_{consecutive_transient_failures}_{}",
                            policy.max_transient_failures
                        ),
                    )
                );
                if consecutive_transient_failures >= policy.max_transient_failures {
                    let _ = latch_supervisor_lease_lost_and_mark(
                        heartbeat_valid,
                        &policy.team_run_id,
                        &policy.supervisor_id,
                        policy.generation,
                        &format!(
                            "renewal failed after {consecutive_transient_failures} consecutive transient failures; last error: {error}"
                        ),
                        None,
                    );
                    eprintln!(
                        "{}",
                        supervisor_heartbeat_diagnostic(
                            &policy.team_run_id,
                            &policy.supervisor_id,
                            policy.generation,
                            &error.to_string(),
                            &format!(
                                "latched_lease_loss_after_{consecutive_transient_failures}_consecutive_transient_failures"
                            ),
                        )
                    );
                    break;
                }
                std::thread::sleep(Duration::from_millis(backoff_ms));
            }
        }
    }
}

pub(super) fn current_unix_ms_u64() -> u64 {
    current_unix_ms().min(u64::MAX as u128) as u64
}

pub(super) fn team_supervisor_lease_ttl_ms() -> u64 {
    std::env::var("FIRM_TEAM_SUPERVISOR_LEASE_MS")
        .or_else(|_| std::env::var("HARNESS_TEAM_SUPERVISOR_LEASE_MS"))
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|ttl| *ttl > 0)
        .unwrap_or(15_000)
}

pub(super) fn supervisor_lease_lost_error(team_run_id: &str) -> CliError {
    CliError::SupervisorLeaseLost(format!(
        "team run {team_run_id} lost its durable Supervisor lease; stale generation quiesced"
    ))
}

pub(super) fn latch_supervisor_lease_lost(
    supervisor_valid: &AtomicBool,
    team_run_id: &str,
    supervisor_id: &str,
    generation: u64,
    reason: &str,
) -> CliError {
    if supervisor_valid.swap(false, Ordering::AcqRel) {
        eprintln!(
            "team run {team_run_id} supervisor {supervisor_id} generation {generation} \
             lease_lost; quiescing stale generation: {reason}"
        );
    }
    supervisor_lease_lost_error(team_run_id)
}

pub(super) fn latch_supervisor_lease_lost_and_mark(
    supervisor_valid: &AtomicBool,
    team_run_id: &str,
    supervisor_id: &str,
    generation: u64,
    reason: &str,
    failure_marker: Option<&Path>,
) -> CliError {
    let error = latch_supervisor_lease_lost(
        supervisor_valid,
        team_run_id,
        supervisor_id,
        generation,
        reason,
    );
    if let Some(marker) = failure_marker {
        if let Err(marker_error) = fs::write(marker, b"heartbeat failure latched") {
            eprintln!(
                "team run {team_run_id} supervisor {supervisor_id} generation {generation} \
                 heartbeat failure marker write failed after lease loss was latched: {marker_error}"
            );
        }
    }
    error
}

pub(super) fn supervisor_test_heartbeat_failure_marker() -> Option<PathBuf> {
    std::env::var_os("FIRM_TEST_SUPERVISOR_HEARTBEAT_FAIL_READY")
        .or_else(|| std::env::var_os("HARNESS_TEST_SUPERVISOR_HEARTBEAT_FAIL_READY"))
        .map(PathBuf::from)
}

impl Drop for LiveMemberControlRegistration {
    fn drop(&mut self) {
        LIVE_MEMBER_CONTROLS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.member_run_id);
    }
}

pub(super) fn register_live_member_control(
    member: &ProviderRuntimeProjection,
    role_action_token: &str,
    capacity: usize,
) -> (
    ControlReceiver<MemberControlCommand>,
    LiveMemberControlRegistration,
) {
    let (sender, receiver) = sync_channel(capacity.max(1));
    let profile = member.provider_profile.as_ref();
    LIVE_MEMBER_CONTROLS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            member.id.clone(),
            LiveMemberControl {
                team_run_id: member.team_run_id.clone(),
                agent_member_id: member.agent_member_id.clone(),
                role_action_token: role_action_token.to_string(),
                execution_mode: profile
                    .map(|profile| profile.execution_mode.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                // Steer requires a real current-cycle injection channel:
                // codex app-server `turn/steer`, or pi RPC `steer` compiled at
                // the cycle control boundary (proven by
                // tests/pi_team_member.rs steer conformance). Everything else
                // keeps failing closed here.
                supports_steer: profile.is_some_and(|profile| {
                    matches!(
                        profile.execution_mode.as_str(),
                        "codex_app_server" | "pi_rpc"
                    )
                }),
                supports_interrupt: profile.is_some_and(|profile| {
                    has_active_verified_provider_capability(profile, "interrupt_current_cycle")
                }),
                supports_close: true,
                sender,
            },
        );
    (
        receiver,
        LiveMemberControlRegistration {
            member_run_id: member.id.clone(),
        },
    )
}

pub(super) fn require_current_supervisor_lease(
    store: &HarnessStore,
    team_run_id: &str,
    supervisor_id: &str,
    generation: u64,
) -> CliResult<TeamSupervisorLease> {
    let now = current_unix_ms_u64();
    let lease = store
        .latest_team_supervisor_lease(team_run_id)?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "team run {team_run_id} has no durable Supervisor lease"
            ))
        })?;
    if lease.status != harness_core::TeamSupervisorLeaseStatus::Active
        || lease.supervisor_id != supervisor_id
        || lease.generation != generation
        || lease.expires_unix_ms <= now
    {
        return Err(CliError::SupervisorLeaseLost(format!(
            "team run {team_run_id} Supervisor lease moved to another owner"
        )));
    }
    Ok(lease)
}

pub(super) fn latch_member_close(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    requested_by: &str,
    reason: &str,
) -> CliResult<TeamMemberCloseRequest> {
    store_conflict_as_usage(store.latch_team_member_close(&TeamMemberCloseRequest {
        id: generated_id("member-close"),
        team_run_id: team_run_id.to_string(),
        member_run_id: member_run_id.to_string(),
        requested_by: requested_by.to_string(),
        reason: reason.to_string(),
        status: TeamMemberCloseStatus::Pending,
        requested_at: now_string(),
        applied_at: None,
    }))
}

pub(super) fn latch_member_close_for_supervisor(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    requested_by: &str,
    reason: &str,
    supervisor_id: &str,
    generation: u64,
) -> CliResult<TeamMemberCloseRequest> {
    let close = pending_close_request(team_run_id, member_run_id, requested_by, reason);
    match store.latch_team_member_close_for_supervisor(
        &close,
        supervisor_id,
        generation,
        current_unix_ms_u64(),
    ) {
        Ok(close) => Ok(close),
        Err(StoreError::Conflict(message))
            if message.starts_with("TEAM_SUPERVISOR_LEASE_LOST:")
                || message.starts_with("TEAM_SUPERVISOR_PARENT_FENCED:") =>
        {
            Err(CliError::SupervisorLeaseLost(message))
        }
        Err(error) => store_conflict_as_usage(Err(error)),
    }
}

pub(super) fn pending_close_request(
    team_run_id: &str,
    member_run_id: &str,
    requested_by: &str,
    reason: &str,
) -> TeamMemberCloseRequest {
    TeamMemberCloseRequest {
        id: generated_id("member-close"),
        team_run_id: team_run_id.to_string(),
        member_run_id: member_run_id.to_string(),
        requested_by: requested_by.to_string(),
        reason: reason.to_string(),
        status: TeamMemberCloseStatus::Pending,
        requested_at: now_string(),
        applied_at: None,
    }
}

pub(super) fn mark_member_coordination_closed(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
) -> CliResult<ProviderRuntimeProjection> {
    mark_member_coordination_closed_with_hook(store, team_run_id, member_run_id, |_, _| Ok(()))
}

pub(super) fn mark_member_coordination_closed_with_hook(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    mut before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<ProviderRuntimeProjection> {
    let mut conflicted_expected = None;
    for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
        let mut member = latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.id == member_run_id && member.team_run_id == team_run_id)
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
        if member.coordination_is_retired() {
            return Err(CliError::Usage(format!(
                "member run {member_run_id} is retired and cannot be closed or reopened"
            )));
        }
        if member.coordination_is_closed() {
            return Ok(member);
        }
        if let Some(expected) = conflicted_expected.take() {
            if !is_same_runtime_close_drift(&expected, &member) {
                return Err(CliError::Usage(format!(
                    "ProviderRuntimeProjection {member_run_id} changed outside the exact runtime generation admitted by Close"
                )));
            }
        }
        let expected = member.clone();
        member.coordination_status = MemberCoordinationStatus::Closed;
        member.last_event_at = Some(now_string());
        before_cas(attempt, &expected)?;
        match store.compare_and_append_member_run(&expected, &member) {
            Ok(()) => return Ok(member),
            Err(StoreError::Conflict(message))
                if message.starts_with("ProviderRuntimeProjection ")
                    && message.ends_with(" changed concurrently; retry the operation")
                    && attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES =>
            {
                conflicted_expected = Some(expected);
            }
            Err(error) => return store_conflict_as_usage(Err(error)),
        }
    }
    unreachable!("bounded coordination-close CAS loop returns on every path")
}

pub(super) fn is_same_runtime_close_drift(
    expected: &ProviderRuntimeProjection,
    latest: &ProviderRuntimeProjection,
) -> bool {
    // The Close latch was already committed under the exact current
    // Supervisor generation. A provider callback may concurrently advance
    // transient status/action timestamps before this projection CAS. Rebase
    // only while stable Member identity, Team, runtime generation, requested
    // controls, workspace authority, and native-session ownership are still
    // identical. A reopen, rebind, successor session, or operator authority
    // change fails closed.
    expected.coordination_is_active()
        && latest.coordination_is_active()
        && member_runtime_anchor_matches(expected, latest)
        && latest.native_session == expected.native_session
}

pub(super) fn require_bound_live_member_authority(
    store: &HarnessStore,
    control: &LiveMemberControl,
    team_run_id: &str,
    member_run_id: &str,
    supervisor_id: &str,
    generation: u64,
    capability_token: &str,
) -> CliResult<harness_core::agentfirm_api::AgentSession> {
    if capability_token.len() != 64 || capability_token != control.role_action_token {
        return Err(CliError::Usage(
            "UNAUTHORIZED_ACTOR: member Role Action capability is invalid for this live runtime"
                .into(),
        ));
    }
    let member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id && member.team_run_id == team_run_id)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "BOUND_MEMBER_RUN_NOT_FOUND: {member_run_id} is not a member of TeamRun {team_run_id}"
            ))
        })?;
    if !member.coordination_is_active() || member.agent_member_id != control.agent_member_id {
        return Err(CliError::Usage(
            "UNAUTHORIZED_ACTOR: live member identity no longer matches durable Team authority"
                .into(),
        ));
    }
    let ledger = TeamRunLedger::new(
        store,
        team_run_id,
        supervisor_id,
        generation,
        Arc::new(AtomicBool::new(true)),
    );
    let session = require_member_provider_session_authority(&ledger, &member, true)?;
    if session.agent_member_id != control.agent_member_id {
        return Err(CliError::Usage(
            "AGENT_SESSION_SCOPE_FENCED: live Role Action identity does not match the current AgentSession"
                .into(),
        ));
    }
    Ok(session)
}

pub(super) fn dispatch_local_live_member_control(
    store: &HarnessStore,
    supervisor_id: &str,
    generation: u64,
    supervisor_valid: &AtomicBool,
    authority_gate: &Mutex<()>,
    request: LiveMemberControlRequest,
) -> CliResult<serde_json::Value> {
    dispatch_local_live_member_control_with_close_admission_hook(
        store,
        supervisor_id,
        generation,
        supervisor_valid,
        authority_gate,
        request,
        || {},
    )
}

pub(super) fn dispatch_local_live_member_control_with_close_admission_hook<F>(
    store: &HarnessStore,
    supervisor_id: &str,
    generation: u64,
    supervisor_valid: &AtomicBool,
    authority_gate: &Mutex<()>,
    request: LiveMemberControlRequest,
    before_close_latch: F,
) -> CliResult<serde_json::Value>
where
    F: FnOnce(),
{
    let team_run_id = request.team_run_id().to_string();
    let member_run_id = request.member_run_id().to_string();
    let is_close = matches!(&request, LiveMemberControlRequest::Close { .. });
    // A Close is linearized against process-local heartbeat loss by this gate.
    // If Close acquires it first, the exact Store generation fence below is
    // its authority point; if heartbeat loss acquires it first, the local
    // latch rejects before any durable or provider side effect.
    let authority_guard = is_close.then(|| {
        authority_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    });
    let mut before_close_latch = Some(before_close_latch);
    // Fence immediately before touching the process-local provider handle.
    if !supervisor_valid.load(Ordering::Acquire) {
        return Err(supervisor_lease_lost_error(&team_run_id));
    }
    require_current_supervisor_lease(store, &team_run_id, supervisor_id, generation)?;
    let run = latest_team_run(store, &team_run_id)?;
    team_run_execution_space_id(store, &run)?;
    let control = LIVE_MEMBER_CONTROLS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&member_run_id)
        .cloned();
    let Some(control) = control else {
        if matches!(request, LiveMemberControlRequest::Close { .. }) {
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: member {member_run_id} has no live provider handle; absence from the process-local registry is not proof that its managed runtime was released"
            )));
        }
        return Err(CliError::Usage(format!(
            "member {member_run_id} has no live provider session in its owning Supervisor"
        )));
    };
    if control.team_run_id != team_run_id {
        return Err(CliError::Usage(format!(
            "member {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    if let LiveMemberControlRequest::ReadInbox {
        capability_token,
        include_all,
        ..
    } = &request
    {
        require_bound_live_member_authority(
            store,
            &control,
            &team_run_id,
            &member_run_id,
            supervisor_id,
            generation,
            capability_token,
        )?;
        return serde_json::to_value(team_run_inbox(
            store,
            &team_run_id,
            &member_run_id,
            *include_all,
        )?)
        .map_err(CliError::Json);
    }
    if let LiveMemberControlRequest::RoleAction {
        capability_token,
        path,
        expected_version,
        idempotency_key,
        body,
        ..
    } = &request
    {
        let session = require_bound_live_member_authority(
            store,
            &control,
            &team_run_id,
            &member_run_id,
            supervisor_id,
            generation,
            capability_token,
        )?;
        let auth = agentfirm_api::AuthenticatedMutation {
            execution_space_id: session.execution_space_id,
            actor: harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: control.agent_member_id,
            },
            authorized_authority_actors: Vec::new(),
            idempotency_key: idempotency_key.clone(),
            expected_version: *expected_version,
            request_fingerprint: None,
        };
        let result = role_actions_api::execute(store, auth, path, &serde_json::to_vec(body)?, None)
            .map_err(|error| match error {
                StoreError::Conflict(encoded) => CliError::Usage(encoded),
                other => CliError::Store(other),
            })?;
        return serde_json::to_value(result).map_err(CliError::Json);
    }
    match request.requirement() {
        LiveMemberControlRequirement::Steer if !control.supports_steer => {
            return Err(CliError::Usage(format!(
                "{} does not support mid-turn steer; send a queued TeamMessageProjection instead",
                control.execution_mode
            )));
        }
        LiveMemberControlRequirement::Interrupt if !control.supports_interrupt => {
            return Err(CliError::Usage(format!(
                "{} does not support live interruption",
                control.execution_mode
            )));
        }
        LiveMemberControlRequirement::Close if !control.supports_close => {
            return Err(CliError::Usage(format!(
                "{} does not support explicit Host close",
                control.execution_mode
            )));
        }
        LiveMemberControlRequirement::RoleAction => {
            unreachable!("Role Action requests return after Supervisor capability validation")
        }
        _ => {}
    }
    if let LiveMemberControlRequest::Close {
        reason,
        requested_by,
        ..
    } = &request
    {
        // Every authority, ownership, and capability rejection is complete.
        // Atomically re-fence the exact durable generation and persist the
        // admitted Close before provider-interaction cancellation or lifecycle
        // writes. A rejected Close therefore has zero durable or provider side
        // effect under every successor/expiry interleaving.
        before_close_latch
            .take()
            .expect("Close admission hook is called once")();
        latch_member_close_for_supervisor(
            store,
            &team_run_id,
            &member_run_id,
            requested_by,
            reason,
            supervisor_id,
            generation,
        )?;
        drop(authority_guard);
        // The pending latch freezes new delivery in the provider loop. Do not
        // project Closed/Stopped before the provider-neutral close_runtime
        // receipt and exact RuntimeCommand have settled.
    }
    let interaction_cancel = match &request {
        LiveMemberControlRequest::Interrupt {
            reason,
            requested_by,
            ..
        }
        | LiveMemberControlRequest::Close {
            reason,
            requested_by,
            ..
        } => Some((requested_by.clone(), reason.clone())),
        _ => None,
    };
    let (reply_tx, reply_rx) = sync_channel(1);
    let command = match request {
        LiveMemberControlRequest::Steer {
            content,
            requested_by,
            ..
        } => MemberControlCommand::Steer {
            content,
            requested_by,
            reply: reply_tx,
        },
        LiveMemberControlRequest::Interrupt {
            reason,
            requested_by,
            ..
        } => MemberControlCommand::Interrupt {
            reason,
            requested_by,
            reply: reply_tx,
        },
        LiveMemberControlRequest::Close {
            reason,
            requested_by,
            ..
        } => MemberControlCommand::Close {
            reason,
            requested_by,
            reply: reply_tx,
        },
        LiveMemberControlRequest::ReadInbox { .. } => {
            unreachable!("bound Inbox reads return after Supervisor capability validation")
        }
        LiveMemberControlRequest::RoleAction { .. } => {
            unreachable!("Role Action requests return before provider control dispatch")
        }
    };
    if control.sender.send(command).is_err() {
        if is_close {
            return Err(CliError::Usage(format!(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: Close for {member_run_id} is durably latched but the owning live adapter channel ended before a provider Close receipt"
            )));
        }
        return Err(CliError::Usage(format!(
            "member {member_run_id} provider session already ended"
        )));
    }
    if let Some((requested_by, reason)) = interaction_cancel {
        // Provider user-input callbacks run synchronously on the adapter
        // thread. Enqueue the authorized control first, then cancel the
        // unanswered Harness Message so that the callback can return to the
        // same thread's control poll. InterruptCurrentCycle (and, for Close,
        // the later CloseMember) still crosses the provider boundary only
        // through its durable RuntimeCommand.
        cancel_unanswered_provider_messages(
            store,
            &team_run_id,
            &member_run_id,
            &requested_by,
            &reason,
        )?;
    }
    // An idle provider loop may be at its longest wake backoff when Close
    // arrives. Fifteen seconds was the exact observed boundary for a real
    // Kimi ACP release, yielding a false recovery response milliseconds before
    // the durable close acknowledgement. Keep the wait bounded while allowing
    // one complete idle poll plus quiesce/release receipts.
    match reply_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(result) => result,
        Err(_) if is_close => Err(CliError::Usage(format!(
            "RUNTIME_COMMAND_RECOVERY_REQUIRED: Close for {member_run_id} is durably latched but provider acknowledgement is uncertain"
        ))),
        Err(_) => Err(CliError::Usage(
            "provider control acknowledgement timed out".to_string(),
        )),
    }
}

pub(super) fn handle_live_member_control_connection(
    mut stream: TcpStream,
    store: &HarnessStore,
    team_run_id: &str,
    supervisor_id: &str,
    generation: u64,
    supervisor_valid: &AtomicBool,
    authority_gate: &Mutex<()>,
) {
    let response = (|| -> CliResult<serde_json::Value> {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(20)))?;
        let mut line = String::new();
        // Read directly from the accepted stream. Cloning this short-lived
        // control socket consumes another file descriptor and can fail with
        // EAGAIN under a fresh full-suite process load before any request is
        // decoded. The reader borrow ends before the response write below, so
        // the duplicate descriptor is unnecessary.
        BufReader::new(&mut stream)
            .take(262_145)
            .read_line(&mut line)?;
        if line.len() > 262_144 {
            return Err(CliError::Usage(
                "Supervisor control request exceeds 256 KiB".to_string(),
            ));
        }
        let request: LiveMemberControlRequest = serde_json::from_str(&line)?;
        if request.team_run_id() != team_run_id {
            return Err(CliError::Usage(format!(
                "Supervisor for team run {team_run_id} cannot control {}",
                request.team_run_id()
            )));
        }
        dispatch_local_live_member_control(
            store,
            supervisor_id,
            generation,
            supervisor_valid,
            authority_gate,
            request,
        )
    })();
    let envelope = match response {
        Ok(result) => LiveMemberControlResponse {
            ok: true,
            result: Some(result),
            error: None,
            store_lock_timeout: None,
        },
        Err(error) => {
            let store_lock_timeout = match &error {
                CliError::Store(StoreError::LockTimeout(path)) => Some(path.clone()),
                _ => None,
            };
            LiveMemberControlResponse {
                ok: false,
                result: None,
                error: Some(error.to_string()),
                store_lock_timeout,
            }
        }
    };
    if let Ok(mut payload) = serde_json::to_vec(&envelope) {
        payload.push(b'\n');
        let _ = stream.write_all(&payload);
        let _ = stream.flush();
    }
}

pub(super) fn dispatch_live_member_control(
    store: &HarnessStore,
    request: LiveMemberControlRequest,
) -> CliResult<serde_json::Value> {
    let team_run_id = request.team_run_id();
    let run = latest_team_run(store, team_run_id)?;
    team_run_execution_space_id(store, &run)?;
    let lease = store
        .latest_team_supervisor_lease(team_run_id)?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "team run {team_run_id} has no live Supervisor for provider control"
            ))
        })?;
    require_current_supervisor_lease(store, team_run_id, &lease.supervisor_id, lease.generation)?;
    let address = lease
        .owner_locator
        .strip_prefix("tcp://")
        .ok_or_else(|| {
            CliError::Usage(format!(
                "team run {team_run_id} Supervisor locator is not routable: {}",
                lease.owner_locator
            ))
        })?
        .parse::<std::net::SocketAddr>()
        .map_err(|error| CliError::Usage(format!("invalid Supervisor locator: {error}")))?;
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|error| {
            CliError::Usage(format!(
                "cannot reach team run {team_run_id} Supervisor at {}: {error}",
                lease.owner_locator
            ))
        })?;
    // The owning Supervisor allows one complete idle poll plus provider
    // quiesce/release (30s). The routing client must not time out first and
    // turn a later truthful acknowledgement into a false recovery response.
    stream.set_read_timeout(Some(Duration::from_secs(40)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut payload = serde_json::to_vec(&request)?;
    payload.push(b'\n');
    stream.write_all(&payload)?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).take(262_145).read_line(&mut line)?;
    if line.len() > 262_144 {
        return Err(CliError::Usage(
            "Supervisor control response exceeds 256 KiB".to_string(),
        ));
    }
    let response: LiveMemberControlResponse = serde_json::from_str(&line)?;
    if response.ok {
        response.result.ok_or_else(|| {
            CliError::Usage("Supervisor returned an empty control acknowledgement".to_string())
        })
    } else {
        if let Some(path) = response.store_lock_timeout {
            return Err(CliError::Store(StoreError::LockTimeout(path)));
        }
        Err(CliError::Usage(response.error.unwrap_or_else(|| {
            "Supervisor rejected provider control".to_string()
        })))
    }
}

pub(super) fn managed_member_runtime_close_is_settled(
    store: &HarnessStore,
    member: &ProviderRuntimeProjection,
) -> CliResult<bool> {
    use harness_core::agentfirm_api::{
        AgentSessionStatus, RuntimeActivity, RuntimeCommandKind, RuntimeCommandStatus,
        RuntimeDriverRef, RuntimeEffectCertainty, RuntimePostconditionStatus, RuntimeResidency,
    };
    if member.is_external_interactive() {
        return Ok(true);
    }
    let required_interrupt = member.status == MemberRunStatus::Running;
    let latest_member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|candidate| candidate.id == member.id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {}", member.id)))?;
    if member.native_session.is_none()
        && matches!(
            member.status,
            MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
        )
    {
        return Ok(true);
    }
    if !member_runtime_progress_matches(member, member, &latest_member, false)
        || !latest_member.coordination_is_closed()
        || latest_member.status != MemberRunStatus::Stopped
    {
        return Ok(false);
    }
    let Some(close_request) = store
        .latest_team_member_close_request(&member.id)?
        .filter(|request| request.status == TeamMemberCloseStatus::Applied)
    else {
        return Ok(false);
    };
    let run = latest_team_run(store, &member.team_run_id)?;
    let execution_space_id = team_run_execution_space_id(store, &run)?;
    let sessions = store
        .fabric_agent_sessions(&execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == member.agent_member_id
                && session.provider_kind == member.provider
        })
        .collect::<Vec<_>>();
    let [session] = sessions.as_slice() else {
        return Ok(false);
    };
    // Team Close is reversible and never stops the machine-owned
    // AgentSession. Its exact postcondition is a detached idle Session plus a
    // settled CloseMember command; StopSession belongs only to Node/operator
    // authority and would destroy Reopen semantics.
    let native_session_matches = match (
        latest_member.native_session.as_ref(),
        session.native_session_ref.as_ref(),
    ) {
        (Some(member_native), Some(session_native)) => {
            member_native.provider == session_native.provider
                && member_native.execution_mode == session_native.execution_mode
                && member_native.native_session_id == session_native.native_session_id
                && member_native.native_locator_kind == session_native.native_locator_kind
                && member_native.provider_version == session_native.provider_version
                && member_native.adapter_contract_version == session_native.adapter_contract_version
                && member_native.supports_resume == session_native.supports_resume
                && member_native.parent_native_session_id == session_native.parent_native_session_id
        }
        (None, None) => true,
        _ => false,
    };
    if session.lifecycle != AgentSessionStatus::Idle
        || session.control_state.runtime_residency != RuntimeResidency::Detached
        || session.control_state.activity != RuntimeActivity::Idle
        || !native_session_matches
    {
        return Ok(false);
    }
    let Some(supervisor) = store.latest_team_supervisor_lease(&member.team_run_id)? else {
        return Ok(false);
    };
    let exact_supervisor_driver = matches!(
        &session.control_state.driver_ref,
        RuntimeDriverRef::TeamSupervisor {
            team_run_id,
            team_supervisor_id,
            team_supervisor_generation,
        } if team_run_id == &member.team_run_id
            && team_supervisor_id == &supervisor.supervisor_id
            && *team_supervisor_generation == supervisor.generation
            && supervisor.node_id == session.node_id
            && supervisor.node_daemon_id == session.node_daemon_id
            && supervisor.node_daemon_generation == session.node_daemon_generation
            && supervisor.execution_space_id == session.execution_space_id
    );
    if !exact_supervisor_driver {
        return Ok(false);
    }
    let expected_binding = runtime_command_binding_for_session(session);
    let close_sources = [
        format!("{}:idle:close-runtime", close_request.id),
        format!("{}:active:close-runtime", close_request.id),
    ];
    let interrupt_source = format!("{}:active:interrupt", close_request.id);
    let commands = store.runtime_commands(&execution_space_id)?;
    let close_applied = commands.iter().any(|command| {
        command.command == RuntimeCommandKind::CloseMember
            && command.binding == expected_binding
            && command.status == RuntimeCommandStatus::Applied
            && command.effect_certainty == RuntimeEffectCertainty::Applied
            && command.postcondition_status == RuntimePostconditionStatus::Satisfied
            && command.postcondition
                == runtime_command_postcondition_for(RuntimeCommandKind::CloseMember)
            && command
                .source_record_id
                .as_ref()
                .is_some_and(|source| close_sources.contains(source))
    });
    let interrupt_applied = commands.iter().any(|command| {
        command.command == RuntimeCommandKind::InterruptCurrentCycle
            && command.binding == expected_binding
            && command.status == RuntimeCommandStatus::Applied
            && command.effect_certainty == RuntimeEffectCertainty::Applied
            && command.postcondition_status == RuntimePostconditionStatus::Satisfied
            && command.postcondition
                == runtime_command_postcondition_for(RuntimeCommandKind::InterruptCurrentCycle)
            && command.source_record_id.as_deref() == Some(interrupt_source.as_str())
    });
    Ok(close_applied && (!required_interrupt || interrupt_applied))
}

/// Reconcile a Reopen activation only from the exact durable postcondition.
/// A NodeDaemon dispatch can race its own periodic adoption scan: the request
/// socket may report a transient connect failure after the daemon has already
/// attached the higher MemberRun generation. Returning HTTP 409 in that state
/// is a false negative that invites an unsafe retry.
pub(super) fn managed_member_runtime_reopen_is_settled(
    store: &HarnessStore,
    reopened: &ProviderRuntimeProjection,
) -> CliResult<Option<ProviderRuntimeProjection>> {
    use harness_core::agentfirm_api::{
        AgentSessionStatus, RuntimeActivity, RuntimeDriverRef, RuntimeResidency,
    };
    let latest_member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|candidate| candidate.id == reopened.id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {}", reopened.id)))?;
    if latest_member.runtime_generation != reopened.runtime_generation
        || !latest_member.coordination_is_active()
        || !matches!(
            latest_member.status,
            MemberRunStatus::Idle | MemberRunStatus::Running
        )
        || !provider_callback_native_session_matches(
            &reopened.native_session,
            &latest_member.native_session,
        )
    {
        return Ok(None);
    }
    if latest_member.is_external_interactive() {
        return Ok(Some(latest_member));
    }
    let Some(native_session) = latest_member.native_session.as_ref() else {
        return Ok(None);
    };
    let run = latest_team_run(store, &latest_member.team_run_id)?;
    let execution_space_id = team_run_execution_space_id(store, &run)?;
    let Some(supervisor) = store
        .latest_team_supervisor_lease(&latest_member.team_run_id)?
        .filter(is_supervisor_current)
    else {
        return Ok(None);
    };
    let sessions = store
        .fabric_agent_sessions(&execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == latest_member.agent_member_id
                && session.provider_kind == latest_member.provider
                && session
                    .native_session_ref
                    .as_ref()
                    .is_some_and(|candidate| {
                        candidate.native_session_id == native_session.native_session_id
                            && candidate.provider == native_session.provider
                            && candidate.execution_mode == native_session.execution_mode
                    })
                && matches!(
                    session.lifecycle,
                    AgentSessionStatus::Idle | AgentSessionStatus::Active
                )
                && session.control_state.runtime_residency == RuntimeResidency::Attached
                && matches!(
                    session.control_state.activity,
                    RuntimeActivity::Idle | RuntimeActivity::Running
                )
                && matches!(
                    &session.control_state.driver_ref,
                    RuntimeDriverRef::TeamSupervisor {
                        team_run_id,
                        team_supervisor_id,
                        team_supervisor_generation,
                    } if team_run_id == &latest_member.team_run_id
                        && team_supervisor_id == &supervisor.supervisor_id
                        && *team_supervisor_generation == supervisor.generation
                        && supervisor.node_id == session.node_id
                        && supervisor.node_daemon_id == session.node_daemon_id
                        && supervisor.node_daemon_generation == session.node_daemon_generation
                        && supervisor.execution_space_id == session.execution_space_id
                )
        })
        .collect::<Vec<_>>();
    if sessions.len() != 1 {
        return Ok(None);
    }
    Ok(Some(latest_member))
}

pub(super) fn await_managed_member_runtime_reopen_settled(
    store: &HarnessStore,
    reopened: &ProviderRuntimeProjection,
) -> CliResult<Option<ProviderRuntimeProjection>> {
    for _ in 0..400 {
        if let Some(member) = managed_member_runtime_reopen_is_settled(store, reopened)? {
            return Ok(Some(member));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(None)
}

/// A live-control socket can time out just before the Supervisor persists the
/// exact provider Close receipt and detached Session state. Reconcile only
/// from that complete durable postcondition; never turn a local timeout into
/// permission to replay Close against an uncertain provider handle.
pub(super) fn await_managed_member_runtime_close_settled(
    store: &HarnessStore,
    member_before: &ProviderRuntimeProjection,
) -> CliResult<bool> {
    for _ in 0..400 {
        if managed_member_runtime_close_is_settled(store, member_before)? {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(false)
}
