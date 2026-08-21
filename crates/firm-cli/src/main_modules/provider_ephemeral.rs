use super::*;

pub(super) struct NdjsonRun {
    pub(super) process_success: bool,
    /// Process exit code when the child exited on its own; `None` when it was
    /// killed on timeout or terminated by a signal (no code available).
    pub(super) exit_code: Option<i32>,
    /// True when the per-node timeout fired and we killed the child.
    pub(super) timed_out: bool,
    /// True when the per-leaf wall-clock timeout fired.
    pub(super) wall_timed_out: bool,
    pub(super) events: Vec<serde_json::Value>,
    pub(super) stderr: String,
    pub(super) warnings: Vec<String>,
}

/// Spawn a child that emits NDJSON on stdout, non-interactively (stdin closed).
/// Events are reduced in memory only; the provider-owned native session remains
/// the sole transcript/tool stream. Enforces a per-node timeout: on
/// timeout the child is killed and `process_success=false` (the run tolerates
/// failed nodes). Returns the terminal [`NdjsonRun`].
/// SIGKILL the worker's whole process GROUP (the child is the group leader, so
/// its pid is the pgid; `kill -9 -<pgid>`). codex/claude spawn child binaries
/// that inherit our stdout pipe — killing only the immediate child would leave a
/// grandchild holding the pipe open and the reader thread (and its join) blocked
/// forever. Falls back to killing the immediate child.
pub(super) fn kill_worker_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        // SIGKILL the whole process GROUP (negative pid == the group). The child is
        // its own group leader (`process_group(0)`), so its pid IS the pgid; a
        // grandchild (codex/claude spawn a child binary; or a test's `sleep`)
        // inherits the group, so this reaps the tree and closes the inherited
        // stdout pipe — which is what lets the reader thread's join return.
        //
        // We call `kill(2)` directly rather than shelling out to `kill -9 -<pgid>`:
        // the external `kill` parses a leading-dash pgid INCONSISTENTLY across
        // platforms (BSD/macOS accept it; util-linux on CI swallowed it as options),
        // which left the grandchild alive and hung the reader for the full 600s.
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Clone, Debug)]
pub(super) struct OrphanRegistration {
    pub(super) dir: PathBuf,
    pub(super) run_id: String,
    pub(super) cmd_marker: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(super) struct OrphanPidfile {
    pub(super) run_id: String,
    pub(super) pid: u32,
    pub(super) pgid: u32,
    pub(super) cmd_marker: String,
    pub(super) started_ms: u128,
}

pub(super) struct OrphanPidfileGuard {
    pub(super) path: PathBuf,
}

impl OrphanPidfileGuard {
    pub(super) fn create(reg: OrphanRegistration, pid: u32) -> CliResult<Self> {
        fs::create_dir_all(&reg.dir)?;
        let path = reg.dir.join(format!("{}__{}.json", reg.run_id, pid));
        let entry = OrphanPidfile {
            run_id: reg.run_id,
            pid,
            // `process_group(0)` makes the child its own group leader, so pid == pgid.
            pgid: pid,
            cmd_marker: reg.cmd_marker,
            started_ms: current_unix_ms(),
        };
        fs::write(&path, serde_json::to_vec(&entry)?)?;
        Ok(Self { path })
    }
}

impl Drop for OrphanPidfileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[allow(clippy::too_many_arguments)] // shared process runner surface plus optional orphan registration
pub(super) fn run_ndjson_child(
    mut cmd: Command,
    session_dir: &Path,
    session_id: &str,
    live_file_name: &str,
    timeout_ms: u64,
    wall_clock_ms: Option<u64>,
    orphan_reg: Option<OrphanRegistration>,
    // Human label for this worker in spawn/timeout error + warning strings
    // (e.g. "ephemeral worker", "codex exec", "claude -p"). The persistent member
    // path passes its provider-specific label so failure summaries read the same
    // as before this runner was shared.
    context: &str,
) -> CliResult<NdjsonRun> {
    // Put the worker in its OWN process group so a timeout can kill the whole
    // tree (see kill_worker_tree), not just the immediate child.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CliError::Usage(format!("failed to spawn {context}: {error}")))?;
    let _orphan_guard = if let Some(reg) = orphan_reg {
        match OrphanPidfileGuard::create(reg, child.id()) {
            Ok(guard) => Some(guard),
            Err(error) => {
                kill_worker_tree(&mut child);
                return Err(error);
            }
        }
    } else {
        None
    };

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::Usage(format!("{context} stdout not available")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CliError::Usage(format!("{context} stderr not available")))?;

    let _ = (session_dir, session_id, live_file_name);

    // IDLE-timeout clock. A productive worker keeps emitting events, each resetting
    // this to "now"; the main thread kills only a worker that has gone SILENT for
    // `timeout_ms` (a wedged provider / auth or network stall) — never a slow but
    // still-streaming turn. Stored as millis-since-`start`.
    let start = Instant::now();
    let last_activity_ms = Arc::new(AtomicU64::new(0));
    let activity_ms = Arc::clone(&last_activity_ms);
    let activity_start = start;

    // Read stdout in a DEDICATED THREAD so the main thread can enforce the idle
    // timeout by KILLING a worker that stops emitting events but never closes stdout
    // (an auth/network stall, a wedged provider). The old code read stdout on the
    // main thread and only checked the deadline AFTER the read loop returned, so a
    // hung worker (stdout still open) blocked forever and froze the whole run. The
    // thread tees each event live + collects them; killing the child closes stdout,
    // which ends this loop.
    let stdout_handle = std::thread::spawn(move || {
        let mut warnings = Vec::new();
        let mut events = Vec::new();
        let mut dropped_lines = 0usize;
        for line in BufReader::new(stdout).lines() {
            let Ok(line_str) = line else { continue };
            let trimmed = line_str.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Any non-empty output proves the worker is alive — reset the idle clock.
            activity_ms.store(
                activity_start.elapsed().as_millis() as u64,
                Ordering::Relaxed,
            );
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                dropped_lines += 1;
                continue;
            };
            events.push(payload);
        }
        if dropped_lines > 0 {
            warnings.push(format!(
                "{dropped_lines} stdout line(s) were not valid JSON and were dropped"
            ));
        }
        (events, warnings)
    });

    // Drain stderr in its own thread so a chatty worker cannot fill the pipe and
    // block (which would also stall the kill path).
    let stderr_handle = std::thread::spawn(move || {
        let mut log = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut log);
        log
    });

    // Main thread: enforce the IDLE timeout. While the worker keeps streaming events
    // the idle clock resets, so a slow-but-productive turn runs to completion however
    // long it takes; only a worker SILENT for `timeout_ms` (a wedged provider, an
    // auth/network stall) is killed. Killing closes stdout/stderr so the reader
    // threads finish and join cleanly.
    let idle_limit = Duration::from_millis(timeout_ms.max(1));
    let wall_clock_limit = wall_clock_ms.map(|ms| Duration::from_millis(ms.max(1)));
    let mut timed_out = false;
    let mut wall_timed_out = false;
    let mut exit_code: Option<i32> = None;
    let process_success = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_code = status.code();
                break status.success();
            }
            Ok(None) => {
                if let Some(wall) = wall_clock_limit {
                    if start.elapsed() > wall {
                        kill_worker_tree(&mut child);
                        wall_timed_out = true;
                        break false;
                    }
                }
                let last = Duration::from_millis(last_activity_ms.load(Ordering::Relaxed));
                if start.elapsed().saturating_sub(last) > idle_limit {
                    kill_worker_tree(&mut child);
                    timed_out = true;
                    break false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break false,
        }
    };

    let (events, mut warnings) = stdout_handle.join().unwrap_or_default();
    let mut stderr_log = stderr_handle.join().unwrap_or_default();
    if timed_out && stderr_log.is_empty() {
        stderr_log = format!("timeout waiting for {context}");
    }
    if wall_timed_out && stderr_log.is_empty() {
        let wall_s = wall_clock_ms.unwrap_or(0).div_ceil(1_000);
        stderr_log = format!("{context} exceeded per-leaf wall-clock timeout of {wall_s}s");
    }
    if timed_out {
        warnings.push(format!("{context} timed out"));
    }
    if wall_timed_out {
        let wall_s = wall_clock_ms.unwrap_or(0).div_ceil(1_000);
        warnings.push(format!(
            "{context} exceeded per-leaf wall-clock timeout of {wall_s}s"
        ));
    }

    Ok(NdjsonRun {
        process_success,
        exit_code,
        timed_out: timed_out || wall_timed_out,
        wall_timed_out,
        events,
        stderr: stderr_log,
        warnings,
    })
}
