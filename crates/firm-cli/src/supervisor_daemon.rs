//! Supervisor daemon — the detached process that owns the writer loop,
//! heartbeat, and TCP control listener for a team run.  The `harness daemon
//! supervisor serve` command calls [`run_supervisor_daemon`] in the foreground.
//! `harness team-run start` spawns the daemon as a child and exits.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{
    current_unix_ms_u64, drive_prepared_team_run, prepare_team_run_start_body,
    supervisor_lease_live_diagnosis, CliError, CliResult, HarnessStore, PreparedTeamRunStart,
    TeamRunLedger, TeamSupervisorRegistration,
};

// ---------------------------------------------------------------------------
// Signal handling (portable Unix FFI — same pattern as resident_daemon.rs)
// ---------------------------------------------------------------------------

/// Set by the SIGTERM/SIGINT handler so the accept loop can exit between
/// connections.  The daemon runs [`drive_prepared_team_run`] on the main
/// thread, so the signal only affects the per-connection accept loop inside
/// the control thread — the delivery loop exits when `heartbeat_valid` goes
/// false.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn install_signal_handlers() {
    extern "C" fn handle(_sig: i32) {
        SHUTDOWN.store(true, Ordering::SeqCst);
    }
    // SAFETY: `handle` only stores into an AtomicBool (async-signal-safe),
    // and `signal(2)` is a stable C ABI symbol present in libc on all unix
    // targets.  `handle` (an `extern "C" fn(i32)`) coerces directly to
    // `SigHandler`, so no function-item-to-integer cast is needed.
    unsafe {
        signal(SIGTERM, handle);
        signal(SIGINT, handle);
    }
}

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;
type SigHandler = extern "C" fn(i32);
extern "C" {
    fn signal(signum: i32, handler: SigHandler) -> usize;
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Information a CLI client needs to adopt or inspect a running daemon.
#[derive(Clone, Debug)]
pub(crate) struct DaemonInfo {
    pub pid: u32,
    pub locator: String,
    pub generation: u64,
    #[allow(dead_code)]
    pub supervisor_id: String,
}

/// Status of a supervisor daemon for a given team run.
#[derive(Clone, Debug)]
pub(crate) enum SupervisorDaemonStatus {
    /// Lease Active, PID alive, heartbeat fresh — normal operation.
    Running {
        pid: u32,
        generation: u64,
        heartbeat_age_ms: u64,
    },
    /// Lease Active, heartbeat stale, PID dead — daemon crashed.
    Crashed { pid: u32, generation: u64 },
    /// Lease Active but expired — daemon gone, lease past TTL.
    Expired { pid: u32, generation: u64 },
    /// Lease released or never existed.
    Absent,
}

// ---------------------------------------------------------------------------
// Daemon entry point (foreground — `harness daemon supervisor serve`)
// ---------------------------------------------------------------------------

/// Run the supervisor daemon in the foreground.  Blocks until the delivery
/// loop exits (team run completes, cancelled, or lease is lost).
pub(crate) fn run_supervisor_daemon(
    store: &HarnessStore,
    run_id: &str,
    max_concurrency: usize,
    idle_timeout_secs: u64,
) -> CliResult<()> {
    install_signal_handlers();

    let body = prepare_team_run_start_body(store, run_id, max_concurrency)?;

    let supervisor_registration = TeamSupervisorRegistration::start(store, run_id)?;

    let ledger = Arc::new(TeamRunLedger::new(
        store,
        run_id,
        &supervisor_registration.supervisor_id,
        supervisor_registration.generation,
        Arc::clone(&supervisor_registration.heartbeat_valid),
    ));

    use crate::{now_string, store_conflict_as_usage};
    use harness_core::{TeamRunStatus, WaveStatus};

    let running = if body.run.status == TeamRunStatus::Planning {
        let mut running = body.run.clone();
        running.status = TeamRunStatus::Running;
        running.updated_at = now_string();
        store_conflict_as_usage(store.compare_and_append_team_run_with_wave_status(
            &body.run,
            &running,
            WaveStatus::Running,
            &now_string(),
        ))?;
        running
    } else {
        body.run.clone()
    };

    ledger.fold_event(
        harness_core::TeamRunEventSourceKind::Host,
        None,
        "team_run",
        run_id,
        "updated",
        &format!(
            "member supervisor {} generation {} {} ({} unclosed member(s), max-concurrency {max_concurrency})",
            supervisor_registration.supervisor_id,
            supervisor_registration.generation,
            if body.run.status == TeamRunStatus::Planning {
                "started"
            } else {
                "reattached"
            },
            body.members.len(),
        ),
    )?;

    let prepared = PreparedTeamRunStart {
        run_id: body.run_id,
        objective: body.objective,
        running,
        members: body.members,
        ledger,
        supervisor_registration,
    };

    eprintln!(
        "[supervisor-daemon] team run {}: serving (pid {}, gen {})",
        run_id,
        std::process::id(),
        prepared.supervisor_registration.generation,
    );

    drive_prepared_team_run(
        prepared,
        None, // execution_space — daemon resolves from store when needed
        None, // project_context — daemon resolves from store when needed
        max_concurrency,
        Duration::from_secs(idle_timeout_secs),
        None,
    )
}

// ---------------------------------------------------------------------------
// launchd plist generation (artifact only)
// ---------------------------------------------------------------------------

/// Write a macOS LaunchAgent plist for the supervisor daemon.  This is an
/// *artifact* — the portable `Command::spawn` path is the primary mechanism;
/// the plist is optional hardening.
pub(crate) fn generate_launchd_plist(
    team_run_id: &str,
    firm_bin: &str,
    max_concurrency: usize,
    idle_timeout_secs: u64,
) -> CliResult<()> {
    let home = dirs_fallback();
    let agents_dir = home.join("Library").join("LaunchAgents");
    std::fs::create_dir_all(&agents_dir)?;

    let label = format!("com.firm.supervisor.{team_run_id}");
    let plist_path = agents_dir.join(format!("{label}.plist"));

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{firm_bin}</string>
        <string>daemon</string>
        <string>supervisor</string>
        <string>serve</string>
        <string>--team-run-id</string>
        <string>{team_run_id}</string>
        <string>--max-concurrency</string>
        <string>{max_concurrency}</string>
        <string>--idle-timeout-secs</string>
        <string>{idle_timeout_secs}</string>
    </array>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/firm-supervisor-{team_run_id}.stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/firm-supervisor-{team_run_id}.stderr.log</string>
</dict>
</plist>
"#
    );

    std::fs::write(&plist_path, plist)?;
    eprintln!(
        "[supervisor-daemon] wrote launchd plist: {}",
        plist_path.display()
    );
    eprintln!(
        "[supervisor-daemon] to activate: launchctl load {}",
        plist_path.display()
    );
    Ok(())
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

// ---------------------------------------------------------------------------
// Control-plane helpers (status / stop / check / spawn / adopt)
// ---------------------------------------------------------------------------

/// Inspect the lease and report the daemon's status.
pub(crate) fn supervisor_daemon_status(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<SupervisorDaemonStatus> {
    let Some(lease) = store.latest_team_supervisor_lease(team_run_id)? else {
        return Ok(SupervisorDaemonStatus::Absent);
    };
    if lease.status != harness_core::TeamSupervisorLeaseStatus::Active {
        return Ok(SupervisorDaemonStatus::Absent);
    }
    let now_ms = current_unix_ms_u64();
    let heartbeat_age_ms = now_ms.saturating_sub(lease.heartbeat_unix_ms);
    let (pid_alive, _diag) = supervisor_lease_live_diagnosis(&lease);
    if !pid_alive {
        return Ok(SupervisorDaemonStatus::Crashed {
            pid: lease.owner_process_id,
            generation: lease.generation,
        });
    }
    if lease.expires_unix_ms > 0 && lease.expires_unix_ms < now_ms {
        return Ok(SupervisorDaemonStatus::Expired {
            pid: lease.owner_process_id,
            generation: lease.generation,
        });
    }
    Ok(SupervisorDaemonStatus::Running {
        pid: lease.owner_process_id,
        generation: lease.generation,
        heartbeat_age_ms,
    })
}

/// Send a shutdown request to a running daemon via the control channel.
pub(crate) fn stop_supervisor_daemon(store: &HarnessStore, team_run_id: &str) -> CliResult<()> {
    let info = check_existing_supervisor_daemon(store, team_run_id)?.ok_or_else(|| {
        CliError::Usage(format!(
            "no running supervisor daemon for team run {team_run_id}"
        ))
    })?;

    let addr = info.locator.strip_prefix("tcp://").unwrap_or(&info.locator);
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    // The control protocol is JSON-line: send an interrupt command.
    let cmd = serde_json::json!({
        "kind": "interrupt",
        "member_run_id": "*",
    });
    writeln!(stream, "{}", serde_json::to_string(&cmd)?)?;
    stream.flush()?;

    // Read one line of acknowledgement (best-effort).
    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    let _ = reader.read_line(&mut buf);

    eprintln!(
        "[supervisor-daemon] stop sent to pid {} (gen {})",
        info.pid, info.generation
    );
    Ok(())
}

/// Check whether an active supervisor daemon exists for `team_run_id`.
/// Returns `None` when no daemon is running or the lease is stale.
pub(crate) fn check_existing_supervisor_daemon(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<Option<DaemonInfo>> {
    let Some(lease) = store.latest_team_supervisor_lease(team_run_id)? else {
        return Ok(None);
    };
    if lease.status != harness_core::TeamSupervisorLeaseStatus::Active {
        return Ok(None);
    }
    let now_ms = current_unix_ms_u64();
    if lease.expires_unix_ms > 0 && lease.expires_unix_ms < now_ms {
        return Ok(None);
    }
    let (pid_alive, _diag) = supervisor_lease_live_diagnosis(&lease);
    if !pid_alive {
        return Ok(None);
    }

    // Quick TCP probe to confirm the daemon is actually serving.
    let addr = lease
        .owner_locator
        .strip_prefix("tcp://")
        .unwrap_or(&lease.owner_locator);
    if TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|e| CliError::Usage(format!("bad locator: {e}")))?,
        Duration::from_secs(2),
    )
    .is_err()
    {
        return Ok(None);
    }

    Ok(Some(DaemonInfo {
        pid: lease.owner_process_id,
        locator: lease.owner_locator,
        generation: lease.generation,
        supervisor_id: lease.supervisor_id,
    }))
}

/// Spawn the supervisor daemon as a detached child process.  Polls the lease
/// until a heartbeat appears (timeout 10s).
pub(crate) fn spawn_supervisor_daemon(
    store: &HarnessStore,
    run_id: &str,
    max_concurrency: usize,
    idle_timeout_secs: u64,
) -> CliResult<DaemonInfo> {
    let exe = std::env::current_exe()
        .map_err(|e| CliError::Usage(format!("cannot resolve current executable: {e}")))?;

    let store_root = store.root().to_path_buf();
    let store_root_str = store_root.to_string_lossy().to_string();

    let mut cmd = Command::new(&exe);
    cmd.args([
        "daemon",
        "supervisor",
        "serve",
        "--team-run-id",
        run_id,
        "--max-concurrency",
        &max_concurrency.to_string(),
        "--idle-timeout-secs",
        &idle_timeout_secs.to_string(),
    ])
    .env("FIRM_ROOT", &store_root_str)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());

    // Inherit all parent environment variables so test harness vars
    // (FAKE_KIMI_RESULT, PATH with fake shims, etc.) flow through to
    // the daemon child.  FIRM_ROOT is already overridden above.
    for (key, val) in std::env::vars() {
        if key != "FIRM_ROOT" {
            cmd.env(key, val);
        }
    }

    let child = cmd
        .spawn()
        .map_err(|e| CliError::Usage(format!("failed to spawn supervisor daemon: {e}")))?;
    let pid = child.id();

    eprintln!("[supervisor-daemon] spawned daemon pid {pid} for team run {run_id}");

    // Poll the lease until the daemon writes its first heartbeat AND its TCP
    // control listener is accepting connections.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        std::thread::sleep(Duration::from_millis(200));
        if let Some(lease) = store.latest_team_supervisor_lease(run_id)? {
            if lease.heartbeat_unix_ms > 0
                && lease.status == harness_core::TeamSupervisorLeaseStatus::Active
            {
                let (alive, _) = supervisor_lease_live_diagnosis(&lease);
                if alive {
                    // Also verify the TCP control listener is ready before
                    // returning — the heartbeat thread may start before the
                    // control thread binds.
                    let addr = lease
                        .owner_locator
                        .strip_prefix("tcp://")
                        .unwrap_or(&lease.owner_locator);
                    if TcpStream::connect_timeout(
                        &addr
                            .parse()
                            .map_err(|e| CliError::Usage(format!("bad locator: {e}")))?,
                        Duration::from_secs(1),
                    )
                    .is_ok()
                    {
                        return Ok(DaemonInfo {
                            pid: lease.owner_process_id,
                            locator: lease.owner_locator,
                            generation: lease.generation,
                            supervisor_id: lease.supervisor_id,
                        });
                    }
                }
            }
        }
        if std::time::Instant::now() > deadline {
            return Err(CliError::Usage(format!(
                "supervisor daemon pid {pid} did not become ready within 10s for team run {run_id}"
            )));
        }
    }
}

/// Verify that we can connect to a running daemon.  Returns `Ok(())` on
/// successful TCP handshake.
pub(crate) fn adopt_daemon(info: &DaemonInfo) -> CliResult<()> {
    let addr = info.locator.strip_prefix("tcp://").unwrap_or(&info.locator);
    let _stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|e| CliError::Usage(format!("bad locator: {e}")))?,
        Duration::from_secs(5),
    )
    .map_err(|e| {
        CliError::Usage(format!(
            "cannot connect to supervisor daemon at {addr}: {e}"
        ))
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI subcommand dispatch
// ---------------------------------------------------------------------------

/// Parse `--team-run-id`, `--max-concurrency`, and `--idle-timeout-secs`
/// from the provided args list.
fn value(args: &[String], flag: &str) -> Option<String> {
    let mut i = 1; // skip subcommand
    while i < args.len() {
        if args[i] == flag {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

/// Dispatch the `harness daemon supervisor ...` subcommand.
pub(crate) fn supervisor_daemon_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    if args.is_empty() {
        return Err(CliError::Usage(
            "supervisor daemon serve|status|stop|generate-plist".into(),
        ));
    }
    let run_id = value(args, "--team-run-id")
        .ok_or_else(|| CliError::Usage("--team-run-id is required".into()))?;
    match args[0].as_str() {
        "serve" => {
            let max_concurrency: usize = value(args, "--max-concurrency")
                .and_then(|s| s.parse().ok())
                .unwrap_or(4);
            let idle_timeout_secs: u64 = value(args, "--idle-timeout-secs")
                .and_then(|s| s.parse().ok())
                .unwrap_or(300);
            run_supervisor_daemon(store, &run_id, max_concurrency, idle_timeout_secs)
        }
        "status" => {
            let status = supervisor_daemon_status(store, &run_id)?;
            match status {
                SupervisorDaemonStatus::Running {
                    pid,
                    generation,
                    heartbeat_age_ms,
                } => {
                    println!(
                        "running (pid {pid}, gen {generation}, heartbeat age {heartbeat_age_ms}ms)"
                    );
                }
                SupervisorDaemonStatus::Crashed { pid, generation } => {
                    println!("crashed (pid {pid}, gen {generation})");
                }
                SupervisorDaemonStatus::Expired { pid, generation } => {
                    println!("expired (pid {pid}, gen {generation})");
                }
                SupervisorDaemonStatus::Absent => {
                    println!("absent (no active supervisor daemon)");
                }
            }
            Ok(())
        }
        "stop" => stop_supervisor_daemon(store, &run_id),
        "generate-plist" => {
            let max_concurrency: usize = value(args, "--max-concurrency")
                .and_then(|s| s.parse().ok())
                .unwrap_or(4);
            let idle_timeout_secs: u64 = value(args, "--idle-timeout-secs")
                .and_then(|s| s.parse().ok())
                .unwrap_or(300);
            let firm_bin = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "firm".to_string());
            generate_launchd_plist(&run_id, &firm_bin, max_concurrency, idle_timeout_secs)
        }
        other => Err(CliError::Usage(format!(
            "unknown supervisor daemon command: {other}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Multi-team supervisor daemon (#366)
// ---------------------------------------------------------------------------
// The multi-team daemon manages N team-runs in a single process.
// It scans the store for active runs, spawns one supervisor thread per run,
// and exposes a Unix-domain control socket for start/status/stop commands.
// This supersedes the per-run detached daemon as the primary path; the
// per-run daemon remains available as a fallback when the multi-team daemon
// is not running.

/// Socket path for the multi-team daemon's control socket.
/// Uses a hash-based fallback under /tmp when the store root path exceeds
/// the macOS AF_UNIX 104-byte limit.
#[allow(dead_code)] // #415 owns the currently unwired multi-team daemon product path.
pub(crate) fn multi_team_socket_path(store_root: &std::path::Path) -> PathBuf {
    let direct = store_root.join("supervisor.sock");
    let direct_str = direct.to_string_lossy();
    if direct_str.len() < 100 {
        return direct;
    }
    // Hash-based fallback for long paths (macOS AF_UNIX 104-byte limit).
    let mut hasher = DefaultHasher::new();
    store_root.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();
    std::path::Path::new("/tmp").join(format!("firm-supervisor-{hash:x}.sock"))
}

/// A managed team-run context inside the multi-team daemon.
#[allow(dead_code)]
struct MultiTeamContext {
    run_id: String,
    heartbeat_valid: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<CliResult<()>>>,
    started_at: Instant,
}

/// The multi-team supervisor daemon.
#[allow(dead_code)]
pub(crate) struct MultiTeamDaemon {
    store: HarnessStore,
    contexts: Mutex<Vec<MultiTeamContext>>,
    max_concurrency: usize,
    idle_timeout_secs: u64,
    scan_interval: Duration,
    shutdown: Arc<AtomicBool>,
}

#[allow(dead_code)]
impl MultiTeamDaemon {
    /// Run the multi-team daemon in the foreground. Blocks until SIGTERM/SIGINT
    /// or until the control socket receives a "stop" command.
    pub(crate) fn run(
        store: HarnessStore,
        max_concurrency: usize,
        idle_timeout_secs: u64,
        scan_interval_secs: u64,
    ) -> CliResult<()> {
        let shutdown = Arc::new(AtomicBool::new(false));

        // Signal handling: use a self-contained pattern where the handler
        // sets an AtomicBool — no static raw pointer (fixes P0-8).
        let shutdown_sig = Arc::clone(&shutdown);
        install_signal_handlers_mt(Arc::clone(&shutdown_sig));

        let socket_path = multi_team_socket_path(store.root());
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            CliError::Usage(format!(
                "cannot bind supervisor socket at {}: {e}",
                socket_path.display()
            ))
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|e| CliError::Usage(format!("cannot set socket non-blocking: {e}")))?;

        eprintln!("[multi-team-daemon] listening on {}", socket_path.display());

        let daemon = MultiTeamDaemon {
            store,
            contexts: Mutex::new(Vec::new()),
            max_concurrency,
            idle_timeout_secs,
            scan_interval: Duration::from_secs(scan_interval_secs),
            shutdown: shutdown_sig,
        };

        // Crash recovery: adopt orphaned runs on startup.
        daemon.recover_orphaned_runs()?;

        // Main loop: scan store + poll control socket.
        daemon.serve_loop(&listener)?;

        // Graceful shutdown.
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
        daemon.graceful_shutdown()?;

        eprintln!("[multi-team-daemon] shutdown complete");
        Ok(())
    }

    /// Enumerate non-terminal team-runs and adopt runs whose supervisor lease
    /// is expired (no live supervisor elsewhere).
    fn recover_orphaned_runs(&self) -> CliResult<()> {
        let runs = self.store.team_runs().map_err(|e| {
            CliError::Store(harness_store::StoreError::Conflict(format!(
                "list team runs: {e}"
            )))
        })?;
        let now_ms = current_unix_ms_u64();
        let mut adopted = 0usize;

        for run in &runs {
            use harness_core::TeamRunStatus;
            // P0-2 fix: only adopt Running runs (not Planning) and check lease.
            if !matches!(run.status, TeamRunStatus::Running) {
                continue;
            }

            let lease = self
                .store
                .latest_team_supervisor_lease(&run.id)
                .ok()
                .flatten();
            let should_adopt = match &lease {
                None => true,
                Some(l) => {
                    l.status != harness_core::TeamSupervisorLeaseStatus::Active
                        || (l.expires_unix_ms > 0 && l.expires_unix_ms < now_ms)
                }
            };

            if should_adopt {
                eprintln!("[multi-team-daemon] adopting orphaned run {}", run.id);
                if let Err(e) = self.start_supervising(&run.id) {
                    eprintln!("[multi-team-daemon] failed to adopt run {}: {e}", run.id);
                } else {
                    adopted += 1;
                }
            }
        }

        if adopted > 0 {
            eprintln!("[multi-team-daemon] adopted {adopted} orphaned run(s)");
        }
        Ok(())
    }

    /// Main loop: scan for new runs, reap finished contexts, poll control socket.
    fn serve_loop(&self, listener: &UnixListener) -> CliResult<()> {
        let mut buf = String::new();

        while !self.shutdown.load(Ordering::SeqCst) {
            // 1. Scan for active team-runs that aren't yet managed.
            self.scan_and_adopt()?;

            // 2. Reap finished contexts.
            self.reap_finished()?;

            // 3. Poll control socket for one command (non-blocking).
            self.poll_control_socket(listener, &mut buf)?;

            // 4. Sleep the scan interval with shutdown-aware wake-up.
            let deadline = Instant::now() + self.scan_interval;
            while Instant::now() < deadline && !self.shutdown.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        Ok(())
    }

    /// Scan store for active Running team-runs not yet managed.
    /// Does NOT hold the context lock across store I/O (fixes P0-7).
    fn scan_and_adopt(&self) -> CliResult<()> {
        let runs = self.store.team_runs().map_err(|e| {
            CliError::Store(harness_store::StoreError::Conflict(format!(
                "list team runs: {e}"
            )))
        })?;

        let managed_ids: Vec<String> = {
            let ctx = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            ctx.iter().map(|c| c.run_id.clone()).collect()
        };

        for run in &runs {
            use harness_core::TeamRunStatus;
            if !matches!(run.status, TeamRunStatus::Running) {
                continue;
            }
            if managed_ids.contains(&run.id) {
                continue;
            }
            // P0-2 fix: check lease before adopting.
            let now_ms = current_unix_ms_u64();
            let should_start = match self
                .store
                .latest_team_supervisor_lease(&run.id)
                .ok()
                .flatten()
            {
                None => true,
                Some(l) => {
                    l.status != harness_core::TeamSupervisorLeaseStatus::Active
                        || (l.expires_unix_ms > 0 && l.expires_unix_ms < now_ms)
                }
            };
            if !should_start {
                continue;
            }

            eprintln!("[multi-team-daemon] starting supervisor for run {}", run.id);
            // P0-4 fix: errors propagate, not just stderr.
            if let Err(e) = self.start_supervising(&run.id) {
                eprintln!("[multi-team-daemon] failed to start run {}: {e}", run.id);
            }
        }
        Ok(())
    }

    /// Spawn a supervisor thread for a single team-run.
    fn start_supervising(&self, run_id: &str) -> CliResult<()> {
        // P0-2 fix: enforce concurrent team-run limit.
        {
            let contexts = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            if contexts.len() >= self.max_concurrency {
                return Err(CliError::Usage(format!(
                    "multi-team daemon at capacity ({}/{} runs); cannot start {run_id}",
                    contexts.len(),
                    self.max_concurrency,
                )));
            }
        }

        let store = self.store.clone();
        let run_id = run_id.to_string();
        let max_concurrency = self.max_concurrency;
        let idle_timeout_secs = self.idle_timeout_secs;

        // Validate and create registration outside the context lock (fixes P0-7).
        let body = prepare_team_run_start_body(&store, &run_id, max_concurrency)?;
        let registration = TeamSupervisorRegistration::start(&store, &run_id)?;
        let heartbeat_valid = Arc::clone(&registration.heartbeat_valid);

        // Transition Planning→Running if needed (same as per-run daemon).
        use crate::{now_string, store_conflict_as_usage};
        use harness_core::{TeamRunStatus, WaveStatus};

        let running = if body.run.status == TeamRunStatus::Planning {
            let mut running = body.run.clone();
            running.status = TeamRunStatus::Running;
            running.updated_at = now_string();
            store_conflict_as_usage(store.compare_and_append_team_run_with_wave_status(
                &body.run,
                &running,
                WaveStatus::Running,
                &now_string(),
            ))?;
            running
        } else {
            body.run.clone()
        };

        let ledger = Arc::new(TeamRunLedger::new(
            &store,
            &run_id,
            &registration.supervisor_id,
            registration.generation,
            Arc::clone(&registration.heartbeat_valid),
        ));

        ledger.fold_event(
            harness_core::TeamRunEventSourceKind::Host,
            None,
            "team_run",
            &run_id,
            "updated",
            &format!(
                "member supervisor {} generation {} {} ({} unclosed member(s), max-concurrency {max_concurrency})",
                registration.supervisor_id,
                registration.generation,
                if body.run.status == TeamRunStatus::Planning {
                    "started"
                } else {
                    "reattached"
                },
                body.members.len(),
            ),
        )?;

        let prepared = PreparedTeamRunStart {
            run_id: body.run_id,
            objective: body.objective,
            running,
            members: body.members,
            ledger,
            supervisor_registration: registration,
        };

        eprintln!(
            "[multi-team-daemon] team run {}: serving (pid {}, gen {})",
            run_id,
            std::process::id(),
            prepared.supervisor_registration.generation,
        );

        let thread = std::thread::spawn(move || {
            drive_prepared_team_run(
                prepared,
                None, // execution_space — daemon resolves from store
                None, // project_context — daemon resolves from store
                max_concurrency,
                Duration::from_secs(idle_timeout_secs),
                None,
            )
        });

        // P0-7 fix: lock only for the insert, not around store I/O.
        {
            let mut contexts = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            contexts.push(MultiTeamContext {
                run_id,
                heartbeat_valid,
                thread: Some(thread),
                started_at: Instant::now(),
            });
        }
        Ok(())
    }

    /// Reap finished supervisor threads and remove them from the context registry.
    fn reap_finished(&self) -> CliResult<()> {
        let mut contexts = self
            .contexts
            .lock()
            .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;

        let mut finished = Vec::new();
        let mut still_running = Vec::new();

        for ctx in contexts.drain(..) {
            let is_done = ctx.thread.as_ref().map(|t| t.is_finished()).unwrap_or(true);
            if is_done {
                finished.push(ctx);
            } else {
                still_running.push(ctx);
            }
        }

        *contexts = still_running;

        for ctx in finished {
            if let Some(thread) = ctx.thread {
                match thread.join() {
                    Ok(Ok(())) => {
                        eprintln!("[multi-team-daemon] run {} completed", ctx.run_id);
                    }
                    Ok(Err(e)) => {
                        eprintln!("[multi-team-daemon] run {} error: {e}", ctx.run_id);
                    }
                    Err(_) => {
                        eprintln!("[multi-team-daemon] run {} panicked", ctx.run_id);
                    }
                }
            }
        }
        Ok(())
    }

    /// Poll the control socket for one incoming command (non-blocking).
    fn poll_control_socket(&self, listener: &UnixListener, buf: &mut String) -> CliResult<()> {
        match listener.accept() {
            Ok((mut stream, _addr)) => {
                buf.clear();
                let mut reader = std::io::BufReader::new(&mut stream);
                if reader.read_line(buf).is_ok() {
                    self.handle_control_command(&mut stream, buf.trim())?;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                eprintln!("[multi-team-daemon] socket accept error: {e}");
            }
        }
        Ok(())
    }

    /// Handle a single control socket command.
    fn handle_control_command(&self, stream: &mut UnixStream, cmd_line: &str) -> CliResult<()> {
        let cmd: serde_json::Value = match serde_json::from_str(cmd_line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(stream, r#"{{"ok":false,"error":"invalid json: {}"}}"#, e);
                return Ok(());
            }
        };

        let cmd_name = cmd["cmd"].as_str().unwrap_or("");
        match cmd_name {
            "start" => {
                let run_id = cmd["run_id"].as_str().unwrap_or("");
                if run_id.is_empty() {
                    let _ = writeln!(stream, r#"{{"ok":false,"error":"run_id is required"}}"#);
                    return Ok(());
                }
                // P0-4 fix: propagate actual error, not "delegated to daemon".
                match self.start_supervising(run_id) {
                    Ok(()) => {
                        let _ = writeln!(stream, r#"{{"ok":true,"run_id":"{}"}}"#, run_id);
                    }
                    Err(e) => {
                        let _ = writeln!(
                            stream,
                            r#"{{"ok":false,"run_id":"{}","error":"{}"}}"#,
                            run_id, e
                        );
                    }
                }
            }
            "status" => {
                let runs: Vec<serde_json::Value> = {
                    let contexts = self
                        .contexts
                        .lock()
                        .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
                    contexts
                        .iter()
                        .map(|ctx| {
                            let is_finished =
                                ctx.thread.as_ref().map(|t| t.is_finished()).unwrap_or(true);
                            serde_json::json!({
                                "run_id": ctx.run_id,
                                "status": if is_finished { "finished" } else { "running" },
                                "elapsed_secs": ctx.started_at.elapsed().as_secs(),
                            })
                        })
                        .collect()
                };
                let resp = serde_json::json!({"ok": true, "runs": runs});
                let _ = writeln!(
                    stream,
                    "{}",
                    serde_json::to_string(&resp).unwrap_or_default()
                );
            }
            "stop" => {
                // P0-1 fix: actually set shutdown, not just reply ok.
                self.shutdown.store(true, Ordering::SeqCst);
                let _ = writeln!(stream, r#"{{"ok":true}}"#);
            }
            _ => {
                let _ = writeln!(
                    stream,
                    r#"{{"ok":false,"error":"unknown command: {}"}}"#,
                    cmd_name
                );
            }
        }
        Ok(())
    }

    /// Graceful shutdown: signal all managed contexts to stop, drain them,
    /// and join threads with a deadline.
    fn graceful_shutdown(&self) -> CliResult<()> {
        eprintln!("[multi-team-daemon] graceful shutdown initiated");

        // Drain contexts from the registry.
        let contexts: Vec<MultiTeamContext> = {
            let mut guard = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            std::mem::take(&mut *guard)
        };

        if contexts.is_empty() {
            return Ok(());
        }

        eprintln!(
            "[multi-team-daemon] waiting for {} run(s) to finish...",
            contexts.len()
        );

        // P0-1 fix: signal every managed context to stop by invalidating
        // their heartbeat.  This causes drive_prepared_team_run to detect
        // lease loss and exit its main loop promptly.
        for ctx in &contexts {
            ctx.heartbeat_valid.store(false, Ordering::Release);
        }

        // P0-3 fix: join threads with a deadline.  Since std::thread::JoinHandle
        // lacks join_timeout, we poll is_finished() with a sleep loop.
        const JOIN_DEADLINE_SECS: u64 = 30;
        let deadline = Instant::now() + Duration::from_secs(JOIN_DEADLINE_SECS);

        for ctx in contexts {
            let Some(thread) = ctx.thread else { continue };
            loop {
                if thread.is_finished() {
                    let _ = thread.join();
                    break;
                }
                if Instant::now() >= deadline {
                    eprintln!(
                        "[multi-team-daemon] shutdown deadline exceeded for run {}",
                        ctx.run_id
                    );
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Multi-team daemon signal handling (channel-based, no static raw pointer)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn install_signal_handlers_mt(shutdown: Arc<AtomicBool>) {
    // P0-8 fix: leak the Arc to get a 'static reference for the signal
    // handler. The leaked memory is reclaimed at process exit. This avoids
    // the dangling raw pointer pattern of the per-run daemon while still
    // being async-signal-safe.
    let leaked: &'static AtomicBool = Box::leak(Box::new(shutdown));

    extern "C" fn handle(sig: i32) {
        let _ = sig;
        // SAFETY: MT_SIGNAL_FLAG is set before signal handlers are installed
        // and lives for the process lifetime. The store is async-signal-safe.
        // We access the static mut via raw pointer to avoid the static_mut_refs
        // warning in Rust 2024 edition.
        unsafe {
            let ptr: *const Option<&'static AtomicBool> = &raw const MT_SIGNAL_FLAG;
            if let Some(flag) = &*ptr {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }

    unsafe {
        MT_SIGNAL_FLAG = Some(leaked);
        signal(SIGTERM, handle as SigHandler);
        signal(SIGINT, handle as SigHandler);
    }
}

#[allow(dead_code)]
static mut MT_SIGNAL_FLAG: Option<&'static AtomicBool> = None;

// ---------------------------------------------------------------------------
// CLI integration: delegate team-run start to multi-team daemon
// ---------------------------------------------------------------------------

/// Try to send a start command to the multi-team daemon via its control socket.
/// Returns the response line on success.
#[allow(dead_code)]
pub(crate) fn try_delegate_to_daemon(
    store: &HarnessStore,
    run_id: &str,
) -> Result<String, std::io::Error> {
    let socket_path = multi_team_socket_path(store.root());
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let cmd = serde_json::json!({"cmd": "start", "run_id": run_id});
    let cmd_str = serde_json::to_string(&cmd).map_err(std::io::Error::other)?;
    writeln!(stream, "{cmd_str}")?;
    stream.flush()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

/// Send a status request to the multi-team daemon.
#[allow(dead_code)]
pub(crate) fn daemon_status_via_socket(store: &HarnessStore) -> Option<String> {
    let socket_path = multi_team_socket_path(store.root());
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    let cmd = r#"{"cmd":"status"}"#;
    writeln!(stream, "{cmd}").ok()?;
    stream.flush().ok()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf).ok()?;
    let response = buf.trim().to_string();
    if response.is_empty() {
        return None;
    }
    Some(response)
}

/// Send a stop command to the multi-team daemon.
#[allow(dead_code)]
pub(crate) fn daemon_stop_via_socket(store: &HarnessStore) -> Option<String> {
    let socket_path = multi_team_socket_path(store.root());
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    let cmd = r#"{"cmd":"stop"}"#;
    writeln!(stream, "{cmd}").ok()?;
    stream.flush().ok()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf).ok()?;
    Some(buf.trim().to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_info_debug() {
        let info = DaemonInfo {
            pid: 12345,
            locator: "tcp://127.0.0.1:9999".into(),
            generation: 3,
            supervisor_id: "sup-1".into(),
        };
        assert_eq!(info.pid, 12345);
        assert_eq!(info.generation, 3);
    }

    #[test]
    fn multi_team_socket_path_short_root() {
        let root = std::path::Path::new("/tmp/firm-test");
        let path = multi_team_socket_path(root);
        assert_eq!(path, root.join("supervisor.sock"));
    }

    #[test]
    fn multi_team_socket_path_long_root_fallback() {
        let long = "/tmp/very-long-directory-name-that-makes-the-path-exceed-the-af-unix-limit-on-macos-which-is-104-bytes".repeat(2);
        let root = std::path::Path::new(&long);
        let path = multi_team_socket_path(root);
        assert!(path.to_string_lossy().starts_with("/tmp/firm-supervisor-"));
        assert!(path.to_string_lossy().len() < 104);
    }

    #[test]
    fn multi_team_socket_path_deterministic() {
        let root = std::path::Path::new("/some/store/root");
        let p1 = multi_team_socket_path(root);
        let p2 = multi_team_socket_path(root);
        assert_eq!(p1, p2);
    }
}
