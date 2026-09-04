use super::*;

// `harness team-run start` — durable Agent Team Supervisor.
// The starter currently attaches the durable Supervisor to its process, while
// the store lease is the cross-process authority and crash-recovery boundary.
// `harness serve` remains a read/broadcast plus control gateway. Registered
// Agent Team modes are only Codex app-server, Kimi ACP, and Claude Agent SDK;
// Retired bounded exec/CLI modes never authorize Agent Team execution.
//
// Persistence and execution concurrency are intentionally separate. Every
// unclosed Member owns one lightweight supervisor thread, while
// --max-concurrency limits only provider turns that currently hold an
// ActiveTurnLease. Idle members remain addressable without consuming permits.
// All seq-assigning ledger writes serialize through one mutex —
// `next_team_run_seq` is a read-max-then-append pair that would race across
// member threads otherwise.
// ---------------------------------------------------------------------------

/// Default cap on concurrently-running member ACP sessions.
pub(super) const TEAM_RUN_START_DEFAULT_CONCURRENCY: usize = 4;

pub(super) struct ActiveTurnLeasePool {
    pub(super) active: Mutex<usize>,
    pub(super) available: Condvar,
    pub(super) limit: usize,
}

impl ActiveTurnLeasePool {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            active: Mutex::new(0),
            available: Condvar::new(),
            limit,
        }
    }

    pub(super) fn acquire(self: &Arc<Self>) -> ActiveTurnLease {
        let mut active = self.active.lock().expect("active turn lease poisoned");
        while *active >= self.limit {
            active = self
                .available
                .wait(active)
                .expect("active turn lease poisoned");
        }
        *active += 1;
        ActiveTurnLease {
            pool: Arc::clone(self),
        }
    }
}

pub(super) struct ActiveTurnLease {
    pub(super) pool: Arc<ActiveTurnLeasePool>,
}

impl Drop for ActiveTurnLease {
    fn drop(&mut self) {
        let mut active = self.pool.active.lock().expect("active turn lease poisoned");
        *active = active.saturating_sub(1);
        self.pool.available.notify_one();
    }
}

/// Test-only escape hatch for foreground integration tests. Production
/// supervisors have no implicit idle retirement.
pub(super) fn member_supervisor_test_idle_grace() -> Option<Duration> {
    std::env::var("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS")
        .or_else(|_| std::env::var("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS"))
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
}

pub(super) static NATIVE_SESSION_WAKE_TOKEN: OnceLock<String> = OnceLock::new();

/// Process-local control plane for provider sessions started by `serve` or the
/// MCP server. The durable TeamMessageProjection remains the conversation record; this
/// registry is only the live transport into the currently running provider
/// turn and is deliberately not reconstructed after process restart.
pub(super) static LIVE_MEMBER_CONTROLS: OnceLock<Mutex<HashMap<String, LiveMemberControl>>> =
    OnceLock::new();
pub(super) static LIVE_TEAM_SUPERVISORS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Clone)]
pub(super) struct LiveMemberControl {
    pub(super) team_run_id: String,
    pub(super) agent_member_id: String,
    pub(super) capability_fingerprint: String,
    pub(super) collaboration_binding: harness_runtime_contract::CollaborationCapabilityBinding,
    pub(super) execution_mode: String,
    pub(super) supports_steer: bool,
    pub(super) supports_interrupt: bool,
    pub(super) supports_close: bool,
    pub(super) sender: SyncSender<MemberControlCommand>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub(super) enum LiveMemberControlRequest {
    Steer {
        team_run_id: String,
        member_run_id: String,
        content: String,
        requested_by: String,
    },
    Interrupt {
        team_run_id: String,
        member_run_id: String,
        reason: String,
        requested_by: String,
    },
    Close {
        team_run_id: String,
        member_run_id: String,
        reason: String,
        requested_by: String,
    },
    ReadInbox {
        team_run_id: String,
        member_run_id: String,
        capability_token: String,
        include_all: bool,
    },
    RoleAction {
        team_run_id: String,
        member_run_id: String,
        capability_token: String,
        path: String,
        expected_version: u64,
        idempotency_key: String,
        body: serde_json::Value,
        confirmed_action: Option<String>,
    },
}

impl LiveMemberControlRequest {
    pub(super) fn team_run_id(&self) -> &str {
        match self {
            Self::Steer { team_run_id, .. }
            | Self::Interrupt { team_run_id, .. }
            | Self::Close { team_run_id, .. }
            | Self::ReadInbox { team_run_id, .. }
            | Self::RoleAction { team_run_id, .. } => team_run_id,
        }
    }

    pub(super) fn member_run_id(&self) -> &str {
        match self {
            Self::Steer { member_run_id, .. }
            | Self::Interrupt { member_run_id, .. }
            | Self::Close { member_run_id, .. }
            | Self::ReadInbox { member_run_id, .. }
            | Self::RoleAction { member_run_id, .. } => member_run_id,
        }
    }

    pub(super) fn requirement(&self) -> LiveMemberControlRequirement {
        match self {
            Self::Steer { .. } => LiveMemberControlRequirement::Steer,
            Self::Interrupt { .. } => LiveMemberControlRequirement::Interrupt,
            Self::Close { .. } => LiveMemberControlRequirement::Close,
            Self::ReadInbox { .. } => LiveMemberControlRequirement::RoleAction,
            Self::RoleAction { .. } => LiveMemberControlRequirement::RoleAction,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct LiveMemberControlResponse {
    pub(super) ok: bool,
    #[serde(default)]
    pub(super) result: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) error: Option<String>,
    #[serde(default)]
    pub(super) store_lock_timeout: Option<String>,
}

pub(super) enum MemberControlCommand {
    Steer {
        content: String,
        requested_by: String,
        reply: SyncSender<CliResult<serde_json::Value>>,
    },
    Interrupt {
        reason: String,
        requested_by: String,
        reply: SyncSender<CliResult<serde_json::Value>>,
    },
    Close {
        reason: String,
        requested_by: String,
        reply: SyncSender<CliResult<serde_json::Value>>,
    },
}

pub(super) enum IdleMemberWake {
    Work(Box<ClaimedWork>),
    ActiveWorkContinuation(Box<Work>),
    Messages {
        messages: Vec<TeamMessageProjection>,
        host_attentions: Vec<HostAttention>,
    },
    HostAttentions(Vec<HostAttention>),
    /// A durable Close latch has been observed while the provider adapter is
    /// idle. The wait loop deliberately does not mutate the MemberRun or drop
    /// the process: the owning adapter must first obtain the narrow reversible
    /// CloseRuntime receipt, then apply the latch. Strong Quiesce/Release stay
    /// reserved for runtime-composition or driver replacement.
    CloseRequested {
        close: TeamMemberCloseRequest,
        reply: Option<SyncSender<CliResult<serde_json::Value>>>,
    },
    TestRetired,
    Degraded(String),
}

pub(super) const CANONICAL_MESSAGE_DELIVERY_REF: &str = "canonical-message-delivery:";
pub(super) const CANONICAL_EXECUTION_SPACE_REF: &str = "canonical-execution-space:";

pub(super) fn canonical_delivery_context(
    execution_space_id: &str,
    supervisor_id: &str,
    command_name: &str,
    idempotency_key: String,
    expected_version: u64,
) -> harness_core::agentfirm_api::MutationContext {
    harness_core::agentfirm_api::MutationContext {
        execution_space_id: execution_space_id.to_string(),
        authenticated_actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: supervisor_id.to_string(),
        },
        authority_actor: None,
        command_name: command_name.to_string(),
        idempotency_key,
        expected_version,
        request_fingerprint: None,
    }
}

pub(super) fn transition_provider_session_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    desired: harness_core::agentfirm_api::AgentSessionStatus,
) -> CliResult<()> {
    use harness_core::agentfirm_api::AgentSessionStatus;
    let (space_id, mut session) = provider_session_for_member(ledger, member)?;
    let daemon = ledger
        .store
        .latest_node_daemon_lease(&session.node_id)?
        .filter(|lease| {
            lease.daemon_id == session.node_daemon_id
                && lease.generation == session.node_daemon_generation
                && lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED".into()))?;
    let transitions: Vec<AgentSessionStatus> = match (session.lifecycle, desired) {
        (current, target) if current == target => Vec::new(),
        (AgentSessionStatus::Cold, AgentSessionStatus::Active) => {
            vec![AgentSessionStatus::Idle, AgentSessionStatus::Active]
        }
        // A Session left `Interrupted` by a NodeDaemon drain re-enters the lane
        // through `Idle`; the Store admits that hop only while the killed
        // runtime is still provably gone, so this never resumes a live cycle.
        (AgentSessionStatus::Interrupted, AgentSessionStatus::Active) => {
            vec![AgentSessionStatus::Idle, AgentSessionStatus::Active]
        }
        (AgentSessionStatus::Waiting, AgentSessionStatus::Active) => {
            vec![AgentSessionStatus::Active]
        }
        (AgentSessionStatus::Active, AgentSessionStatus::Closed) => {
            vec![AgentSessionStatus::Interrupted, AgentSessionStatus::Closed]
        }
        (AgentSessionStatus::Cold, AgentSessionStatus::Closed)
        | (AgentSessionStatus::Idle, AgentSessionStatus::Closed)
        | (AgentSessionStatus::Waiting, AgentSessionStatus::Closed)
        | (AgentSessionStatus::Interrupted, AgentSessionStatus::Closed) => vec![desired],
        (AgentSessionStatus::Active, AgentSessionStatus::Idle)
        | (AgentSessionStatus::Cold, AgentSessionStatus::Idle)
        | (AgentSessionStatus::Waiting, AgentSessionStatus::Idle)
        | (AgentSessionStatus::Interrupted, AgentSessionStatus::Idle)
        | (AgentSessionStatus::Idle, AgentSessionStatus::Active) => vec![desired],
        _ => {
            return Err(CliError::Usage(format!(
                "AGENT_SESSION_RECOVERY_REQUIRED: cannot project {:?}->{desired:?}",
                session.lifecycle
            )))
        }
    };
    for next in transitions {
        // Closing a session is authorized by the exact durable StopSession
        // command, not merely by possession of the NodeDaemon lease. Carry
        // that command's key into the Store transition so the under-lock
        // generation/command fence remains authoritative. The command has
        // already crossed the provider boundary and settled Applied before
        // this projection step.
        let transition_idempotency_key = if next == AgentSessionStatus::Closed {
            let mut matching_stop_keys = ledger
                .store
                .runtime_commands(&space_id)?
                .into_iter()
                .filter(|command| {
                    command.command == harness_core::agentfirm_api::RuntimeCommandKind::StopSession
                        && command.target_session_id.as_deref() == Some(session.id.as_str())
                        && command.target_session_generation == Some(session.runtime_generation)
                        && command.target_node_daemon_id == session.node_daemon_id
                        && command.target_node_daemon_generation == session.node_daemon_generation
                        && command.status
                            == harness_core::agentfirm_api::RuntimeCommandStatus::Applied
                        && command.effect_certainty
                            == harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
                })
                .map(|command| command.idempotency_key)
                .collect::<Vec<_>>();
            matching_stop_keys.sort();
            matching_stop_keys.dedup();
            if matching_stop_keys.len() != 1 {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "closing session {} requires one exact applied StopSession command, found {}",
                    session.id,
                    matching_stop_keys.len()
                )));
            }
            format!("{}:effect", matching_stop_keys.pop().unwrap())
        } else {
            format!("session:{}:{}:{next:?}", session.id, session.version)
        };
        let result = ledger.store.transition_agent_session(
            &canonical_delivery_context(
                &space_id,
                &daemon.daemon_id,
                "node_daemon.agent_session.provider_state",
                transition_idempotency_key,
                session.version,
            ),
            &session.id,
            next,
            &now_string(),
        )?;
        session = result.projection;
    }
    Ok(())
}

/// Resolve the only provider session authorized for this exact MemberRun.
/// Agent identities are organization-wide and may legitimately have sessions
/// in other Execution Spaces; they are never sufficient routing authority.
pub(super) fn provider_session_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
) -> CliResult<(String, harness_core::agentfirm_api::AgentSession)> {
    let (execution_space_id, _, session) = provider_runtime_subject_for_member(ledger, member)?;
    Ok((execution_space_id, session))
}

pub(super) fn provider_runtime_subject_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
) -> CliResult<(
    String,
    harness_core::agentfirm_api::MemberRun,
    harness_core::agentfirm_api::AgentSession,
)> {
    use harness_core::agentfirm_api::AgentSessionStatus;

    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    if member.team_run_id != run.id || !run.member_run_ids.contains(&member.id) {
        return Err(CliError::Usage(format!(
            "MEMBER_RUN_SCOPE_MISMATCH: member {} does not belong to TeamRun {}",
            member.id, run.id
        )));
    }
    let execution_space_id = team_run_execution_space_id(&ledger.store, &run)?;
    let member_scope = ledger
        .store
        .trust_member_run_scope(&member.id)?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "MEMBER_RUN_SCOPE_MISMATCH: member {} has no canonical Execution Space",
                member.id
            ))
        })?;
    if member_scope != execution_space_id {
        return Err(CliError::Usage(format!(
            "MEMBER_RUN_SCOPE_MISMATCH: member {} belongs to Execution Space {}, not TeamRun space {}",
            member.id, member_scope, execution_space_id
        )));
    }
    let canonical_members = ledger
        .store
        .trust_member_runs(&execution_space_id)?
        .into_iter()
        .filter(|candidate| candidate.id == member.id)
        .collect::<Vec<_>>();
    let canonical_member = match canonical_members.as_slice() {
        [canonical_member] => canonical_member,
        rows => {
            return Err(CliError::Usage(format!(
                "MEMBER_RUN_SCOPE_MISMATCH: member {} has {} canonical projections in Execution Space {}",
                member.id,
                rows.len(),
                execution_space_id
            )))
        }
    };
    if canonical_member.team_run_id != run.id
        || canonical_member.agent_member_id != member.agent_member_id
        || canonical_member.runtime_generation != member.runtime_generation
    {
        return Err(CliError::Usage(format!(
            "MEMBER_RUN_SCOPE_MISMATCH: canonical member {} does not match TeamRun, identity, or generation",
            member.id
        )));
    }
    let sessions = ledger
        .store
        .fabric_agent_sessions(&execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == member.agent_member_id
                && session.lifecycle != AgentSessionStatus::Closed
        })
        .collect::<Vec<_>>();
    let session = match sessions.as_slice() {
        [session] => (*session).clone(),
        rows => {
            return Err(CliError::Usage(format!(
                "AGENT_SESSION_AMBIGUOUS: member {} requires one current session in Execution Space {}, found {}",
                member.id,
                execution_space_id,
                rows.len()
            )))
        }
    };
    // MemberRun.runtime_generation fences the Team-owned adapter process.
    // AgentSession.runtime_generation fences the machine-owned provider
    // session. They are deliberately independent: Team Close/Reopen replaces
    // the adapter generation while retaining the same AgentSession, native
    // transcript, and WorkExecutionBindings. Conflating the counters strands
    // every legitimate same-session Reopen as soon as the MemberRun advances.
    if session.execution_space_id != execution_space_id || session.provider_kind != member.provider
    {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_SCOPE_FENCED: member {} expects provider {} in Execution Space {}, but scoped session {} is provider {} in {}",
            member.id,
            member.provider,
            execution_space_id,
            session.id,
            session.provider_kind,
            session.execution_space_id
        )));
    }
    Ok((execution_space_id, canonical_member.clone(), session))
}

/// Persist the bounded live-runtime projection used by driver/composition
/// handoff admission. This is not a provider event ledger: it records only
/// whether the exact current handle is attached and whether its execution
/// lane is running. Callers must settle the provider RuntimeCommand first;
/// the Store refuses this update while an effect remains ambiguous.
pub(crate) fn transition_provider_session_runtime_control(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    residency: harness_core::agentfirm_api::RuntimeResidency,
    activity: harness_core::agentfirm_api::RuntimeActivity,
) -> CliResult<()> {
    use harness_core::agentfirm_api::{ActorKind, ActorRef, AgentSessionStatus, MutationContext};

    let mut matches = Vec::new();
    for execution_space_id in ledger.store.canonical_execution_space_ids()? {
        for session in ledger
            .store
            .fabric_agent_sessions(&execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_member_id == member.agent_member_id
                    && session.lifecycle != AgentSessionStatus::Closed
            })
        {
            matches.push((execution_space_id.clone(), session));
        }
    }
    if matches.len() != 1 {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_AMBIGUOUS: runtime observation for {} requires one current session, found {}",
            member.agent_member_id,
            matches.len()
        )));
    }
    let (execution_space_id, session) = matches.pop().expect("one current session");
    if session.control_state.runtime_residency == residency
        && session.control_state.activity == activity
    {
        return Ok(());
    }
    let mut next = session.control_state.clone();
    next.runtime_residency = residency;
    next.activity = activity;
    next.last_reconciled_at = Some(now_string());
    let daemon_actor = ActorRef {
        kind: ActorKind::Service,
        id: session.node_daemon_id.clone(),
    };
    let result = ledger.store.bind_agent_session_control_state(
        &MutationContext {
            execution_space_id: execution_space_id.clone(),
            authenticated_actor: daemon_actor,
            authority_actor: None,
            command_name: "node_daemon.runtime_control.observe".into(),
            idempotency_key: format!(
                "runtime-control-observation:{}:{}:{}:{residency:?}:{activity:?}",
                session.id, session.runtime_generation, session.version
            ),
            expected_version: session.version,
            request_fingerprint: None,
        },
        &session.id,
        session.runtime_generation,
        next,
        &now_string(),
    );
    if let Err(error) = result {
        if let Some(trust_error) = error.trust_error() {
            if trust_error.code == harness_core::agentfirm_api::TrustErrorCode::RuntimeEffectUnknown
            {
                return Err(CliError::RuntimeRecoveryRequired(format!(
                    "{}:{}",
                    trust_error.resource_kind, trust_error.resource_id
                )));
            }
        }
        return Err(CliError::Store(error));
    }
    Ok(())
}

pub(super) fn require_provider_session_authority(
    ledger: &TeamRunLedger,
    agent_member_id: &str,
    require_active: bool,
) -> CliResult<harness_core::agentfirm_api::AgentSession> {
    use harness_core::agentfirm_api::AgentSessionStatus;
    let members = latest_member_runs_in_append_order(&ledger.store)?
        .into_iter()
        .filter(|member| {
            member.team_run_id == ledger.run_id && member.agent_member_id == agent_member_id
        })
        .collect::<Vec<_>>();
    let member = match members.as_slice() {
        [member] => member,
        rows => {
            return Err(CliError::Usage(format!(
                "MEMBER_RUN_SCOPE_MISMATCH: TeamRun {} has {} members for AgentIdentity {}",
                ledger.run_id,
                rows.len(),
                agent_member_id
            )))
        }
    };
    let (_, session) = provider_session_for_member(ledger, member)?;
    if require_active && session.lifecycle != AgentSessionStatus::Active {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_NOT_ACTIVE: provider authority requires Active session {}, found {:?}",
            session.id, session.lifecycle
        )));
    }
    ledger
        .store
        .latest_node_daemon_lease(&session.node_id)?
        .filter(|lease| {
            lease.daemon_id == session.node_daemon_id
                && lease.generation == session.node_daemon_generation
                && lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED".into()))?;
    crate::provider_adapter::map_permission(
        &session.provider_kind,
        session.effective_permission_ceiling,
    )
    .map_err(CliError::Usage)?;
    Ok(session)
}

pub(super) fn require_member_provider_session_authority(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    require_active: bool,
) -> CliResult<harness_core::agentfirm_api::AgentSession> {
    let (_, session) = provider_session_for_member(ledger, member)?;
    if require_active
        && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Active
    {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_NOT_ACTIVE: provider authority requires Active session {}, found {:?}",
            session.id, session.lifecycle
        )));
    }
    Ok(session)
}

pub(super) fn claim_canonical_messages_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
) -> CliResult<Vec<TeamMessageProjection>> {
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    let execution_space_id = team_run_execution_space_id(&ledger.store, &run)?;
    let sessions = ledger
        .store
        .fabric_agent_sessions(&execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == member.agent_member_id
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .collect::<Vec<_>>();
    if sessions.is_empty() {
        return Ok(Vec::new());
    }
    if sessions.len() != 1 {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_AMBIGUOUS: {} has {} current sessions in TeamRun Execution Space {}",
            member.agent_member_id,
            sessions.len(),
            execution_space_id
        )));
    }
    let session = sessions.into_iter().next().expect("one session");
    let messages = ledger
        .store
        .fabric_messages(&execution_space_id)?
        .into_iter()
        .map(|message| (message.id.clone(), message))
        .collect::<BTreeMap<_, _>>();
    let mut queued = ledger
        .store
        .fabric_message_deliveries(&execution_space_id)?
        .into_iter()
        .filter(|delivery| {
            delivery.recipient_agent_member_id.as_deref() == Some(member.agent_member_id.as_str())
                && delivery.status
                    == harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued
        })
        .collect::<Vec<_>>();
    queued.sort_by(|left, right| {
        let left_message = messages.get(&left.message_id);
        let right_message = messages.get(&right.message_id);
        left_message
            .map(|message| message.created_at.as_str())
            .cmp(&right_message.map(|message| message.created_at.as_str()))
            .then_with(|| {
                let sequence = |id: &str| id.rsplit('-').next()?.parse::<u64>().ok();
                sequence(&left.message_id)
                    .cmp(&sequence(&right.message_id))
                    .then_with(|| left.message_id.cmp(&right.message_id))
            })
    });
    // Informational-only mail is durable context, not a provider-round
    // trigger. Keep it queued until a response-required message arrives; the
    // resulting round then consumes the entire ordered batch exactly once.
    // This is the canonical-ledger equivalent of
    // `claim_canonical_round_messages_for` for the provider inbox adapter.
    let triggers_round = queued.iter().any(|delivery| {
        messages.get(&delivery.message_id).is_some_and(|message| {
            message.response_intent == harness_core::agentfirm_api::ResponseIntent::ResponseRequired
                || message.kind
                    == harness_core::agentfirm_api::MessageKind::ProviderInteractionResponse
        })
    });
    if !triggers_round {
        return Ok(Vec::new());
    }
    let mut claimed_messages = Vec::new();
    for delivery in queued {
        let source = messages.get(&delivery.message_id).ok_or_else(|| {
            CliError::Usage(format!(
                "canonical RegistryDeliveryAttempt {} references missing TeamMessageProjection {}",
                delivery.id, delivery.message_id
            ))
        })?;
        if source.team_run_id.as_deref() != Some(ledger.run_id.as_str()) {
            continue;
        }
        let lease = ledger
            .store
            .latest_node_daemon_lease(&session.node_id)?
            .filter(|lease| {
                lease.daemon_id == session.node_daemon_id
                    && lease.generation == session.node_daemon_generation
                    && lease.status == NodeDaemonLeaseStatus::Active
                    && lease.expires_unix_ms > current_unix_ms_u64()
            })
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "NODE_DAEMON_GENERATION_FENCED: session {} has no current daemon",
                    session.id
                ))
            })?;
        let requested_mode = ledger
            .store
            .fabric_message_subscriptions(&execution_space_id)?
            .into_iter()
            .find(|subscription| subscription.id == delivery.subscription_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "MESSAGE_SUBSCRIPTION_NOT_FOUND: {}",
                    delivery.subscription_id
                ))
            })?
            .delivery_mode;
        let dispatch_mode = crate::provider_adapter::effective_delivery_mode(
            &session.provider_kind,
            requested_mode,
            session.lifecycle,
            false,
        )
        .map_err(CliError::Usage)?;
        let claim_id = generated_id("canonical-message-claim");
        let invocation = ledger.store.claim_message_for_provider(
            &canonical_delivery_context(
                &execution_space_id,
                &lease.daemon_id,
                "node_daemon.message_delivery.claim",
                format!("daemon:{}:{}:claim", lease.generation, delivery.id),
                delivery.version.saturating_sub(1),
            ),
            &delivery.id,
            &session.node_id,
            &lease.daemon_id,
            lease.generation,
            &claim_id,
            dispatch_mode,
            &now_string(),
        )?;
        let sender = match source.sender_actor_ref.kind {
            harness_core::agentfirm_api::ActorKind::AgentMember => TeamActorRef {
                kind: TeamActorKind::AgentMember,
                id: source.sender_actor_ref.id.clone(),
                display_name: None,
                authn_source: Some("canonical_trust_kernel".into()),
            },
            harness_core::agentfirm_api::ActorKind::Service => TeamActorRef {
                kind: TeamActorKind::Service,
                id: source.sender_actor_ref.id.clone(),
                display_name: None,
                authn_source: Some("canonical_trust_kernel".into()),
            },
            harness_core::agentfirm_api::ActorKind::Human
            | harness_core::agentfirm_api::ActorKind::External => TeamActorRef {
                kind: TeamActorKind::Operator,
                id: source.sender_actor_ref.id.clone(),
                display_name: None,
                authn_source: Some("canonical_trust_kernel".into()),
            },
        };
        claimed_messages.push(TeamMessageProjection {
            id: source.id.clone(),
            team_run_id: source
                .team_run_id
                .clone()
                .unwrap_or_else(|| ledger.run_id.clone()),
            work_id: source.work_id.clone(),
            source_plan_ref: None,
            sender: Some(sender),
            sender_runtime_id: source.sender_actor_ref.id.clone(),
            recipients: vec![TeamRecipientRef {
                kind: TeamRecipientKind::ProviderRuntimeProjection,
                id: member.id.clone(),
            }],
            recipient_runtime_ids: vec![member.id.clone()],
            kind: match source.kind {
                harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest => {
                    ProviderDispatchIntent::ProviderInteractionRequest
                }
                harness_core::agentfirm_api::MessageKind::ProviderInteractionResponse => {
                    ProviderDispatchIntent::ProviderInteractionResponse
                }
                harness_core::agentfirm_api::MessageKind::Message
                | harness_core::agentfirm_api::MessageKind::Reply
                | harness_core::agentfirm_api::MessageKind::RequestDecision => {
                    ProviderDispatchIntent::Message
                }
            },
            body: source.body.clone(),
            correlation_id: source.correlation_id.clone(),
            causation_id: source.causation_id.clone(),
            response_intent: Some(match source.response_intent {
                harness_core::agentfirm_api::ResponseIntent::Informational => {
                    ProviderResponseIntent::Informational
                }
                harness_core::agentfirm_api::ResponseIntent::ResponseRequired => {
                    ProviderResponseIntent::ResponseRequired
                }
            }),
            evidence_refs: source
                .evidence_refs
                .iter()
                .cloned()
                .chain([
                    format!("{CANONICAL_EXECUTION_SPACE_REF}{execution_space_id}"),
                    format!("{CANONICAL_MESSAGE_DELIVERY_REF}{}", delivery.id),
                ])
                .collect(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: member.id.clone(),
                policy: TeamDeliveryPolicy::Inject,
                status: TeamDeliveryStatus::Claimed,
                attempt: delivery.attempt,
                claim_id: Some(claim_id),
                claimed_by_supervisor_id: Some(lease.daemon_id.clone()),
                claimed_generation: Some(lease.generation),
                claimed_unix_ms: Some(current_unix_ms_u64()),
                claim_expires_unix_ms: Some(current_unix_ms_u64().saturating_add(30_000)),
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: now_string(),
            }],
            created_at: invocation.projection.created_at,
        });
    }
    Ok(claimed_messages)
}

#[derive(Clone, Copy)]
pub(super) enum LiveMemberControlRequirement {
    Steer,
    Interrupt,
    Close,
    RoleAction,
}

pub(super) struct LiveMemberControlRegistration {
    pub(super) member_run_id: String,
}

pub(super) struct TeamSupervisorRegistration {
    pub(super) team_run_id: String,
    pub(super) supervisor_id: String,
    pub(super) generation: u64,
    pub(super) store: HarnessStore,
    pub(super) heartbeat_stop: Arc<AtomicBool>,
    pub(super) heartbeat_valid: Arc<AtomicBool>,
    /// Serializes the process-local lease-loss latch with live Close
    /// admission. Durable generation changes are serialized separately by the
    /// Store writer lock.
    pub(super) authority_gate: Arc<Mutex<()>>,
    pub(super) heartbeat_thread: Option<std::thread::JoinHandle<()>>,
    pub(super) control_stop: Arc<AtomicBool>,
    pub(super) control_thread: Option<std::thread::JoinHandle<()>>,
}

impl TeamSupervisorRegistration {
    /// Acquire a supervisor lease, bind a TCP control listener, and start
    /// heartbeat + control threads. Does NOT register in LIVE_TEAM_SUPERVISORS
    /// — callers that need in-process deduplication must manage that guard
    /// themselves (e.g. `reserve_team_supervisor`).
    pub(crate) fn start(
        store: &HarnessStore,
        team_run_id: &str,
        expected_execution_space_id: Option<&str>,
    ) -> CliResult<TeamSupervisorRegistration> {
        let supervisor_id = generated_id("supervisor");
        let ttl_ms = team_supervisor_lease_ttl_ms();
        let control_listener = TcpListener::bind("127.0.0.1:0")?;
        control_listener.set_nonblocking(true)?;
        let owner_locator = format!("tcp://{}", control_listener.local_addr()?);
        let run = latest_team_run(store, team_run_id)?;
        let parent = store
            .latest_node_daemon_lease(&run.execution_node_id)?
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "NODE_DAEMON_UNAVAILABLE: Node {} has no daemon lease",
                    run.execution_node_id
                ))
            })?;
        let now_ms = current_unix_ms_u64();
        if parent.status != NodeDaemonLeaseStatus::Active || parent.expires_unix_ms <= now_ms {
            return Err(CliError::Usage(format!(
                "NODE_DAEMON_UNAVAILABLE: Node {} has no active daemon generation",
                run.execution_node_id
            )));
        }
        let registrations = store
            .latest_node_project_registrations()?
            .into_iter()
            .filter(|registration| {
                registration.node_id == run.execution_node_id
                    && registration.project_binding_id == run.project_binding_id
                    && registration.status == NodeProjectRegistrationStatus::Active
                    && expected_execution_space_id
                        .is_none_or(|expected| registration.execution_space_id == expected)
            })
            .collect::<Vec<_>>();
        if registrations.len() != 1 {
            return Err(CliError::Usage(format!(
                "PROJECT_NOT_REGISTERED_ON_NODE: expected one active registration for {} on {} in Execution Space {}, found {}",
                run.project_binding_id,
                run.execution_node_id,
                expected_execution_space_id.unwrap_or("<unambiguous>"),
                registrations.len()
            )));
        }
        let registration = registrations.into_iter().next().ok_or_else(|| {
            CliError::Usage(format!(
                "PROJECT_NOT_REGISTERED_ON_NODE: {} on {}",
                run.project_binding_id, run.execution_node_id
            ))
        })?;
        let lease = store
            .acquire_team_supervisor_under_node_lease(
                team_run_id,
                &run.execution_node_id,
                &parent.daemon_id,
                parent.generation,
                &registration.execution_space_id,
                &run.project_binding_id,
                &supervisor_id,
                std::process::id(),
                &owner_locator,
                now_ms,
                ttl_ms,
            )
            // Typed, not flattened: a lost Supervisor-lease race on the
            // adoption/start path must reach `start_failure_is_transient` as a
            // Store conflict, never as an uncoded `CliError::Usage` that reads
            // as a structural defect (DEV-149-REVIEW-02).
            .map_err(CliError::Store)?;
        let heartbeat_stop = Arc::new(AtomicBool::new(false));
        let heartbeat_valid = Arc::new(AtomicBool::new(true));
        let authority_gate = Arc::new(Mutex::new(()));
        let heartbeat_store = store.clone();
        let heartbeat_stop_thread = Arc::clone(&heartbeat_stop);
        let heartbeat_valid_thread = Arc::clone(&heartbeat_valid);
        let heartbeat_authority_gate = Arc::clone(&authority_gate);
        let generation = lease.generation;
        let heartbeat_policy = SupervisorHeartbeatPolicy {
            team_run_id: team_run_id.to_string(),
            supervisor_id: supervisor_id.clone(),
            generation,
            ttl_ms,
            heartbeat_interval_ms: (ttl_ms / 3).clamp(50, 1_000),
            max_transient_failures: MAX_TRANSIENT_SUPERVISOR_RENEWAL_FAILURES,
        };
        let heartbeat_thread = std::thread::spawn(move || {
            let heartbeat_store = heartbeat_store;
            let heartbeat_policy = heartbeat_policy;
            run_supervisor_heartbeat_loop(
                &heartbeat_policy,
                &heartbeat_stop_thread,
                &heartbeat_valid_thread,
                &heartbeat_authority_gate,
                || {
                    heartbeat_store
                        .renew_team_supervisor_lease(
                            &heartbeat_policy.team_run_id,
                            &heartbeat_policy.supervisor_id,
                            heartbeat_policy.generation,
                            current_unix_ms_u64(),
                            heartbeat_policy.ttl_ms,
                        )
                        .map(|_lease| ())
                },
                supervisor_test_heartbeat_failure_marker,
            );
        });
        let control_stop = Arc::new(AtomicBool::new(false));
        let control_store = store.clone();
        let control_team_run_id = team_run_id.to_string();
        let control_supervisor_id = supervisor_id.clone();
        let control_generation = lease.generation;
        let control_stop_thread = Arc::clone(&control_stop);
        let control_valid_thread = Arc::clone(&heartbeat_valid);
        let control_authority_gate = Arc::clone(&authority_gate);
        let control_thread = std::thread::spawn(move || {
            while !control_stop_thread.load(Ordering::Acquire) {
                match control_listener.accept() {
                    Ok((stream, _)) => {
                        handle_live_member_control_connection(
                            stream,
                            &control_store,
                            &control_team_run_id,
                            &control_supervisor_id,
                            control_generation,
                            &control_valid_thread,
                            &control_authority_gate,
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(TeamSupervisorRegistration {
            team_run_id: team_run_id.to_string(),
            supervisor_id,
            generation: lease.generation,
            store: store.clone(),
            heartbeat_stop,
            heartbeat_valid,
            authority_gate,
            heartbeat_thread: Some(heartbeat_thread),
            control_stop,
            control_thread: Some(control_thread),
        })
    }
}

impl Drop for TeamSupervisorRegistration {
    fn drop(&mut self) {
        self.control_stop.store(true, Ordering::Release);
        if let Some(handle) = self.control_thread.take() {
            let _ = handle.join();
        }
        self.heartbeat_stop.store(true, Ordering::Release);
        if let Some(handle) = self.heartbeat_thread.take() {
            let _ = handle.join();
        }
        let _ = self.store.release_team_supervisor_lease(
            &self.team_run_id,
            &self.supervisor_id,
            self.generation,
            current_unix_ms_u64(),
        );
        let _authority_guard = self
            .authority_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.heartbeat_valid.store(false, Ordering::Release);
        LIVE_TEAM_SUPERVISORS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.team_run_id);
    }
}
