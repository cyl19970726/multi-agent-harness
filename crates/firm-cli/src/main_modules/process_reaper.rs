use super::*;


/// Age after which a `Running` WorkflowRun is assumed orphaned and reaped. The
/// run-script path is SYNCHRONOUS — a run is only `Running` in the store while its
/// host process is alive — so a row left `Running` past this age means the process
/// died (crash / Ctrl-C / OOM) before finalizing it. Generous (the longest real
/// runs are ~1.5h) so a legitimately long run is never reaped.
// Age-based backstop for the reaper. The PRIMARY signal is host-pid liveness
// (a killed driver is caught in seconds); this only governs legacy runs that
// carry no `host_pid`, plus the rare pid-reuse false-negative.
pub(super) const REAP_STALE_RUN_AFTER_MS: u128 = 4 * 60 * 60 * 1000; // 4 hours

/// How often the serve-side reaper scans for abandoned runs. A killed driver is
/// reflected on the dashboard within this window.
pub(super) const REAP_POLL_INTERVAL: Duration = Duration::from_secs(30);

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
pub(super) fn supervisor_lease_live_diagnosis(lease: &harness_core::TeamSupervisorLease) -> (bool, String) {
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

pub(super) fn kill_orphan_worker_group(pid: u32, pgid: u32) -> bool {
    #[cfg(unix)]
    {
        if pgid > 0 {
            let rc = unsafe { libc::kill(-(pgid as libc::pid_t), libc::SIGKILL) };
            if rc == 0 {
                return true;
            }
        }
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, pgid);
        false
    }
}

pub(super) fn process_command_for_pid(pid: u32) -> String {
    Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .args(["-o", "command="])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

pub(super) fn process_group_for_pid(pid: u32) -> Option<u32> {
    if pid == 0 {
        return None;
    }
    #[cfg(unix)]
    {
        let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
        (pgid > 0).then_some(pgid as u32)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

pub(super) fn parse_ps_etime_ms(value: &str) -> Option<u128> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (days, rest) = if let Some((days, rest)) = trimmed.split_once('-') {
        (days.trim().parse::<u128>().ok()?, rest)
    } else {
        (0, trimmed)
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let seconds = match parts.as_slice() {
        [seconds] => seconds.trim().parse::<u128>().ok()?,
        [minutes, seconds] => {
            minutes.trim().parse::<u128>().ok()? * 60 + seconds.trim().parse::<u128>().ok()?
        }
        [hours, minutes, seconds] => {
            hours.trim().parse::<u128>().ok()? * 60 * 60
                + minutes.trim().parse::<u128>().ok()? * 60
                + seconds.trim().parse::<u128>().ok()?
        }
        _ => return None,
    };
    Some((days * 24 * 60 * 60 + seconds) * 1_000)
}

pub(super) fn process_elapsed_ms_for_pid(pid: u32) -> Option<u128> {
    let output = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .args(["-o", "etime="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ps_etime_ms(&String::from_utf8_lossy(&output.stdout))
}

// `ps etime` is only second-resolution and often rounds down, so a just-spawned
// original worker can appear to have started slightly after the pidfile write.
pub(super) const PID_START_TOLERANCE_MS: u128 = 2_000;

pub(super) fn process_identity_matches_pidfile(pidfile: &OrphanPidfile, command: &str) -> bool {
    if pidfile.cmd_marker.is_empty() || !command.contains(&pidfile.cmd_marker) {
        return false;
    }
    if process_group_for_pid(pidfile.pid) != Some(pidfile.pgid) {
        return false;
    }
    let Some(elapsed_ms) = process_elapsed_ms_for_pid(pidfile.pid) else {
        return false;
    };
    let inferred_start_ms = current_unix_ms().saturating_sub(elapsed_ms);
    inferred_start_ms <= pidfile.started_ms.saturating_add(PID_START_TOLERANCE_MS)
}

pub(super) fn worker_pid_dir(store: &HarnessStore) -> PathBuf {
    store.root().join("worker_pids")
}

pub(super) fn reap_orphaned_workers(store: &HarnessStore, dry_run: bool) -> CliResult<serde_json::Value> {
    let dir = worker_pid_dir(store);
    let mut scanned = 0usize;
    let mut killed = 0usize;
    let mut already_dead = 0usize;
    let mut skipped_pid_reuse = 0usize;
    let mut kept_running = 0usize;
    let mut entries = Vec::new();

    let runs: BTreeMap<String, WorkflowRun> = latest_workflow_runs_in_append_order(store)?
        .into_iter()
        .map(|run| (run.id.clone(), run))
        .collect();

    let read_dir = match fs::read_dir(&dir) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::json!({
                "scanned": 0,
                "killed": 0,
                "already_dead": 0,
                "skipped_pid_reuse": 0,
                "kept_running": 0,
                "dry_run": dry_run,
                "entries": [],
            }))
        }
        Err(error) => return Err(error.into()),
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        scanned += 1;
        let path_display = path.display().to_string();
        let pidfile = match fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<OrphanPidfile>(&content).ok())
        {
            Some(pidfile) => pidfile,
            None => {
                if !dry_run {
                    let _ = fs::remove_file(&path);
                }
                entries.push(serde_json::json!({
                    "path": path_display,
                    "action": if dry_run { "would_remove_invalid_pidfile" } else { "removed_invalid_pidfile" },
                }));
                continue;
            }
        };

        let owner = runs.get(&pidfile.run_id);
        let owner_running = owner.is_some_and(|run| run.status == WorkflowRunStatus::Running);
        let owner_host_alive = owner
            .and_then(|run| run.host_pid)
            .is_some_and(pid_exists_libc);
        if owner_running && owner_host_alive {
            kept_running += 1;
            entries.push(serde_json::json!({
                "path": path_display,
                "run_id": pidfile.run_id,
                "pid": pidfile.pid,
                "pgid": pidfile.pgid,
                "cmd_marker": pidfile.cmd_marker,
                "action": "kept_running",
            }));
            continue;
        }

        if !pid_exists_libc(pidfile.pid) {
            already_dead += 1;
            if !dry_run {
                let _ = fs::remove_file(&path);
            }
            entries.push(serde_json::json!({
                "path": path_display,
                "run_id": pidfile.run_id,
                "pid": pidfile.pid,
                "pgid": pidfile.pgid,
                "cmd_marker": pidfile.cmd_marker,
                "action": if dry_run { "would_remove_already_dead" } else { "already_dead" },
            }));
            continue;
        }

        let command = process_command_for_pid(pidfile.pid);
        if !process_identity_matches_pidfile(&pidfile, &command) {
            skipped_pid_reuse += 1;
            if !dry_run {
                let _ = fs::remove_file(&path);
            }
            entries.push(serde_json::json!({
                "path": path_display,
                "run_id": pidfile.run_id,
                "pid": pidfile.pid,
                "pgid": pidfile.pgid,
                "cmd_marker": pidfile.cmd_marker,
                "command": command,
                "current_pgid": process_group_for_pid(pidfile.pid),
                "action": if dry_run { "would_skip_pid_reuse" } else { "skipped_pid_reuse" },
            }));
            continue;
        }

        let killed_worker = dry_run || kill_orphan_worker_group(pidfile.pid, pidfile.pgid);
        if killed_worker {
            killed += 1;
            if !dry_run {
                let _ = fs::remove_file(&path);
            }
        }
        entries.push(serde_json::json!({
            "path": path_display,
            "run_id": pidfile.run_id,
            "pid": pidfile.pid,
            "pgid": pidfile.pgid,
            "cmd_marker": pidfile.cmd_marker,
            "command": command,
            "action": match (dry_run, killed_worker) {
                (true, _) => "would_kill",
                (false, true) => "killed",
                (false, false) => "kill_failed",
            },
        }));
    }

    Ok(serde_json::json!({
        "scanned": scanned,
        "killed": killed,
        "already_dead": already_dead,
        "skipped_pid_reuse": skipped_pid_reuse,
        "kept_running": kept_running,
        "dry_run": dry_run,
        "entries": entries,
    }))
}

/// Finalize ABANDONED `Running` workflow runs to `Failed`, so a crashed / killed
/// driver does not sit `Running` forever in the store / snapshot / dashboard.
///
/// A run is abandoned when EITHER:
///   - its `host_pid` is recorded and that process is no longer alive on this
///     host (driver killed / crashed / Ctrl-C'd) — caught within one poll,
///     regardless of age; OR
///   - it has been `Running` longer than [`REAP_STALE_RUN_AFTER_MS`] — the age
///     backstop covering legacy rows with no `host_pid` (and pid reuse).
///
/// Reaping a run also flips its still-open (`running`/`queued`) steps to `failed`
/// so the per-step view is not frozen mid-flight after the run itself fails. The
/// appended terminal rows are picked up and broadcast by the SSE watcher, so a
/// live dashboard updates without a refetch. Best-effort; returns the count of
/// runs reaped. Same-host only — `host_pid` liveness is meaningless across hosts.
pub(super) fn reap_stale_workflow_runs(store: &HarnessStore) -> CliResult<usize> {
    let now = current_unix_ms();
    // Group the latest step rows by run so a reaped run's open steps close too.
    let mut steps_by_run: BTreeMap<String, Vec<WorkflowStep>> = BTreeMap::new();
    for step in latest_workflow_steps_in_append_order(store)? {
        steps_by_run
            .entry(step.run_id.clone())
            .or_default()
            .push(step);
    }
    let mut reaped = 0;
    for mut run in latest_workflow_runs_in_append_order(store)? {
        if run.status != WorkflowRunStatus::Running {
            continue;
        }
        let age = now.saturating_sub(created_ms(&run.created_at));
        let pid_dead = run.host_pid.map(|pid| !pid_is_alive(pid)).unwrap_or(false);
        let too_old = age >= REAP_STALE_RUN_AFTER_MS;
        if !pid_dead && !too_old {
            continue;
        }
        // Close any non-terminal steps so the dashboard's per-step status is not
        // stuck at `running` after the run itself is failed.
        if let Some(steps) = steps_by_run.get(&run.id) {
            for step in steps {
                if !matches!(
                    step.status,
                    WorkflowStepStatus::Running | WorkflowStepStatus::Queued
                ) {
                    continue;
                }
                let mut closed = step.clone();
                let had_partial = closed.result.is_some()
                    || closed
                        .output_summary
                        .as_deref()
                        .is_some_and(|summary| !summary.is_empty());
                closed.status = WorkflowStepStatus::Failed;
                closed.ended_at = Some(now_string());
                closed.output_summary = Some(match closed.output_summary.as_deref() {
                    Some(s) if !s.is_empty() => format!("{s} [reaped: driver process gone]"),
                    _ => "reaped: driver process gone".to_string(),
                });
                closed.terminal_reason = Some(WorkflowTerminalReason::DriverExited);
                closed.partial = had_partial;
                store.append_workflow_step(&closed)?;
            }
        }
        run.status = WorkflowRunStatus::Failed;
        run.ended_at = Some(now_string());
        run.summary = Some(match run.host_pid {
            Some(pid) if pid_dead => format!(
                "reaped: driver process (pid {pid}) is no longer alive — the run was abandoned before it finalized"
            ),
            _ => format!(
                "reaped: orphaned Running for ~{}h — host process exited before the run finalized",
                age / (60 * 60 * 1000)
            ),
        });
        run.terminal_reason = Some(WorkflowTerminalReason::DriverExited);
        run.partial_output_available = steps_by_run.get(&run.id).is_some_and(|steps| {
            steps.iter().any(|step| {
                matches!(
                    step.status,
                    WorkflowStepStatus::Completed | WorkflowStepStatus::Cached
                ) || step.result.is_some()
            })
        });
        store.append_workflow_run(&run)?;
        // A crashed/abandoned run reaching its terminal Failed status also notifies
        // the completion hook (no-op unless HARNESS_WORKFLOW_ON_COMPLETE is set), so
        // a run whose owner died before finalizing still signals completion.
        fire_workflow_completion_hook(&run);
        reaped += 1;
    }
    Ok(reaped)
}

pub(super) fn latest_workflow_steps_in_append_order(store: &HarnessStore) -> CliResult<Vec<WorkflowStep>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for step in store.workflow_steps()? {
        ids.retain(|id| id != &step.id);
        ids.push(step.id.clone());
        by_id.insert(step.id.clone(), step);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

pub(super) fn latest_messages_in_append_order(store: &HarnessStore) -> CliResult<Vec<RegistryMessage>> {
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

pub(super) fn latest_runtimes(store: &HarnessStore) -> CliResult<BTreeMap<String, ProviderProcess>> {
    let mut runtimes = BTreeMap::new();
    for runtime in store.runtimes()? {
        runtimes.insert(runtime.id.clone(), runtime);
    }
    Ok(runtimes)
}

pub(super) fn latest_members(store: &HarnessStore) -> CliResult<BTreeMap<String, ProviderLaunchProfile>> {
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
