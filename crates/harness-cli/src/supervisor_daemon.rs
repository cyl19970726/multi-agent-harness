//! Supervisor daemon — the detached process that owns the writer loop,
//! heartbeat, and TCP control listener for a team run.  The `harness daemon
//! supervisor serve` command calls [`run_supervisor_daemon`] in the foreground.
//! `harness team-run start` spawns the daemon as a child and exits.

use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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
    harness_bin: &str,
    max_concurrency: usize,
    idle_timeout_secs: u64,
) -> CliResult<()> {
    let home = dirs_fallback();
    let agents_dir = home.join("Library").join("LaunchAgents");
    std::fs::create_dir_all(&agents_dir)?;

    let label = format!("com.harness.supervisor.{team_run_id}");
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
        <string>{harness_bin}</string>
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
    <string>/tmp/harness-supervisor-{team_run_id}.stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/harness-supervisor-{team_run_id}.stderr.log</string>
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
    .env("HARNESS_ROOT", &store_root_str)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());

    // Inherit all parent environment variables so test harness vars
    // (FAKE_KIMI_RESULT, PATH with fake shims, etc.) flow through to
    // the daemon child.  HARNESS_ROOT is already overridden above.
    for (key, val) in std::env::vars() {
        if key != "HARNESS_ROOT" {
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
            let harness_bin = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "harness".to_string());
            generate_launchd_plist(&run_id, &harness_bin, max_concurrency, idle_timeout_secs)
        }
        other => Err(CliError::Usage(format!(
            "unknown supervisor daemon command: {other}"
        ))),
    }
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
}
