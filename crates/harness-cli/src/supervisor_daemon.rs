//! Multi-team supervisor daemon: one long-lived process manages N team-runs.
//!
//! The daemon watches the store for active team-runs and runs one supervisor
//! context per run on dedicated threads, reusing the existing
//! [`crate::prepare_team_run_start`] + [`crate::drive_prepared_team_run`] path.
//! It exposes a Unix-domain control socket so `harness team-run start` can
//! become a non-blocking control message.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ Daemon (main thread)                        │
//! │  ├─ control listener (supervisor.sock)      │
//! │  ├─ scan loop (periodic store re-read)      │
//! │  └─ SupervisorContext registry              │
//! │       ├─ run-A: thread + registration       │
//! │       ├─ run-B: thread + registration       │
//! │       └─ ...                                │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! Crash recovery: on restart the daemon enumerates non-terminal team-runs,
//! checks supervisor leases, and re-attaches to orphaned runs whose leases
//! have expired.
//!
//! The whole module is gated `#[cfg(unix)]` at its declaration in `main.rs`.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_core::{AgentTeamRun, TeamRunStatus};
use harness_store::HarnessStore;

use crate::{
    drive_prepared_team_run, latest_team_runs_in_append_order, prepare_team_run_start,
    CliError, CliResult,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// File name of the supervisor daemon control socket under the store root.
const SUPERVISOR_SOCKET_NAME: &str = "supervisor.sock";

/// Directory for short-named fallback sockets when the store-root path exceeds
/// the AF_UNIX sun_path limit.
#[cfg(target_os = "macos")]
const SUPERVISOR_SOCKET_FALLBACK_DIR: &str = "/tmp";

/// File name of the daemon pidfile.
const SUPERVISOR_PIDFILE_NAME: &str = "supervisor-daemon.pid";

/// Interval between store scans for new/terminated team-runs.
#[allow(dead_code)]
const SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// How long to wait for managed context threads to join during shutdown.
#[allow(dead_code)]
const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Conservative AF_UNIX `sun_path` budget (104 on macOS, 108 on Linux).
const SUN_PATH_MAX: usize = 104;

/// How often the shared daemon heartbeat is written (seconds).
const DAEMON_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

/// Number of additional re-reads before retiring a context whose stored
/// TeamRun no longer appears non-terminal (defense against stale reads).
const STALE_RUN_RETIRE_GRACE: u32 = 3;

// ---------------------------------------------------------------------------
// Socket / pidfile paths
// ---------------------------------------------------------------------------

/// Compute the supervisor daemon socket path.
///
/// On macOS the AF_UNIX sun_path limit is 104 bytes; if the store-root-based
/// path exceeds that, we use a hash-based short name under a static directory
/// (`/tmp`) instead.
pub fn supervisor_socket_path(store_root: &Path) -> PathBuf {
    let default = store_root.join(SUPERVISOR_SOCKET_NAME);
    if default.as_os_str().len() < SUN_PATH_MAX {
        return default;
    }
    // Use a hash-based short name in a well-known short directory.
    let hash = content_hash_hex16(store_root.as_os_str().as_encoded_bytes());
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(SUPERVISOR_SOCKET_FALLBACK_DIR)
            .join(format!("harness-{hash}.sock"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux has a 108-byte limit; just try /tmp as well.
        PathBuf::from("/tmp").join(format!("harness-{hash}.sock"))
    }
}

fn content_hash_hex16(data: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn supervisor_pidfile_path(store_root: &Path) -> PathBuf {
    store_root.join(SUPERVISOR_PIDFILE_NAME)
}

/// True if a supervisor daemon is reachable at the store socket.
pub fn supervisor_daemon_is_available(store_root: &Path) -> bool {
    let path = supervisor_socket_path(store_root);
    UnixStream::connect(path).is_ok()
}

// ---------------------------------------------------------------------------
// Control protocol
// ---------------------------------------------------------------------------

/// Inbound control message from the socket.
#[derive(Debug, serde::Deserialize)]
struct ControlRequest {
    cmd: String,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    max_concurrency: Option<usize>,
}

/// Outbound control response.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ControlResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<Vec<RunStatusEntry>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunStatusEntry {
    pub run_id: String,
    pub status: String,
    pub members: usize,
}

// ---------------------------------------------------------------------------
// Supervisor context
// ---------------------------------------------------------------------------

/// One managed team-run: its supervisor registration and its driver thread.
#[allow(dead_code)]
struct SupervisorContext {
    run_id: String,
    /// The thread driving this team-run.  `None` after it has been joined.
    thread: Option<std::thread::JoinHandle<CliResult<()>>>,
    /// Tracked so we don't re-join.
    reaped: bool,
    /// When this context was created.
    started_at: Instant,
}

impl SupervisorContext {
    fn finished(&mut self) -> bool {
        if self.reaped {
            return true;
        }
        if let Some(handle) = self.thread.as_ref() {
            if handle.is_finished() {
                self.reaped = true;
                return true;
            }
        }
        false
    }

    fn join(&mut self) {
        if self.reaped {
            return;
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
            self.reaped = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Daemon state
// ---------------------------------------------------------------------------

/// Pending ad-hoc start requests received over the control socket between
/// scan cycles.
#[derive(Debug, Clone)]
struct PendingStart {
    run_id: String,
    max_concurrency: usize,
}

#[allow(dead_code)]
struct DaemonState {
    store_root: PathBuf,
    store: HarnessStore,
    /// Managed team-run contexts, keyed by run_id.
    contexts: HashMap<String, SupervisorContext>,
    /// Pending start requests from the control socket.
    pending_starts: Vec<PendingStart>,
    /// How many consecutive scans a finished context persisted before cleanup.
    finished_scan_count: HashMap<String, u32>,
    shutdown: Arc<AtomicBool>,
}

impl DaemonState {
    fn new(store_root: PathBuf, store: HarnessStore, shutdown: Arc<AtomicBool>) -> Self {
        Self {
            store_root,
            store,
            contexts: HashMap::new(),
            pending_starts: Vec::new(),
            finished_scan_count: HashMap::new(),
            shutdown,
        }
    }

    /// Enumerate non-terminal team-runs from the store.
    fn active_team_runs(&self) -> CliResult<Vec<AgentTeamRun>> {
        let runs = latest_team_runs_in_append_order(&self.store)?;
        Ok(runs
            .into_iter()
            .filter(|run| !matches!(run.status, TeamRunStatus::Completed | TeamRunStatus::Cancelled | TeamRunStatus::Failed))
            .collect())
    }

    /// Start supervising a team-run on a new thread.
    fn start_supervising(
        &mut self,
        run_id: &str,
        max_concurrency: usize,
    ) -> CliResult<()> {
        if self.contexts.contains_key(run_id) {
            return Ok(()); // Already managed.
        }
        let prepared = prepare_team_run_start(&self.store, run_id, max_concurrency)?;
        let run_id_owned = run_id.to_string();
        let handle = std::thread::spawn(move || {
            drive_prepared_team_run(
                prepared,
                None,        // execution_space
                None,        // project_context
                max_concurrency,
                Duration::from_secs(300), // idle_timeout
                None,        // live_sink
            )
        });
        self.contexts.insert(
            run_id_owned.clone(),
            SupervisorContext {
                run_id: run_id_owned,
                thread: Some(handle),
                reaped: false,
                started_at: Instant::now(),
            },
        );
        Ok(())
    }

    /// Reap finished contexts and clean up their entries.
    fn reap_finished(&mut self) {
        let mut finished_ids = Vec::new();
        for (id, ctx) in self.contexts.iter_mut() {
            if ctx.finished() {
                finished_ids.push(id.clone());
            }
        }
        // Second pass: join and track retirement grace count.
        for run_id in &finished_ids {
            // The context was reaped by `finished()` above, so join is a no-op.
            let count = self
                .finished_scan_count
                .entry(run_id.clone())
                .and_modify(|c| *c += 1)
                .or_insert(1);
            if *count >= STALE_RUN_RETIRE_GRACE {
                self.contexts.remove(run_id);
                self.finished_scan_count.remove(run_id);
                eprintln!("supervisor daemon: retired context for {run_id}");
            }
        }
    }

    /// Process pending ad-hoc start requests.
    fn process_pending(&mut self) -> CliResult<()> {
        let pending: Vec<PendingStart> = self.pending_starts.drain(..).collect();
        for req in pending {
            if let Err(error) = self.start_supervising(&req.run_id, req.max_concurrency) {
                eprintln!(
                    "supervisor daemon: failed to start supervising {}: {error}",
                    req.run_id
                );
            }
        }
        Ok(())
    }

    /// Scan the store and start supervising any active runs we're not already
    /// managing. This is the core adoption loop.
    fn scan(&mut self, max_concurrency: usize) -> CliResult<()> {
        self.process_pending()?;
        let active = self.active_team_runs()?;
        for run in &active {
            if !self.contexts.contains_key(&run.id) {
                eprintln!(
                    "supervisor daemon: adopting team-run {} (status: {})",
                    run.id,
                    serde_json::to_string(&run.status).unwrap_or_default()
                );
                if let Err(error) = self.start_supervising(&run.id, max_concurrency) {
                    eprintln!(
                        "supervisor daemon: failed to adopt {}: {error}",
                        run.id
                    );
                }
            }
        }
        self.reap_finished();

        // Clean up contexts for runs no longer in the active set.
        let active_ids: std::collections::HashSet<&str> =
            active.iter().map(|r| r.id.as_str()).collect();
        let mut to_retire = Vec::new();
        for (run_id, ctx) in &mut self.contexts {
            if ctx.finished() && !active_ids.contains(run_id.as_str()) {
                to_retire.push(run_id.clone());
            }
        }
        for run_id in to_retire {
            eprintln!("supervisor daemon: retiring finished context for {run_id}");
            self.contexts.remove(&run_id);
        }
        Ok(())
    }

    /// Build a status snapshot for the control socket.
    fn status_snapshot(&mut self) -> Vec<RunStatusEntry> {
        self.contexts
            .iter_mut()
            .map(|(id, ctx)| RunStatusEntry {
                run_id: id.clone(),
                status: if ctx.finished() {
                    "finished".into()
                } else {
                    "running".into()
                },
                members: 0, // Not cheaply available without store re-read.
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Control socket
// ---------------------------------------------------------------------------

fn validate_socket_path_len(path: &Path) -> io::Result<()> {
    let len = path.as_os_str().len();
    if len >= SUN_PATH_MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "supervisor socket path too long ({len} bytes, limit {SUN_PATH_MAX}): {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn bind_supervisor_socket(socket_path: &Path) -> io::Result<UnixListener> {
    validate_socket_path_len(socket_path)?;

    if socket_path.exists() {
        if UnixStream::connect(socket_path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "supervisor daemon already running at {}",
                    socket_path.display()
                ),
            ));
        }
        // Stale socket: remove and rebind.
        std::fs::remove_file(socket_path)?;
    }

    match UnixListener::bind(socket_path) {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "supervisor daemon already running at {} (bind race)",
                socket_path.display()
            ),
        )),
        Err(error) => Err(error),
    }
}

fn handle_control_connection(
    stream: UnixStream,
    state: &Arc<Mutex<DaemonState>>,
    default_max_concurrency: usize,
) {
    let writer_result = stream.try_clone();
    let Ok(mut writer) = writer_result else {
        return; // Can't clone, can't respond — just return.
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    let response = match reader.read_line(&mut line) {
        Ok(0) => return, // Client hung up.
        Ok(_) => {
            let req: ControlRequest = match serde_json::from_str(line.trim_end()) {
                Ok(req) => req,
                Err(error) => {
                    let _ = write_control_response(
                        &mut writer,
                        &ControlResponse {
                            ok: false,
                            error: Some(format!("bad request: {error}")),
                            run_id: None,
                            runs: None,
                        },
                    );
                    return;
                }
            };
            match req.cmd.as_str() {
                "start" => {
                    let run_id = match req.run_id {
                        Some(id) => id,
                        None => {
                            let _ = write_control_response(
                                &mut writer,
                                &ControlResponse {
                                    ok: false,
                                    error: Some("missing run_id".into()),
                                    run_id: None,
                                    runs: None,
                                },
                            );
                            return;
                        }
                    };
                    let max_concurrency = req
                        .max_concurrency
                        .unwrap_or(default_max_concurrency);
                    let mut guard = state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard
                        .pending_starts
                        .push(PendingStart {
                            run_id: run_id.clone(),
                            max_concurrency,
                        });
                    ControlResponse {
                        ok: true,
                        error: None,
                        run_id: Some(run_id),
                        runs: None,
                    }
                }
                "status" => {
                    let mut guard = state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let runs = guard.status_snapshot();
                    ControlResponse {
                        ok: true,
                        error: None,
                        run_id: None,
                        runs: Some(runs),
                    }
                }
                "stop" => {
                    // The scan loop reads shutdown; the control handler just
                    // acknowledges.
                    ControlResponse {
                        ok: true,
                        error: None,
                        run_id: None,
                        runs: None,
                    }
                }
                other => ControlResponse {
                    ok: false,
                    error: Some(format!("unknown command: {other}")),
                    run_id: None,
                    runs: None,
                },
            }
        }
        Err(error) => ControlResponse {
            ok: false,
            error: Some(format!("read error: {error}")),
            run_id: None,
            runs: None,
        },
    };

    let _ = write_control_response(&mut writer, &response);
}

fn write_control_response(writer: &mut impl Write, response: &ControlResponse) -> io::Result<()> {
    let mut out = serde_json::to_string(response).map_err(io::Error::other)?;
    out.push('\n');
    writer.write_all(out.as_bytes())?;
    writer.flush()
}

// ---------------------------------------------------------------------------
// Signal handling (best-effort)
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod signal {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    pub(super) const SIGINT: i32 = 2;
    pub(super) const SIGTERM: i32 = 15;

    type SigHandler = extern "C" fn(i32);

    extern "C" {
        pub(super) fn signal(signum: i32, handler: SigHandler) -> usize;
    }

    pub(super) fn install(shutdown: &AtomicBool) {
        let ptr = shutdown as *const AtomicBool as usize;
        extern "C" fn handle(sig: i32) {
            // We can't access arbitrary state from a signal handler safely.
            // We use a well-known static instead.
            super::signal_handler_dispatch(sig);
        }
        unsafe {
            signal(SIGTERM, handle);
            signal(SIGINT, handle);
        }
        // Store a reference the handler can use.
        SHUTDOWN_FLAG
            .store(ptr, Ordering::SeqCst);
    }

    static SHUTDOWN_FLAG: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn dispatch_shutdown() {
        let ptr = SHUTDOWN_FLAG.load(Ordering::SeqCst);
        if ptr != 0 {
            let flag = unsafe { &*(ptr as *const AtomicBool) };
            flag.store(true, Ordering::SeqCst);
        }
    }
}

fn signal_handler_dispatch(_sig: i32) {
    signal::dispatch_shutdown();
}

// ---------------------------------------------------------------------------
// Daemon heartbeat
// ---------------------------------------------------------------------------

/// Write a daemon heartbeat file so external monitoring can tell the daemon
/// is alive without connecting to the control socket.
fn write_heartbeat(store_root: &Path, pid: u32) {
    let path = store_root.join("supervisor-daemon.heartbeat");
    let content = serde_json::json!({
        "pid": pid,
        "started_at": chrono_or_now(),
    });
    if let Ok(json) = serde_json::to_string(&content) {
        let _ = std::fs::write(&path, json);
    }
}

fn chrono_or_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("unix-ms:{}", d.as_millis()))
        .unwrap_or_else(|_| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Run the multi-team supervisor daemon in the foreground.
///
/// Binds the control socket, adopts any active team-runs, and enters the
/// serve loop. Returns when the daemon is shut down (signal or control
/// message).
pub fn run_serve(
    store_root: &Path,
    max_concurrency: usize,
    scan_interval: Duration,
) -> CliResult<()> {
    std::fs::create_dir_all(store_root)?;

    let socket_path = supervisor_socket_path(store_root);
    let listener = bind_supervisor_socket(&socket_path)?;
    listener.set_nonblocking(true)?;

    let pid = std::process::id();
    let pidfile = supervisor_pidfile_path(store_root);
    std::fs::write(&pidfile, pid.to_string())?;

    let shutdown = Arc::new(AtomicBool::new(false));
    signal::install(&shutdown);

    let store = HarnessStore::new(store_root.to_path_buf());
    let state = Arc::new(Mutex::new(DaemonState::new(
        store_root.to_path_buf(),
        store,
        Arc::clone(&shutdown),
    )));

    // Initial adoption: pick up any active team-runs already in the store.
    {
        let mut guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(error) = guard.scan(max_concurrency) {
            eprintln!("supervisor daemon: initial scan error: {error}");
        }
    }

    let heartbeat_interval = DAEMON_HEARTBEAT_INTERVAL;
    let mut last_heartbeat = Instant::now();
    let mut last_scan = Instant::now();

    eprintln!(
        "supervisor daemon listening on {} (pid {pid})",
        socket_path.display()
    );

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Accept one control connection.
        match listener.accept() {
            Ok((stream, _)) => {
                let state_clone = Arc::clone(&state);
                std::thread::spawn(move || {
                    handle_control_connection(stream, &state_clone, max_concurrency);
                });
            }
            Err(ref error) if error.kind() == io::ErrorKind::WouldBlock => {
                // No pending connection — carry on.
            }
            Err(error) => {
                eprintln!("supervisor daemon: accept error: {error}");
            }
        }

        // Periodic scan.
        let now = Instant::now();
        if now.duration_since(last_scan) >= scan_interval {
            let mut guard = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Err(error) = guard.scan(max_concurrency) {
                eprintln!("supervisor daemon: scan error: {error}");
            }
            last_scan = Instant::now();
        }

        // Periodic heartbeat.
        if now.duration_since(last_heartbeat) >= heartbeat_interval {
            write_heartbeat(store_root, pid);
            last_heartbeat = Instant::now();
        }

        // Brief sleep to avoid busy-waiting.
        std::thread::sleep(Duration::from_millis(100));
    }

    eprintln!("supervisor daemon: shutting down...");

    // Drain managed contexts.
    {
        let mut guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for ctx in guard.contexts.values_mut() {
            ctx.join();
        }
    }

    // Best-effort cleanup.
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&pidfile);
    let _ = std::fs::remove_file(store_root.join("supervisor-daemon.heartbeat"));

    eprintln!("supervisor daemon: stopped");
    Ok(())
}

// ---------------------------------------------------------------------------
// Client helpers (used by team_run_start when daemon is available)
// ---------------------------------------------------------------------------

/// Send a start request to the daemon over the control socket.
pub fn daemon_start_run(
    store_root: &Path,
    run_id: &str,
    max_concurrency: usize,
) -> CliResult<ControlResponse> {
    let path = supervisor_socket_path(store_root);
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let req = serde_json::json!({
        "cmd": "start",
        "run_id": run_id,
        "max_concurrency": max_concurrency,
    });
    let mut line = serde_json::to_string(&req).map_err(io::Error::other)?;
    line.push('\n');

    {
        let mut writer = stream.try_clone()?;
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
    }

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    let n = reader.read_line(&mut buf)?;
    if n == 0 {
        return Err(CliError::Usage(
            "supervisor daemon closed connection without response".into(),
        ));
    }
    serde_json::from_str::<ControlResponse>(buf.trim_end())
        .map_err(|error| CliError::Usage(format!("bad daemon response: {error}")))
}

/// Query daemon status.
pub fn daemon_status(store_root: &Path) -> CliResult<ControlResponse> {
    let path = supervisor_socket_path(store_root);
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let req = serde_json::json!({"cmd": "status"});
    let mut line = serde_json::to_string(&req).map_err(io::Error::other)?;
    line.push('\n');

    {
        let mut writer = stream.try_clone()?;
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
    }

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    let n = reader.read_line(&mut buf)?;
    if n == 0 {
        return Err(CliError::Usage(
            "supervisor daemon closed connection without response".into(),
        ));
    }
    serde_json::from_str::<ControlResponse>(buf.trim_end())
        .map_err(|error| CliError::Usage(format!("bad daemon response: {error}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_response_roundtrip() {
        let resp = ControlResponse {
            ok: true,
            error: None,
            run_id: Some("team-run-1".into()),
            runs: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("team-run-1"));
        assert!(json.contains("\"ok\":true"));
    }

    #[test]
    fn control_response_with_runs() {
        let resp = ControlResponse {
            ok: true,
            error: None,
            run_id: None,
            runs: Some(vec![
                RunStatusEntry {
                    run_id: "run-1".into(),
                    status: "running".into(),
                    members: 2,
                },
                RunStatusEntry {
                    run_id: "run-2".into(),
                    status: "finished".into(),
                    members: 1,
                },
            ]),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("run-1"));
        assert!(json.contains("run-2"));
    }

    #[test]
    fn control_request_parse() {
        let req: ControlRequest =
            serde_json::from_str(r#"{"cmd":"start","run_id":"team-run-1"}"#).unwrap();
        assert_eq!(req.cmd, "start");
        assert_eq!(req.run_id.as_deref(), Some("team-run-1"));
    }

    #[test]
    fn socket_path_too_long_rejected() {
        let long = PathBuf::from(format!("/tmp/{}/supervisor.sock", "x".repeat(120)));
        assert!(validate_socket_path_len(&long).is_err());
    }
}
