use super::*;

/// A provider-STRUCTURED terminal failure: fields the provider transport
/// itself reported, never prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderTerminalFailure {
    /// The provider's own terminal reason token (for example `api_error`).
    pub(crate) reason: String,
    /// The provider's own HTTP status, when the transport reported one.
    pub(crate) http_status: Option<i64>,
}

/// Prefix of the canonical token stored in `MemberAction.provider_status`.
pub(super) const PROVIDER_TERMINAL_STATUS_PREFIX: &str = "provider_terminal";

impl ProviderTerminalFailure {
    /// `provider_terminal:<reason>:<http status or ->`.
    pub(crate) fn to_provider_status(&self) -> String {
        let status = self
            .http_status
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_string());
        format!(
            "{PROVIDER_TERMINAL_STATUS_PREFIX}:{}:{status}",
            self.reason.trim()
        )
    }

    pub(super) fn parse(provider_status: &str) -> Option<Self> {
        let rest = provider_status.strip_prefix(PROVIDER_TERMINAL_STATUS_PREFIX)?;
        // Split from the RIGHT: the status is the last field, so a reason that
        // itself contains `:` stays intact.
        let (reason, status) = rest.strip_prefix(':')?.rsplit_once(':')?;
        Some(Self {
            reason: reason.to_string(),
            http_status: status.parse::<i64>().ok(),
        })
    }
}

/// Reasons a provider transport reports for a spent account.
pub(super) const PROVIDER_EXHAUSTED_REASONS: &[&str] = &[
    "rate_limit",
    "rate_limit_reached",
    "usage_limit_reached",
    "quota_exceeded",
    "credits_depleted",
];

/// Reasons a provider transport reports for a rejected credential.
pub(super) const PROVIDER_UNAUTHORIZED_REASONS: &[&str] = &[
    "auth_error",
    "authentication_error",
    "forbidden",
    "unauthorized",
];

/// Classify a provider-STRUCTURED terminal failure into a capacity state.
///
/// Only the transport's own fields are read: an exact HTTP status integer and a
/// closed reason vocabulary. Free text is never scanned, because the recorded
/// summary also carries the MEMBER's own first line — a member writing "fixed
/// the 403 handler" must not mark its account unauthorized — and because
/// substring matching cannot tell `403` from `1403`. An unrecognised failure
/// stays `None` rather than becoming a gate.
pub(super) fn capacity_state_from_provider_terminal(
    failure: &ProviderTerminalFailure,
) -> Option<(ProviderCapacityState, String)> {
    let reason = failure.reason.trim().to_ascii_lowercase();
    match failure.http_status {
        Some(429) => {
            return Some((
                ProviderCapacityState::Exhausted,
                "the provider transport reported HTTP 429".to_string(),
            ))
        }
        Some(401) | Some(403) => {
            return Some((
                ProviderCapacityState::Unauthorized,
                format!(
                    "the provider transport reported HTTP {}",
                    failure.http_status.unwrap_or_default()
                ),
            ))
        }
        _ => {}
    }
    if PROVIDER_EXHAUSTED_REASONS.contains(&reason.as_str()) {
        return Some((
            ProviderCapacityState::Exhausted,
            format!("the provider transport reported terminal reason {reason}"),
        ));
    }
    if PROVIDER_UNAUTHORIZED_REASONS.contains(&reason.as_str()) {
        return Some((
            ProviderCapacityState::Unauthorized,
            format!("the provider transport reported terminal reason {reason}"),
        ));
    }
    None
}

/// Staleness bound for a start-time capacity decision, overridable for tests.
pub(super) fn capacity_ttl_ms() -> u64 {
    std::env::var("FIRM_CAPACITY_TTL_MS")
        .or_else(|_| std::env::var("HARNESS_CAPACITY_TTL_MS"))
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(harness_core::PROVIDER_CAPACITY_DEFAULT_TTL_MS)
}

/// The start guard is on by default. `FIRM_CAPACITY_PREFLIGHT=off` disables
/// only the probe; the honest-unknown semantics are unchanged, because a
/// disabled probe simply produces no snapshot and no snapshot never blocks.
/// `HARNESS_CAPACITY_PREFLIGHT` remains a compatibility alias.
pub(super) fn capacity_preflight_enabled() -> bool {
    !matches!(
        std::env::var("FIRM_CAPACITY_PREFLIGHT")
            .or_else(|_| std::env::var("HARNESS_CAPACITY_PREFLIGHT"))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "off" | "0" | "false" | "no"
    )
}

/// Merge a recorded terminal failure INTO the current probe observation.
///
/// The probe knows things the recorded row cannot: which proxy variables exist
/// in this process right now, and whether the account/source could be read.
/// Replacing the probe wholesale threw that away and turned the exact Wave 2
/// scenario — no `HTTP(S)_PROXY`, blocked egress, provider answers `403` —
/// into `unauthorized`, gating a healthy account behind a missing env var.
/// That is precisely the misdiagnosis this Work exists to prevent, and it
/// contradicted the canary path, which returns `unknown` for the same failure.
///
/// So: for a mode whose failure is known to be proxy-shaped, a missing proxy
/// takes precedence over a recorded credential rejection. The recorded failure
/// is preserved in `detail` — it is real evidence, just not a verdict.
pub(super) fn reconcile_recorded_capacity(
    probe: ProviderCapacitySnapshot,
    recorded: ProviderCapacitySnapshot,
) -> ProviderCapacitySnapshot {
    let proxy_shaped_mode = probe.execution_mode == "claude_agent_sdk";
    let missing_proxy = !claude_has_proxy_configured(&probe.runtime_context);
    let credential_shaped = recorded.state == ProviderCapacityState::Unauthorized;

    if proxy_shaped_mode && missing_proxy && credential_shaped {
        let recorded_detail = recorded
            .detail
            .clone()
            .unwrap_or_else(|| "a recorded terminal failure rejected the credential".to_string());
        return ProviderCapacitySnapshot {
            // Keep the PROBE's state: unknown, so no start is gated.
            state: ProviderCapacityState::Unknown,
            confidence: ProviderCapacityConfidence::Unknown,
            diagnosis: Some(claude_missing_proxy_diagnosis()),
            detail: Some(format!(
                "{recorded_detail}, but the Harness process has no HTTP(S)_PROXY, so the recorded \
                 rejection is not attributed to the account until the request is retried through \
                 a proxy"
            )),
            ..probe
        };
    }

    // Otherwise the recorded state stands, but it inherits the probe's live
    // runtime facts and account boundary instead of discarding them.
    //
    // The DIAGNOSIS is not inherited when the recorded state is known
    // unavailable. The probe's diagnosis is about reachability — it is set
    // whenever no proxy is configured — and a spent quota is not caused by a
    // missing proxy. Letting a recorded 429 inherit it would block correctly as
    // `exhausted` while telling the operator to go fix their proxy, sending
    // them after the wrong cause. The runtime facts still travel in
    // `runtime_context`, where they are evidence rather than causation.
    let diagnosis = match recorded.diagnosis {
        Some(diagnosis) => Some(diagnosis),
        None if recorded.state.is_known_unavailable() => None,
        None => probe.diagnosis,
    };
    ProviderCapacitySnapshot {
        account: if recorded.account.source == "unknown" {
            probe.account
        } else {
            recorded.account
        },
        runtime_context: if recorded.runtime_context.is_empty() {
            probe.runtime_context
        } else {
            recorded.runtime_context
        },
        diagnosis,
        ..recorded
    }
}

/// Derive a capacity snapshot from the STRUCTURED terminal failures this
/// member already recorded.
///
/// Only an execution mode whose transport reports structured terminal metadata
/// can produce one of these. Today that is `claude_agent_sdk`
/// (`terminal_reason` + `api_error_status`). Kimi ACP surfaces a 403 as
/// free-form JSON-RPC error text with no status field, and Codex app-server
/// errors arrive as adapter strings; neither is classified, so neither
/// fabricates a capacity verdict. Only failures newer than the TTL count, so a
/// recovered account is not gated by yesterday's 403.
pub(super) fn capacity_from_recorded_provider_errors(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    execution_mode: &str,
    now_unix_ms: u64,
    ttl_ms: u64,
) -> CliResult<Option<ProviderCapacitySnapshot>> {
    let actions = ledger.store.member_actions()?;
    Ok(capacity_from_provider_error_actions(
        &actions,
        &member.id,
        &member.provider,
        execution_mode,
        now_unix_ms,
        ttl_ms,
    ))
}

/// Pure selection half of [`capacity_from_recorded_provider_errors`]: the
/// newest CLASSIFIABLE structured failure for this member inside the TTL.
///
/// The search walks backwards and keeps going past rows it cannot classify —
/// silent-round rows carry no structured metadata at all, and one of them
/// sitting on top must never bury a real 403 recorded moments earlier.
pub(super) fn capacity_from_provider_error_actions(
    actions: &[MemberAction],
    member_run_id: &str,
    provider: &str,
    execution_mode: &str,
    now_unix_ms: u64,
    ttl_ms: u64,
) -> Option<ProviderCapacitySnapshot> {
    let (action, (state, detail)) = actions
        .iter()
        .rev()
        .filter(|action| {
            action.member_run_id == member_run_id
                && action.action_type == "provider_error"
                && parse_unix_ms_timestamp(&action.started_at)
                    .is_some_and(|stamp| stamp <= now_unix_ms && now_unix_ms - stamp <= ttl_ms)
        })
        .find_map(|action| {
            // Structured transport metadata ONLY. `summary` also carries the
            // member's own first line and must never reach a classifier.
            let failure = ProviderTerminalFailure::parse(action.provider_status.as_deref()?)?;
            capacity_state_from_provider_terminal(&failure).map(|classified| (action, classified))
        })?;
    let observed_unix_ms = parse_unix_ms_timestamp(&action.started_at).unwrap_or(now_unix_ms);
    Some(ProviderCapacitySnapshot {
        provider: provider.to_string(),
        execution_mode: execution_mode.to_string(),
        account: ProviderAccountRef::unknown(),
        state,
        observed_at: action.started_at.clone(),
        observed_unix_ms,
        reset_at: None,
        evidence_source: ProviderCapacityEvidence::ProviderError,
        confidence: ProviderCapacityConfidence::Observed,
        windows: Vec::new(),
        diagnosis: None,
        runtime_context: Vec::new(),
        detail: Some(format!("{detail}: {}", action.summary)),
    })
}

pub(super) fn parse_unix_ms_timestamp(raw: &str) -> Option<u64> {
    harness_core::parse_harness_unix_ms(raw)
}

/// Clear only the Blocked projection that this capacity gate itself authored.
/// Other Blocked reasons deliberately survive a successful capacity probe.
pub(super) fn recover_capacity_origin_block(member: &mut ProviderRuntimeProjection) {
    let was_capacity_blocked = member.status == MemberRunStatus::Blocked
        && member
            .provider_capacity
            .as_ref()
            .is_some_and(|capacity| capacity.state.is_known_unavailable());
    if was_capacity_blocked && member.coordination_is_active() {
        member.status = if member.native_session.is_some() {
            MemberRunStatus::Disconnected
        } else {
            MemberRunStatus::Idle
        };
        member.finished_at = None;
    }
}

pub(super) fn apply_nonblocking_capacity_observation(
    member: &mut ProviderRuntimeProjection,
    snapshot: ProviderCapacitySnapshot,
) {
    // Recovery provenance lives on the previous observation. Clear the
    // capacity-authored Blocked projection before replacing that evidence
    // with the fresh available/unknown snapshot.
    recover_capacity_origin_block(member);
    member.provider_capacity = Some(snapshot);
    member.last_event_at = Some(now_string());
}

pub(super) fn reconcile_pending_close_during_capacity_recovery(
    ledger: &TeamRunLedger,
    member: &mut ProviderRuntimeProjection,
) -> CliResult<Option<MemberOutcome>> {
    if let Some(close) = pending_member_close(&ledger.store, &member.id)? {
        for _ in 0..PROVIDER_MEMBER_CAS_RETRIES {
            let mut latest = ledger
                .latest_member_run(&member.id)?
                .unwrap_or_else(|| member.clone());
            match stop_member_for_latched_close(ledger, &mut latest, &close) {
                Ok(()) => {
                    *member = latest.clone();
                    return Ok(Some(MemberOutcome::new(
                        &latest,
                        MemberRunStatus::Stopped,
                        "member runtime closed by Host during capacity recovery".to_string(),
                    )));
                }
                Err(CliError::Store(StoreError::Conflict(_))) => continue,
                Err(error) => return Err(error),
            }
        }
        let latest = ledger
            .latest_member_run(&member.id)?
            .unwrap_or_else(|| member.clone());
        *member = latest.clone();
        return Ok(Some(MemberOutcome::new(
            &latest,
            latest.status,
            "capacity recovery stopped after bounded Close reconciliation contention".to_string(),
        )));
    }

    let latest = ledger
        .latest_member_run(&member.id)?
        .unwrap_or_else(|| member.clone());
    if !latest.coordination_is_active()
        || matches!(
            latest.status,
            MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
        )
    {
        *member = latest.clone();
        return Ok(Some(MemberOutcome::new(
            &latest,
            latest.status,
            "member lifecycle superseded capacity recovery".to_string(),
        )));
    }
    Ok(None)
}

/// Observe this member's provider capacity and decide whether it may start.
///
/// Returns `Some(outcome)` when the member must NOT proceed. The caller runs
/// this BEFORE the adapter claims its Assignment, so a blocked member leaves
/// its queued Assignment untouched and re-deliverable.
pub(super) fn provider_capacity_start_gate(
    ledger: &TeamRunLedger,
    member: &mut ProviderRuntimeProjection,
    cwd: &Path,
) -> CliResult<Option<MemberOutcome>> {
    provider_capacity_start_gate_with_hook(ledger, member, cwd, |_, _| Ok(()))
}

pub(super) fn provider_capacity_start_gate_with_hook(
    ledger: &TeamRunLedger,
    member: &mut ProviderRuntimeProjection,
    cwd: &Path,
    before_capacity_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<Option<MemberOutcome>> {
    if !capacity_preflight_enabled() {
        return Ok(None);
    }
    let expected = member.clone();
    // A member pinned to a mode this preflight does not probe is simply not
    // gated: no observation is honest here, and no observation never blocks.
    let Ok(execution_mode) = capacity_execution_mode(
        &member.provider,
        member
            .provider_profile
            .as_ref()
            .map(|profile| profile.execution_mode.as_str()),
    ) else {
        return Ok(None);
    };
    let mut snapshot = provider_capacity_probe(
        &member.provider,
        &execution_mode,
        cwd,
        CapacityProbeOptions::default(),
    );
    let ttl_ms = capacity_ttl_ms();
    let now_unix_ms = current_unix_ms_u64();
    // A live provider answer wins. Recorded terminal errors are consulted only
    // when the probe itself could not observe a state, and they are MERGED into
    // the probe rather than replacing it.
    if snapshot.state == ProviderCapacityState::Unknown {
        if let Some(recorded) = capacity_from_recorded_provider_errors(
            ledger,
            member,
            &execution_mode,
            now_unix_ms,
            ttl_ms,
        )? {
            snapshot = reconcile_recorded_capacity(snapshot, recorded);
        }
    }
    let decision =
        harness_core::provider_capacity_start_decision(Some(&snapshot), now_unix_ms, ttl_ms);
    if !decision.is_blocked() {
        // Only a Blocked row carrying explicit capacity-unavailable provenance
        // is self-recovering. Compatibility, degradation, review, and operator
        // blocks remain authoritative until their own control path clears them.
        apply_nonblocking_capacity_observation(member, snapshot.clone());
        if let Some(outcome) = persist_capacity_recovery_with_hook(
            ledger,
            &expected,
            member,
            snapshot,
            before_capacity_cas,
        )? {
            return Ok(Some(outcome));
        }
        return Ok(None);
    }
    member.provider_capacity = Some(snapshot.clone());
    member.last_event_at = Some(now_string());
    // Blocked: record provider_unavailable and stop. Nothing above this point
    // claimed, delivered, or consumed a TeamMessageProjection.
    //
    // Every write below is BEST EFFORT and this function still returns the
    // blocking outcome. A `?` here would turn a journal failure into `Err`,
    // and the caller treats `Err` as "carry on" — so a store hiccup would let
    // the member start on the exhausted account this gate just refused. The
    // decision is the product fact; the journal is its record, not its gate.
    member.status = MemberRunStatus::Blocked;
    let summary = format!(
        "{} (evidence {:?}, confidence {:?}){}",
        decision.reason(),
        snapshot.evidence_source,
        snapshot.confidence,
        snapshot
            .diagnosis
            .as_ref()
            .map(|diagnosis| format!("; {diagnosis}"))
            .unwrap_or_default()
    );
    if let Some(outcome) = persist_capacity_block_with_hook(
        ledger,
        &expected,
        member,
        snapshot.clone(),
        before_capacity_cas,
    )? {
        return Ok(Some(outcome));
    }
    let mut journal_errors = Vec::new();
    let state_label = serde_json::to_value(snapshot.state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    match ledger.append_action(
        &member.id,
        "provider_unavailable",
        MemberActionStatus::Failed,
        "provider capacity preflight blocked start",
        &summary,
    ) {
        Ok(action) => {
            if let Err(error) = ledger.fold_event(
                TeamRunEventSourceKind::Host,
                Some(member.id.clone()),
                "action",
                &action.id,
                "created",
                &format!(
                    "{} not started: provider_unavailable ({state_label})",
                    member.name
                ),
            ) {
                journal_errors.push(error.to_string());
            }
        }
        Err(error) => journal_errors.push(error.to_string()),
    }
    if let Err(error) = ledger.fold_event(
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "provider_unavailable",
        &summary,
    ) {
        journal_errors.push(error.to_string());
    }
    let summary = if journal_errors.is_empty() {
        summary
    } else {
        format!(
            "{summary}; the block is authoritative but {} journal write(s) failed: {}",
            journal_errors.len(),
            journal_errors.join("; ")
        )
    };
    Ok(Some(MemberOutcome::new(
        member,
        MemberRunStatus::Blocked,
        summary,
    )))
}

/// Publish a fresh non-blocking capacity observation without reviving a
/// lifecycle decision that landed while the provider probe was in flight.
/// Only an explicit capacity-origin Blocked row may self-recover; all other
/// Blocked reasons remain owned by their corresponding control path. As with
/// the provider-start claim and capacity-block paths, a durable Close latch is
/// re-read after a successful CAS so Close wins the latch-before-CAS window.
pub(super) fn persist_capacity_recovery_with_hook(
    ledger: &TeamRunLedger,
    anchor: &ProviderRuntimeProjection,
    member: &mut ProviderRuntimeProjection,
    snapshot: ProviderCapacitySnapshot,
    before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<Option<MemberOutcome>> {
    persist_capacity_recovery_with_hooks(
        ledger,
        anchor,
        member,
        snapshot,
        before_cas,
        |_, _| Ok(()),
    )
}

pub(super) fn persist_capacity_recovery_with_hooks(
    ledger: &TeamRunLedger,
    anchor: &ProviderRuntimeProjection,
    member: &mut ProviderRuntimeProjection,
    snapshot: ProviderCapacitySnapshot,
    mut before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
    mut after_successful_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<Option<MemberOutcome>> {
    let mut expected = anchor.clone();
    for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
        ledger.require_supervisor_lease()?;
        if let Some(outcome) = reconcile_pending_close_during_capacity_recovery(ledger, member)? {
            return Ok(Some(outcome));
        }
        before_cas(attempt, &expected)?;
        match ledger.save_member_run(&expected, member) {
            Ok(()) => {
                after_successful_cas(attempt, member)?;
                if let Some(outcome) =
                    reconcile_pending_close_during_capacity_recovery(ledger, member)?
                {
                    return Ok(Some(outcome));
                }
                *member = ledger
                    .latest_member_run(&member.id)?
                    .unwrap_or_else(|| member.clone());
                return Ok(None);
            }
            Err(CliError::Store(StoreError::Conflict(_))) => {
                if let Some(outcome) =
                    reconcile_pending_close_during_capacity_recovery(ledger, member)?
                {
                    return Ok(Some(outcome));
                }
                let Some(latest) = ledger.latest_member_run(&anchor.id)? else {
                    return Ok(Some(MemberOutcome::new(
                        anchor,
                        anchor.status,
                        "member disappeared while capacity recovery was being recorded".to_string(),
                    )));
                };
                let benign_same_runtime = member_runtime_anchor_matches(anchor, &latest)
                    && latest.status == anchor.status
                    && latest.native_session == anchor.native_session;
                if !benign_same_runtime {
                    *member = latest.clone();
                    return Ok(Some(MemberOutcome::new(
                        &latest,
                        latest.status,
                        "member runtime authority superseded capacity recovery".to_string(),
                    )));
                }
                expected = latest.clone();
                *member = latest;
                apply_nonblocking_capacity_observation(member, snapshot.clone());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Some(MemberOutcome::new(
        member,
        member.status,
        "capacity recovery CAS contention exceeded the bounded retry budget".to_string(),
    )))
}

/// Publish a capacity-origin Blocked transition without losing a concurrent
/// lifecycle decision. Close wins a conflicting CAS and is fully applied;
/// unrelated authority drift is returned as superseded. Only benign drift on
/// the same active runtime generation may be rebased, and retries are bounded.
pub(super) fn persist_capacity_block_with_hook(
    ledger: &TeamRunLedger,
    anchor: &ProviderRuntimeProjection,
    member: &mut ProviderRuntimeProjection,
    snapshot: ProviderCapacitySnapshot,
    mut before_cas: impl FnMut(usize, &ProviderRuntimeProjection) -> CliResult<()>,
) -> CliResult<Option<MemberOutcome>> {
    let mut expected = anchor.clone();
    for attempt in 0..PROVIDER_MEMBER_CAS_RETRIES {
        ledger.require_supervisor_lease()?;
        before_cas(attempt, &expected)?;
        match ledger.save_member_run(&expected, member) {
            Ok(()) => {
                // A Close latch does not mutate ProviderRuntimeProjection until its control
                // path marks coordination Closed, so it can land immediately
                // before this CAS without causing a conflict. Mirror the
                // provider-start claim fence: re-read the latch after the
                // successful CAS and apply it before returning a terminal
                // capacity outcome with no live provider loop left to do so.
                if let Some(close) = pending_member_close(&ledger.store, &member.id)? {
                    let mut latest = ledger
                        .latest_member_run(&member.id)?
                        .unwrap_or_else(|| member.clone());
                    stop_member_for_latched_close(ledger, &mut latest, &close)?;
                    *member = latest.clone();
                    return Ok(Some(MemberOutcome::new(
                        &latest,
                        MemberRunStatus::Stopped,
                        "member runtime closed by Host during capacity preflight".to_string(),
                    )));
                }
                return Ok(None);
            }
            Err(CliError::Store(StoreError::Conflict(_))) => {
                let Some(mut latest) = ledger.latest_member_run(&anchor.id)? else {
                    return Ok(Some(MemberOutcome::new(
                        anchor,
                        anchor.status,
                        "member disappeared while capacity block was being recorded".to_string(),
                    )));
                };
                if let Some(close) = pending_member_close(&ledger.store, &latest.id)? {
                    match stop_member_for_latched_close(ledger, &mut latest, &close) {
                        Ok(()) => {
                            *member = latest.clone();
                            return Ok(Some(MemberOutcome::new(
                                &latest,
                                MemberRunStatus::Stopped,
                                "member runtime closed by Host during capacity preflight"
                                    .to_string(),
                            )));
                        }
                        Err(CliError::Store(StoreError::Conflict(_)))
                            if attempt + 1 < PROVIDER_MEMBER_CAS_RETRIES =>
                        {
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                }
                if !latest.coordination_is_active()
                    || matches!(
                        latest.status,
                        MemberRunStatus::Completed
                            | MemberRunStatus::Failed
                            | MemberRunStatus::Stopped
                    )
                {
                    *member = latest.clone();
                    return Ok(Some(MemberOutcome::new(
                        &latest,
                        latest.status,
                        "member lifecycle superseded capacity preflight".to_string(),
                    )));
                }
                let benign_same_runtime = member_runtime_anchor_matches(anchor, &latest)
                    && latest.status == anchor.status
                    && latest.native_session == anchor.native_session;
                if !benign_same_runtime {
                    *member = latest.clone();
                    return Ok(Some(MemberOutcome::new(
                        &latest,
                        latest.status,
                        "member runtime authority superseded capacity preflight".to_string(),
                    )));
                }
                expected = latest.clone();
                *member = latest;
                member.provider_capacity = Some(snapshot.clone());
                member.status = MemberRunStatus::Blocked;
                member.last_event_at = Some(now_string());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Some(MemberOutcome::new(
        member,
        member.status,
        "capacity block CAS contention exceeded the bounded retry budget".to_string(),
    )))
}

/// One provider row of `harness member preflight`.
///
/// `capacity` and `compatibility` are deliberately SIBLINGS, never merged: a
/// reviewed-current adapter with an exhausted account is a normal, expressible
/// state and the Dashboard must be able to show both.
pub(super) fn provider_preflight_row(
    store: &HarnessStore,
    provider: &str,
    requested_mode: Option<&str>,
    cwd: &Path,
    options: CapacityProbeOptions,
    ttl_ms: u64,
) -> CliResult<serde_json::Value> {
    let execution_mode = capacity_execution_mode(provider, requested_mode)?;
    let mut profile = team_member_provider_profile_for_mode(provider, Some(&execution_mode));
    let detected = team_member_provider_version_output(provider);
    apply_provider_version(&mut profile, detected.as_ref().ok().cloned());
    let compatibility = resolve_provider_compatibility(
        store,
        &profile,
        detected.as_ref().err().map(String::as_str),
    )?;
    let capacity = provider_capacity_probe(provider, &execution_mode, cwd, options);
    // Read the clock AFTER the probe: a probe that takes seconds must not make
    // its own answer look future-dated, which would report it as stale.
    let now_unix_ms = current_unix_ms_u64();
    let capacity_decision =
        harness_core::provider_capacity_start_decision(Some(&capacity), now_unix_ms, ttl_ms);
    let start_decision = provider_preflight_start_decision(&capacity_decision, &compatibility);
    Ok(serde_json::json!({
        "provider": provider,
        "execution_mode": execution_mode,
        "capacity": capacity,
        "capacity_freshness": capacity.freshness(now_unix_ms, ttl_ms),
        "blocked": start_decision.get("decision") == Some(&serde_json::json!("block")),
        "start_decision": start_decision,
        "compatibility": {
            "status": profile.compatibility_status,
            "provider_version": profile.provider_version,
            "reviewed_provider_versions": profile.reviewed_provider_versions,
            "adapter_contract_version": profile.adapter_contract_version,
            "version_probe_error": detected.err(),
            "operational": compatibility,
        },
    }))
}

pub(super) fn provider_preflight_start_decision(
    capacity_decision: &harness_core::ProviderCapacityStartDecision,
    compatibility: &ProviderCompatibilityResolution,
) -> serde_json::Value {
    if compatibility.allowed {
        serde_json::to_value(capacity_decision).unwrap_or_default()
    } else {
        serde_json::json!({
            "decision": "block",
            "gate": "provider_compatibility",
            "reason": format!(
                "provider compatibility is {} (source={})",
                serde_snake_label(&compatibility.status),
                compatibility.source,
            ),
        })
    }
}

pub(super) fn member_preflight_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let providers = {
        let requested = many(args, "--provider");
        if requested.is_empty() {
            provider_registry()
                .iter()
                .map(|adapter| adapter.name().to_string())
                .collect::<Vec<_>>()
        } else {
            requested
        }
    };
    let requested_mode = value(args, "--execution-mode");
    let options = CapacityProbeOptions {
        canary: has_flag(args, "--canary"),
        timeout: Duration::from_secs(
            value(args, "--timeout-s")
                .and_then(|raw| raw.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(30),
        ),
    };
    let cwd = std::env::current_dir().map_err(|error| {
        CliError::Usage(format!("failed to resolve current directory: {error}"))
    })?;
    let ttl_ms = capacity_ttl_ms();
    let rows = providers
        .iter()
        .map(|provider| {
            provider_preflight_row(
                store,
                provider,
                requested_mode.as_deref(),
                &cwd,
                options,
                ttl_ms,
            )
        })
        .collect::<CliResult<Vec<_>>>()?;
    let blocked = rows
        .iter()
        .filter(|row| row.pointer("/start_decision/decision") == Some(&serde_json::json!("block")))
        .count();
    let needs_review = rows
        .iter()
        .filter(|row| {
            row.pointer("/compatibility/operational/needs_review") == Some(&serde_json::json!(true))
        })
        .count();
    if has_flag(args, "--json") {
        print_json(&serde_json::json!({
            "command": "harness member preflight",
            "ok": true,
            "result": {
                "generated_at": now_string(),
                "ttl_ms": ttl_ms,
                "canary": options.canary,
                "cwd": cwd,
                "providers": rows,
            },
        }))?;
    } else {
        // One line per provider. Capacity and compatibility stay in separate
        // columns here too, so the operator never reads one as the other.
        for row in &rows {
            let text = |pointer: &str| {
                row.pointer(pointer)
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
                    .to_string()
            };
            println!(
                "{}\t{}\tcapacity={} ({}, {})\tstart={}\tcompatibility={}",
                text("/provider"),
                text("/execution_mode"),
                text("/capacity/state"),
                text("/capacity/evidence_source"),
                text("/capacity_freshness"),
                text("/start_decision/decision"),
                text("/compatibility/status"),
            );
        }
    }
    if has_flag(args, "--fail-on-unavailable") && blocked > 0 {
        return Err(CliError::Usage(format!(
            "{blocked} provider(s) are blocked by capacity or compatibility; inspect the JSON report"
        )));
    }
    if has_flag(args, "--fail-on-review") && needs_review > 0 {
        return Err(CliError::Usage(format!(
            "{needs_review} provider(s) still require source review; inspect the JSON report"
        )));
    }
    Ok(())
}
