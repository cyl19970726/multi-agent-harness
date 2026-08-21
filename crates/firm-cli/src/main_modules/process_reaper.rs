use super::*;

pub(super) fn pid_exists_libc(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error()
            .raw_os_error()
            .is_some_and(|errno| errno != libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Is the supervisor lease live — status Active, not expired, and owner PID exists.
pub(super) fn is_supervisor_current(lease: &harness_core::TeamSupervisorLease) -> bool {
    // PID liveness is deliberately excluded here: this function gates
    // control-plane decisions (close, reopen, recover-candidate) which
    // must stay on lease expiry+status semantics.  PID-alive check lives
    // in diagnostics (supervisor_lease_live_diagnosis, status output) and
    // in the status warning condition separately.
    lease.status == harness_core::TeamSupervisorLeaseStatus::Active
        && lease.expires_unix_ms > current_unix_ms_u64()
}

/// Returns (is_live, human-readable diagnosis). The diagnosis lists which of the
/// three liveness checks failed, or "live" when all pass.
pub(super) fn supervisor_lease_live_diagnosis(
    lease: &harness_core::TeamSupervisorLease,
) -> (bool, String) {
    let status_active = lease.status == harness_core::TeamSupervisorLeaseStatus::Active;
    let not_expired = lease.expires_unix_ms > current_unix_ms_u64();
    let pid_alive = pid_exists_libc(lease.owner_process_id);
    let live = status_active && not_expired && pid_alive;
    let mut reasons = Vec::new();
    if !status_active {
        reasons.push(format!("status={}", serde_snake_label(&lease.status)));
    }
    if !not_expired {
        reasons.push("expired".to_string());
    }
    if !pid_alive {
        reasons.push(format!("owner PID {} dead", lease.owner_process_id));
    }
    let diagnosis = if reasons.is_empty() {
        "live".to_string()
    } else {
        reasons.join(", ")
    };
    (live, diagnosis)
}

pub(super) fn latest_messages_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<RegistryMessage>> {
    let mut message_ids = Vec::new();
    let mut messages_by_id = BTreeMap::new();
    for message in store.messages()? {
        message_ids.retain(|id| id != &message.id);
        message_ids.push(message.id.clone());
        messages_by_id.insert(message.id.clone(), message);
    }
    Ok(message_ids
        .into_iter()
        .filter_map(|id| messages_by_id.remove(&id))
        .collect())
}

pub(super) fn latest_runtimes(
    store: &HarnessStore,
) -> CliResult<BTreeMap<String, ProviderProcess>> {
    let mut runtimes = BTreeMap::new();
    for runtime in store.runtimes()? {
        runtimes.insert(runtime.id.clone(), runtime);
    }
    Ok(runtimes)
}

pub(super) fn latest_members(
    store: &HarnessStore,
) -> CliResult<BTreeMap<String, ProviderLaunchProfile>> {
    let mut members = BTreeMap::new();
    for member in store.members()? {
        members.insert(member.id.clone(), member);
    }
    Ok(members)
}

pub(super) fn known_agent_member_ids(store: &HarnessStore) -> CliResult<BTreeSet<String>> {
    let mut ids = latest_members(store)?.into_keys().collect::<BTreeSet<_>>();
    ids.extend(
        store
            .all_trust_agent_members()?
            .into_iter()
            .map(|member| member.id),
    );
    Ok(ids)
}

pub(super) fn latest_teams(store: &HarnessStore) -> CliResult<BTreeMap<String, AgentTeam>> {
    let mut teams = BTreeMap::new();
    for team in store.teams()? {
        teams.insert(team.id.clone(), team);
    }
    Ok(teams)
}

/// Integrity annotation attached wherever a pre-cutover dangling
/// TeamRun -> AgentTeam reference is tolerated as a migration fact.
pub(super) const PRE_CUTOVER_DANGLING_TEAM_ANNOTATION: &str = "PRE_CUTOVER_DANGLING_AGENT_TEAM_REF: the TeamRun references an AgentTeam that exists only in the retired legacy teams.jsonl ledger (DOC-108 pre-cutover space); rendered as read-only legacy context, never as current authority";

/// Read-only view over the retired legacy `teams.jsonl` ledger, keyed by team
/// id (latest row wins). Pre-cutover Execution Spaces wrote Team rows here
/// before durable AgentTeams became canonical trust aggregates (DOC-108), so a
/// TeamRun whose `agent_team_id` is absent from the canonical projection but
/// present in this ledger is a migration fact, not corruption. A team id
/// missing from BOTH ledgers is a genuine dangling reference and callers must
/// keep failing closed on it.
pub(super) fn legacy_team_definitions_by_id(
    store: &HarnessStore,
) -> CliResult<BTreeMap<String, serde_json::Value>> {
    let mut teams = BTreeMap::new();
    let contents = match fs::read_to_string(store.root().join("teams.jsonl")) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(teams),
        Err(error) => return Err(error.into()),
    };
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|error| {
            CliError::Usage(format!(
                "legacy teams.jsonl contains an unparseable row: {error}"
            ))
        })?;
        if let Some(id) = value.get("id").and_then(|id| id.as_str()) {
            teams.insert(id.to_string(), value);
        }
    }
    Ok(teams)
}
