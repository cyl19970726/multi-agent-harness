//! Cross-process warm-child host for resident `claude` processes (opt-in, unix).
//!
//! The in-process pool in [`crate::resident`] keeps a `claude` child warm across
//! turns, but the harness CLI is short-lived: each `harness agent deliver` is a
//! fresh process, so an in-process pool dies with the command. This module wraps
//! that exact pool in a long-lived **daemon** reachable over a per-workspace Unix
//! domain socket, so successive short-lived deliveries share one warm child.
//!
//! It is a pure transport shell: no new pool logic. The daemon owns an
//! `Arc<Mutex<ResidentPool>>` and, per connection, reads one JSON request line,
//! calls [`ResidentPool::run_turn`] under the lock (the same one-turn-at-a-time
//! discipline a single child already requires), then writes one JSON response
//! line. The children are STILL the documented headless contract
//! `claude -p --input-format stream-json …` — see ADR 0021 (amends 0018).
//!
//! Protocol: line-delimited JSON (NDJSON), one request line and one response line
//! per connection, mirroring the `read_line` discipline used in `resident.rs` and
//! `serve_command`. Everything is synchronous `std::*` — no tokio, no new crates.
//!
//! The whole module is gated `#[cfg(unix)]` at its declaration in `main.rs`.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::resident::{self, ResidentPool};

/// File name of the per-workspace daemon socket under the store root.
const SOCKET_NAME: &str = "resident.sock";

/// File name of the daemon pidfile (sibling of the socket), used by `stop`.
const PIDFILE_NAME: &str = "resident-daemon.pid";

/// Conservative AF_UNIX `sun_path` budget (104 on macOS, 108 on Linux). We
/// validate against the smaller bound so a path that works here works on both.
const SUN_PATH_MAX: usize = 104;

/// Slack added to a turn's timeout for the client socket read deadline, so the
/// daemon's own per-turn timeout (which kills + evicts the wedged child and
/// writes a proper error response) fires first; this read deadline is only the
/// last-resort guard against a fully unresponsive daemon.
const DELIVER_READ_SLACK: Duration = Duration::from_secs(10);

/// Set by the SIGTERM/SIGINT handler so the accept loop can exit between
/// connections and clean up its socket. A hard `SIGKILL` cannot run this, but
/// daemon death closes the children's stdin pipes and `claude` exits on EOF.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// One delivery request over the socket: exactly the args of
/// [`ResidentPool::run_turn`], so the daemon is a faithful proxy for it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DaemonRequest {
    /// Pool key's member component (matches `member.id` on the CLI side).
    pub member_id: String,
    /// Full launch surface; the pool fingerprints it to key the warm child.
    pub config: resident::ResidentConfig,
    /// Where the daemon redirects this child's stderr (referenced by path, never
    /// inlined over the socket — matches the resident keep-alive constraint).
    pub stderr_path: String,
    /// The user-message text for this turn.
    pub user_text: String,
    /// Per-turn timeout in milliseconds.
    pub timeout_ms: u64,
}

/// One delivery response over the socket. Shaped so the CLI reconstructs the same
/// `(success, events, session_id, stderr)` tuple the inline path returns.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DaemonResponse {
    pub success: bool,
    pub events: Vec<resident::ResidentEvent>,
    pub session_id: Option<String>,
    /// Echoed back so the CLI reads the log from the same path it requested.
    pub stderr_path: String,
    /// `Some(message)` on any server-side failure (bad JSON, `run_turn` Err); the
    /// connection never hangs the client.
    pub error: Option<String>,
}

/// The per-workspace socket path. Both client and daemon derive it from the same
/// store root, so discovery needs no registry beyond `HARNESS_ROOT`.
pub fn daemon_socket_path(harness_root: &Path) -> PathBuf {
    harness_root.join(SOCKET_NAME)
}

/// The daemon pidfile path (sibling of the socket).
fn daemon_pidfile_path(harness_root: &Path) -> PathBuf {
    harness_root.join(PIDFILE_NAME)
}

/// True if a daemon is reachable at the workspace socket. A successful probe
/// connect means a live daemon owns the socket; any error (no socket, refused,
/// stale) means "no daemon" and the caller degrades to the inline path.
pub fn daemon_is_available(harness_root: &Path) -> bool {
    let path = daemon_socket_path(harness_root);
    UnixStream::connect(path).is_ok()
}

/// Client side: connect, write one request line, read one response line.
pub fn daemon_deliver(harness_root: &Path, req: &DaemonRequest) -> io::Result<DaemonResponse> {
    let path = daemon_socket_path(harness_root);
    let stream = UnixStream::connect(path)?;

    // Bound the read so a stalled daemon (e.g. one wedged child holding the pool
    // lock) surfaces a timed-out delivery instead of hanging the CLI forever. We
    // allow a slack beyond the turn's own timeout so the daemon's in-process
    // per-turn timeout fires first and returns a proper error response.
    let read_deadline = Duration::from_millis(req.timeout_ms).saturating_add(DELIVER_READ_SLACK);
    stream.set_read_timeout(Some(read_deadline))?;

    // Write the request line.
    {
        let mut writer = stream.try_clone()?;
        let mut line = serde_json::to_string(req).map_err(io::Error::other)?;
        line.push('\n');
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
    }

    // Read exactly one response line.
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    let n = reader.read_line(&mut buf)?;
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "resident daemon closed the connection without a response",
        ));
    }
    serde_json::from_str::<DaemonResponse>(buf.trim_end()).map_err(io::Error::other)
}

/// Validate the socket path fits the `sun_path` limit before binding so the
/// failure is a clear message instead of an opaque `bind` errno.
fn check_socket_path_len(path: &Path) -> io::Result<()> {
    let len = path.as_os_str().len();
    if len >= SUN_PATH_MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "socket path too long ({len} bytes, limit {SUN_PATH_MAX}): {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Bind the listener, cleaning up a stale socket first.
///
/// If the path exists and a probe connect succeeds, a live daemon owns it -> we
/// refuse to start ("already running"). If connect fails the socket is stale ->
/// remove and bind. A `bind` that still loses a startup race is reported as
/// "already running" rather than panicking (TOCTOU-safe).
fn bind_with_stale_cleanup(socket_path: &Path) -> io::Result<UnixListener> {
    check_socket_path_len(socket_path)?;

    if socket_path.exists() {
        if UnixStream::connect(socket_path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "resident daemon already running at {}",
                    socket_path.display()
                ),
            ));
        }
        // Stale socket (or a leftover plain file): remove and rebind.
        std::fs::remove_file(socket_path)?;
    }

    match UnixListener::bind(socket_path) {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "resident daemon already running at {} (bind race)",
                socket_path.display()
            ),
        )),
        Err(error) => Err(error),
    }
}

/// Install best-effort SIGTERM/SIGINT handlers that set [`SHUTDOWN`]. Uses the
/// libc `signal` binding available to `std` on unix; if registration is not
/// possible the daemon still shuts down cleanly via the normal `stop` (SIGTERM)
/// path because daemon death closes children's stdin pipes (claude exits on EOF).
fn install_signal_handlers() {
    extern "C" fn handle(_sig: i32) {
        SHUTDOWN.store(true, Ordering::SeqCst);
    }
    // SAFETY: `handle` only stores into an AtomicBool (async-signal-safe), and
    // `signal(2)` is a stable C ABI symbol present in libc on all unix targets.
    // `handle` (an `extern "C" fn(i32)`) coerces directly to `SigHandler`, so no
    // function-item-to-integer cast is needed.
    unsafe {
        signal(SIGTERM, handle);
        signal(SIGINT, handle);
    }
}

// Minimal FFI to `signal(2)` so we avoid adding a `libc`/`signal-hook` crate.
// `signal` is a stable C ABI symbol resolved from the C runtime at link time.
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;
/// C `void (*)(int)` signal-handler pointer. Modeling the handler as a real fn
/// pointer (rather than `usize`) lets us pass `handle` without a cast — the
/// returned previous handler is ignored, so a plain `usize` return is fine.
type SigHandler = extern "C" fn(i32);
extern "C" {
    fn signal(signum: i32, handler: SigHandler) -> usize;
}

/// Run the daemon in the foreground: bind, write a pidfile, install signal
/// handlers, and serve connections until shutdown. The socket and pidfile are
/// removed best-effort on clean exit.
pub fn run_daemon(harness_root: &Path, idle_secs: u64) -> io::Result<()> {
    std::fs::create_dir_all(harness_root)?;
    let socket_path = daemon_socket_path(harness_root);
    let listener = bind_with_stale_cleanup(&socket_path)?;

    // A non-blocking accept so the loop can poll SHUTDOWN between connections.
    listener.set_nonblocking(true)?;

    let pidfile = daemon_pidfile_path(harness_root);
    std::fs::write(&pidfile, std::process::id().to_string())?;

    install_signal_handlers();

    let pool = Arc::new(Mutex::new(ResidentPool::with_max_idle(
        Duration::from_secs(idle_secs),
    )));

    println!("resident daemon listening on {}", socket_path.display());

    serve_loop(&listener, &pool, &SHUTDOWN);

    // Best-effort cleanup on clean shutdown.
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&pidfile);
    Ok(())
}

/// The accept loop, factored out so tests can drive it against a pre-bound
/// listener with an independent shutdown flag. Spawns a handler thread per
/// connection; a failed accept is logged and skipped (never fatal), copying
/// `serve_command`'s resilience.
pub fn serve_loop(listener: &UnixListener, pool: &Arc<Mutex<ResidentPool>>, shutdown: &AtomicBool) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => {
                let pool = Arc::clone(pool);
                std::thread::spawn(move || {
                    if let Err(error) = handle_conn(stream, pool) {
                        eprintln!("resident daemon: connection error: {error}");
                    }
                });
            }
            Err(ref error) if error.kind() == io::ErrorKind::WouldBlock => {
                // Non-blocking listener with no pending connection: poll again.
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                eprintln!("resident daemon: accept failed: {error}");
            }
        }
    }
}

/// Handle one connection: read one request line, run the turn under the pool
/// lock (plus an opportunistic idle reclaim), then write one response line.
///
/// The lock is released before writing the response so the socket write does not
/// extend the lock hold time. Any failure becomes a `success=false` response so
/// the client never hangs.
pub fn handle_conn(stream: UnixStream, pool: Arc<Mutex<ResidentPool>>) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        // Client connected then hung up without sending a request.
        return Ok(());
    }

    let response = match serde_json::from_str::<DaemonRequest>(line.trim_end()) {
        Ok(req) => run_request(&pool, req),
        Err(error) => DaemonResponse {
            success: false,
            events: Vec::new(),
            session_id: None,
            stderr_path: String::new(),
            error: Some(format!("bad request json: {error}")),
        },
    };

    let mut out = serde_json::to_string(&response).map_err(io::Error::other)?;
    out.push('\n');
    writer.write_all(out.as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Run one request against the locked pool and build the response.
fn run_request(pool: &Arc<Mutex<ResidentPool>>, req: DaemonRequest) -> DaemonResponse {
    let stderr_path = req.stderr_path.clone();
    let timeout = Duration::from_millis(req.timeout_ms.max(1));

    let result = {
        // Hold the lock only across run_turn + the opportunistic idle sweep; the
        // global lock enforces the one-turn-at-a-time discipline a single child
        // already requires. Poisoned-lock recovery keeps the daemon serving.
        let mut guard = match pool.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let turn = guard.run_turn(
            &req.member_id,
            req.config,
            Path::new(&req.stderr_path),
            &req.user_text,
            timeout,
        );
        // Reap any idle children opportunistically (no background timer needed).
        guard.reclaim_idle();
        turn
    };

    match result {
        Ok(turn) => DaemonResponse {
            success: turn.success,
            events: turn.events,
            session_id: turn.session_id,
            stderr_path,
            error: None,
        },
        Err(error) => DaemonResponse {
            success: false,
            events: Vec::new(),
            session_id: None,
            stderr_path,
            error: Some(error.to_string()),
        },
    }
}

/// One-word status of the daemon at a workspace socket.
pub enum DaemonStatus {
    Running,
    Stale,
    Absent,
}

/// Probe the socket: a live daemon (connect ok), a stale socket file (exists but
/// refused), or no socket at all.
pub fn daemon_status(harness_root: &Path) -> DaemonStatus {
    let path = daemon_socket_path(harness_root);
    if UnixStream::connect(&path).is_ok() {
        DaemonStatus::Running
    } else if path.exists() {
        DaemonStatus::Stale
    } else {
        DaemonStatus::Absent
    }
}

/// Read the daemon pid from the pidfile, if present.
pub fn daemon_pid(harness_root: &Path) -> Option<u32> {
    std::fs::read_to_string(daemon_pidfile_path(harness_root))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resident::{write_fake_claude, ResidentConfig, ResidentEvent, TempDir};

    fn config_for(binary: &Path, cwd: &Path) -> ResidentConfig {
        ResidentConfig {
            binary: binary.display().to_string(),
            model: None,
            permission_mode: "plan".into(),
            tools: vec![],
            system_prompt: String::new(),
            mcp_config_path: None,
            add_dirs: vec![],
            cwd: cwd.display().to_string(),
            resume: None,
        }
    }

    /// Spawn the accept loop on a thread against a pre-bound listener, using a
    /// per-test shutdown flag (NOT the global static) so concurrent tests do not
    /// interfere. Returns the join handle and the shutdown flag; the caller sets
    /// the flag and nudges the socket once to stop the loop.
    fn start_test_daemon(socket_path: &Path) -> (std::thread::JoinHandle<()>, Arc<AtomicBool>) {
        let listener = UnixListener::bind(socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let loop_shutdown = Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || {
            let pool = Arc::new(Mutex::new(ResidentPool::new()));
            serve_loop(&listener, &pool, &loop_shutdown);
        });
        (handle, shutdown)
    }

    /// Stop a test daemon: set its flag and connect once to unblock the poll.
    fn stop_test_daemon(
        handle: std::thread::JoinHandle<()>,
        shutdown: &Arc<AtomicBool>,
        socket_path: &Path,
    ) {
        shutdown.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(socket_path);
        handle.join().unwrap();
    }

    #[test]
    fn daemon_request_response_roundtrip_json() {
        let req = DaemonRequest {
            member_id: "m-1".into(),
            config: ResidentConfig {
                binary: "claude".into(),
                model: Some("opus".into()),
                permission_mode: "plan".into(),
                tools: vec!["Bash".into(), "Read".into()],
                system_prompt: "be helpful".into(),
                mcp_config_path: Some("/tmp/mcp.json".into()),
                add_dirs: vec!["/work".into()],
                cwd: "/work".into(),
                resume: Some("sess-1".into()),
            },
            stderr_path: "/tmp/claude.stderr".into(),
            user_text: "hello".into(),
            timeout_ms: 5000,
        };
        let line = serde_json::to_string(&req).unwrap();
        let back: DaemonRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(back.member_id, "m-1");
        assert_eq!(back.config, req.config);
        assert_eq!(back.timeout_ms, 5000);

        let resp = DaemonResponse {
            success: true,
            events: vec![
                ResidentEvent {
                    event_type: "system".into(),
                    payload: serde_json::json!({"type":"system","session_id":"s"}),
                },
                ResidentEvent {
                    event_type: "result".into(),
                    payload: serde_json::json!({"type":"result"}),
                },
            ],
            session_id: Some("s".into()),
            stderr_path: "/tmp/claude.stderr".into(),
            error: None,
        };
        let line = serde_json::to_string(&resp).unwrap();
        let back: DaemonResponse = serde_json::from_str(&line).unwrap();
        assert!(back.success);
        assert_eq!(back.events.len(), 2);
        assert_eq!(back.session_id.as_deref(), Some("s"));
        assert_eq!(back.events[0].event_type, "system");
    }

    #[test]
    fn daemon_is_available_false_when_no_socket() {
        let dir = TempDir::new("daemon-absent");
        assert!(!daemon_is_available(&dir.path));
        match daemon_status(&dir.path) {
            DaemonStatus::Absent => {}
            _ => panic!("expected Absent for empty dir"),
        }
    }

    #[test]
    fn stale_socket_is_replaced() {
        let dir = TempDir::new("daemon-stale");
        let socket_path = daemon_socket_path(&dir.path);
        // A leftover plain file at the socket path (no listener behind it).
        std::fs::write(&socket_path, b"stale").unwrap();
        assert!(socket_path.exists());

        // bind_with_stale_cleanup must remove it and bind a real listener.
        let listener = bind_with_stale_cleanup(&socket_path).unwrap();
        // The path now hosts a live listener: a probe connect succeeds.
        assert!(UnixStream::connect(&socket_path).is_ok());
        drop(listener);
    }

    #[test]
    fn socket_path_too_long_is_rejected() {
        let long = PathBuf::from(format!("/tmp/{}/resident.sock", "x".repeat(120)));
        assert!(check_socket_path_len(&long).is_err());
    }

    /// The core proof: a daemon serves TWO SEPARATE socket connections for the
    /// SAME member with ONE warm child, and session_id is continuous.
    #[test]
    fn daemon_keeps_child_warm_across_two_connections() {
        let dir = TempDir::new("daemon-warm");
        let pid_file = dir.join("pid");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_fake_claude(&dir.path, &pid_file, "sess-WARM");
        let config = config_for(&fake, &dir.path);

        let socket_path = daemon_socket_path(&dir.path);
        let (handle, shutdown) = start_test_daemon(&socket_path);

        let req = |text: &str| DaemonRequest {
            member_id: "m".into(),
            config: config.clone(),
            stderr_path: stderr_path.display().to_string(),
            user_text: text.into(),
            timeout_ms: 5000,
        };

        // Connection #1 (a fresh UnixStream — a distinct CLI invocation).
        let r1 = daemon_deliver(&dir.path, &req("one")).unwrap();
        assert!(r1.success, "first turn failed: {:?}", r1.error);
        assert_eq!(r1.session_id.as_deref(), Some("sess-WARM"));

        // Connection #2 (a SEPARATE UnixStream — proves cross-invocation reuse).
        let r2 = daemon_deliver(&dir.path, &req("two")).unwrap();
        assert!(r2.success, "second turn failed: {:?}", r2.error);
        assert_eq!(r2.session_id.as_deref(), Some("sess-WARM"));

        // The fake records its PID once per process: ONE line => ONE child
        // served BOTH connections, i.e. it stayed warm across them.
        let pids = std::fs::read_to_string(&pid_file).unwrap();
        assert_eq!(
            pids.lines().count(),
            1,
            "exactly one child should serve both connections (saw {} pids)",
            pids.lines().count()
        );

        // stderr-via-file works through the daemon.
        let stderr = std::fs::read_to_string(&stderr_path).unwrap();
        assert!(stderr.contains("fake claude up"));

        stop_test_daemon(handle, &shutdown, &socket_path);
    }

    /// A fake `claude` that stays resident, consumes stdin frames, but never
    /// emits a `result` — used to prove a wedged child no longer hangs the
    /// daemon (or its clients) forever now that `send_turn` enforces a timeout.
    fn write_hanging_claude(dir: &Path, session_id: &str) -> PathBuf {
        let script = format!(
            r#"#!/usr/bin/env bash
printf '%s\n' '{{"type":"system","session_id":"{session_id}"}}'
while IFS= read -r line; do
  :
done
exit 0
"#,
            session_id = session_id,
        );
        let path = dir.join("hanging-claude.sh");
        std::fs::write(&path, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    /// A wedged child must NOT hang the daemon: the per-turn timeout fires
    /// server-side, the wedged child is evicted, and the client gets a
    /// `success=false` response with a timeout error instead of hanging.
    #[test]
    fn wedged_child_times_out_into_error_response_not_hang() {
        let dir = TempDir::new("daemon-wedged");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_hanging_claude(&dir.path, "sess-WEDGE");
        let config = config_for(&fake, &dir.path);

        let socket_path = daemon_socket_path(&dir.path);
        let (handle, shutdown) = start_test_daemon(&socket_path);

        let req = DaemonRequest {
            member_id: "m".into(),
            config,
            stderr_path: stderr_path.display().to_string(),
            user_text: "wedge me".into(),
            timeout_ms: 300,
        };

        let start = std::time::Instant::now();
        let resp = daemon_deliver(&dir.path, &req).expect("client must get a response, not hang");
        let elapsed = start.elapsed();

        assert!(!resp.success, "a wedged child must yield a failed turn");
        assert!(
            resp.error.unwrap_or_default().contains("timeout"),
            "error should explain the per-turn timeout"
        );
        assert!(
            elapsed < Duration::from_secs(8),
            "delivery should return near the timeout, took {elapsed:?}"
        );

        stop_test_daemon(handle, &shutdown, &socket_path);
    }

    #[test]
    fn bad_request_json_yields_error_response_not_hang() {
        let dir = TempDir::new("daemon-badjson");
        let socket_path = daemon_socket_path(&dir.path);
        let (handle, shutdown) = start_test_daemon(&socket_path);

        // Send a malformed line and read the response.
        let stream = UnixStream::connect(&socket_path).unwrap();
        {
            let mut w = stream.try_clone().unwrap();
            w.write_all(b"{not json}\n").unwrap();
            w.flush().unwrap();
        }
        let mut reader = BufReader::new(stream);
        let mut buf = String::new();
        reader.read_line(&mut buf).unwrap();
        let resp: DaemonResponse = serde_json::from_str(buf.trim_end()).unwrap();
        assert!(!resp.success);
        assert!(resp.error.unwrap().contains("bad request json"));

        stop_test_daemon(handle, &shutdown, &socket_path);
    }
}
