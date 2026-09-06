//! CLI-side machine daemon clients. No service loops or provider creation.
use crate::daemon_protocol::NativeSessionWakeEndpoint;
use crate::supervisor_daemon::{
    node_daemon_socket_path, CONTROL_TRANSIENT_READ_BACKOFF, CONTROL_TRANSIENT_READ_RETRIES,
    NODE_DAEMON_STOP_DRAIN_BOUND,
};
use crate::{current_unix_ms_u64, CliError, CliResult, HarnessStore, NativeSessionWakeUpdate};
use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// Send an exact Execution Space + TeamRun start request to the local Node.
/// Returns the response line on success.
#[derive(Debug)]
pub(crate) struct NodeDaemonStartRequestError {
    source: std::io::Error,
    request_may_have_been_accepted: bool,
}

impl NodeDaemonStartRequestError {
    fn before_send(source: std::io::Error) -> Self {
        Self {
            source,
            request_may_have_been_accepted: false,
        }
    }

    fn after_send(source: std::io::Error) -> Self {
        Self {
            source,
            request_may_have_been_accepted: true,
        }
    }

    pub(crate) fn request_may_have_been_accepted(&self) -> bool {
        self.request_may_have_been_accepted
    }
}

impl std::fmt::Display for NodeDaemonStartRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

pub(crate) fn try_delegate_to_node_daemon(
    firm_home: &Path,
    node_id: &str,
    execution_space_id: &str,
    run_id: &str,
) -> Result<String, NodeDaemonStartRequestError> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream =
        UnixStream::connect(&socket_path).map_err(NodeDaemonStartRequestError::before_send)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(NodeDaemonStartRequestError::before_send)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(NodeDaemonStartRequestError::before_send)?;

    let cmd = serde_json::json!({
        "cmd": "start",
        "execution_space_id": execution_space_id,
        "run_id": run_id
    });
    let cmd_str = serde_json::to_string(&cmd)
        .map_err(std::io::Error::other)
        .map_err(NodeDaemonStartRequestError::before_send)?;
    // From the first write attempt onward, the daemon may have accepted the
    // complete newline-delimited request even when the client observes only a
    // later transport error. Every such result must be reconciled, never
    // classified as NotApplied or blindly retried.
    writeln!(stream, "{cmd_str}").map_err(NodeDaemonStartRequestError::after_send)?;
    stream
        .flush()
        .map_err(NodeDaemonStartRequestError::after_send)?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    read_control_response_line(
        &mut reader,
        &mut buf,
        CONTROL_TRANSIENT_READ_RETRIES,
        CONTROL_TRANSIENT_READ_BACKOFF,
    )
    .map_err(NodeDaemonStartRequestError::after_send)?;
    if buf.trim().is_empty() || !buf.ends_with('\n') {
        return Err(NodeDaemonStartRequestError::after_send(
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "NodeDaemon closed before returning a start result",
            ),
        ));
    }
    serde_json::from_str::<serde_json::Value>(buf.trim()).map_err(|error| {
        NodeDaemonStartRequestError::after_send(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("NodeDaemon returned invalid start JSON: {error}"),
        ))
    })?;
    Ok(buf.trim().to_string())
}

/// Send a status request to the machine NodeDaemon.
pub(crate) fn daemon_status_via_socket(firm_home: &Path, node_id: &str) -> Option<String> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .ok()?;

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

/// Wait for one fd readiness event, with the wait always computed from the
/// SAME original deadline the caller passes in — never a fresh relative
/// timeout per syscall. A spurious wakeup or EINTR simply waits again for
/// what is actually left; the deadline expiring yields no observation.
#[cfg(unix)]
fn poll_fd_ready(
    fd: std::os::unix::io::RawFd,
    events: libc::c_short,
    deadline: std::time::Instant,
) -> Option<()> {
    loop {
        let remaining = deadline.checked_duration_since(std::time::Instant::now())?;
        if remaining.is_zero() {
            return None;
        }
        let mut pollfd = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let timeout_ms =
            i32::try_from(remaining.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        // SAFETY: `pollfd` points at one valid pollfd for this call only.
        let polled = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if polled < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        if polled == 0 {
            return None;
        }
        return Some(());
    }
}

/// Connect a Unix-domain socket with the wait governed by the caller's
/// deadline (`Instant`, computed once by the caller). socket2 owns the fd
/// lifecycle and provides the mature semantics this boundary needs: the fd
/// is created non-blocking with CLOEXEC so it cannot leak into a provider
/// exec, and `SockAddr::unix` validates the actual platform `sun_path`
/// length. Interior NUL bytes are rejected explicitly below before the
/// socket is even created — the library only length-checks and copies
/// bytes, so without this check a crafted `path\0suffix` could reach the
/// truncated prefix endpoint. Only EINPROGRESS proceeds to a writability
/// wait plus the SO_ERROR check: a backlog EAGAIN is NOT a connection in
/// progress, and SO_ERROR==0 after one would prove nothing, so any other
/// connect error returns no observation rather than an uncertain
/// connection.
#[cfg(unix)]
fn control_socket_connect_deadline(
    socket_path: &Path,
    deadline: std::time::Instant,
) -> Option<UnixStream> {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::io::AsRawFd as _;
    if socket_path.as_os_str().as_bytes().contains(&0) {
        return None;
    }
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).ok()?;
    socket.set_nonblocking(true).ok()?;
    socket.set_cloexec(true).ok()?;
    let address = socket2::SockAddr::unix(socket_path).ok()?;
    match socket.connect(&address) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(libc::EINPROGRESS) => {
            poll_fd_ready(socket.as_raw_fd(), libc::POLLOUT, deadline)?;
            let socket_error = socket.take_error().ok()?;
            match socket_error {
                None => {}
                Some(error) if error.raw_os_error() == Some(libc::EINPROGRESS) => {
                    poll_fd_ready(socket.as_raw_fd(), libc::POLLOUT, deadline)?;
                    if socket.take_error().ok()?.is_some() {
                        return None;
                    }
                }
                Some(_) => return None,
            }
        }
        Err(_) => return None,
    }
    Some(UnixStream::from(std::os::fd::OwnedFd::from(socket)))
}

/// One monotonic deadline across connect, write, and read of a single-line
/// control response, computed once from `io_budget` and shared by every
/// wait. The socket stays non-blocking throughout: the request is written
/// with an explicit offset and the response read in chunks, each preceded
/// by a readiness wait recomputed from the same original `Instant` — no
/// `writeln!`/`write_all`/`read_line` loop whose per-syscall relative
/// timeout a partial write or drip could re-arm indefinitely. A frame that
/// is incomplete when the deadline passes is discarded, and the completed
/// frame is checked against the socket deadline once more before return:
/// bytes that complete only after it are never parsed as proof. The
/// deadline governs socket waiting only; arbitrary OS scheduling delay
/// around the calls is outside any cancellable budget, so this is not a
/// hard process wall-clock limit.
pub(crate) fn control_socket_request_line_bounded(
    socket_path: &Path,
    request: &str,
    io_budget: Duration,
) -> Option<String> {
    use std::io::{Read as _, Write as _};
    use std::os::unix::io::AsRawFd as _;
    let deadline = std::time::Instant::now().checked_add(io_budget)?;
    let mut stream = control_socket_connect_deadline(socket_path, deadline)?;
    let mut frame = request.as_bytes().to_vec();
    frame.push(b'\n');
    let mut offset = 0;
    while offset < frame.len() {
        poll_fd_ready(stream.as_raw_fd(), libc::POLLOUT, deadline)?;
        match stream.write(&frame[offset..]) {
            Ok(0) => return None,
            Ok(written) => offset += written,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => return None,
        }
    }
    let mut response = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        poll_fd_ready(stream.as_raw_fd(), libc::POLLIN, deadline)?;
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => {
                response.extend_from_slice(&chunk[..n]);
                if chunk[..n].contains(&b'\n') {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => return None,
        }
    }
    if std::time::Instant::now() > deadline {
        return None;
    }
    let response = String::from_utf8(response).ok()?;
    let response = response.trim().to_string();
    (!response.is_empty()).then_some(response)
}

/// Status request whose socket I/O is bounded by the caller's remaining
/// budget. The reserved status lane stays responsive while other lanes are
/// busy, so a bounded read observes instead of waiting on the start gate.
pub(crate) fn daemon_status_via_socket_bounded(
    firm_home: &Path,
    node_id: &str,
    io_budget: Duration,
) -> Option<String> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    control_socket_request_line_bounded(&socket_path, r#"{"cmd":"status"}"#, io_budget)
}

pub(crate) use crate::daemon_protocol::NativeSessionWakePostError;
pub(crate) fn post_native_session_wake(
    endpoint: &NativeSessionWakeEndpoint,
    execution_space_id: &str,
    update: &NativeSessionWakeUpdate,
) -> Result<(), NativeSessionWakePostError> {
    let mut stream = std::net::TcpStream::connect(&endpoint.authority)?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let body = serde_json::to_vec(update).map_err(std::io::Error::other)?;
    let encoded_space = execution_space_id.replace('%', "%25").replace(' ', "%20");
    write!(
        stream,
        "POST /v1/live/native-session-wake?space={encoded_space} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nX-AgentFirm-Native-Session-Wake-Token: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.authority,
        endpoint.token,
        body.len(),
    )?;
    stream.write_all(&body)?;
    stream.flush()?;
    let mut status_line = String::new();
    std::io::BufReader::new(&mut stream).read_line(&mut status_line)?;
    if !status_line.contains(" 202 ") {
        return Err(NativeSessionWakePostError::Rejected(
            status_line.trim().to_string(),
        ));
    }
    Ok(())
}

/// Register the current `serve` process as the volatile live-activity sink.
/// A missing daemon is not an error: serve remains usable and a later restart
/// registers again. The endpoint is loopback-only and never durable.
pub(crate) struct NativeSessionWakeRegistration<'a> {
    pub authority: &'a str,
    pub token: &'a str,
    pub agent_member_id: &'a str,
    pub expected_daemon_instance_id: &'a str,
    pub serve_instance_id: &'a str,
}

pub(crate) fn register_native_session_wake_via_socket(
    firm_home: &Path,
    node_id: &str,
    registration: NativeSessionWakeRegistration<'_>,
) -> Option<Result<String, std::io::Error>> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = match UnixStream::connect(&socket_path) {
        Ok(stream) => stream,
        Err(_) => return None,
    };
    if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        return Some(Err(error));
    }
    if let Err(error) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
        return Some(Err(error));
    }
    let command = serde_json::json!({
        "cmd": "register_native_session_wake",
        "authority": registration.authority,
        "token": registration.token,
        "agent_member_id": registration.agent_member_id,
        "expected_daemon_instance_id": registration.expected_daemon_instance_id,
        "serve_instance_id": registration.serve_instance_id,
    });
    if let Err(error) = writeln!(stream, "{command}") {
        return Some(Err(error));
    }
    if let Err(error) = stream.flush() {
        return Some(Err(error));
    }
    let mut response = String::new();
    if let Err(error) = std::io::BufReader::new(&mut stream).read_line(&mut response) {
        return Some(Err(error));
    }
    Some(Ok(response.trim().to_string()))
}

/// Send an authenticated runtime command to the one local NodeDaemon. The
/// caller receives only the daemon's fenced result; it never mutates provider
/// or session ledgers directly.
pub(crate) fn runtime_command_via_socket(
    firm_home: &Path,
    node_id: &str,
    envelope: &harness_core::agentfirm_api::ControlCommandEnvelope,
) -> Result<serde_json::Value, std::io::Error> {
    runtime_command_via_socket_with_policy(
        firm_home,
        node_id,
        envelope,
        ControlSocketReadPolicy {
            timeout: Duration::from_secs(10),
            transient_retries: CONTROL_TRANSIENT_READ_RETRIES,
            retry_backoff: CONTROL_TRANSIENT_READ_BACKOFF,
        },
    )
}

#[derive(Clone, Copy)]
struct ControlSocketReadPolicy {
    pub(crate) timeout: Duration,
    pub(crate) transient_retries: usize,
    pub(crate) retry_backoff: Duration,
}

fn runtime_command_via_socket_with_policy(
    firm_home: &Path,
    node_id: &str,
    envelope: &harness_core::agentfirm_api::ControlCommandEnvelope,
    read_policy: ControlSocketReadPolicy,
) -> Result<serde_json::Value, std::io::Error> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.set_read_timeout(Some(read_policy.timeout))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let command = serde_json::json!({"cmd": "runtime", "envelope": envelope});
    writeln!(
        stream,
        "{}",
        serde_json::to_string(&command).map_err(std::io::Error::other)?
    )?;
    stream.flush()?;
    let mut line = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    read_control_response_line(
        &mut reader,
        &mut line,
        read_policy.transient_retries,
        read_policy.retry_backoff,
    )?;
    if line.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "NodeDaemon returned an empty runtime response",
        ));
    }
    serde_json::from_str(line.trim()).map_err(std::io::Error::other)
}

pub(crate) fn read_control_response_line(
    reader: &mut impl BufRead,
    line: &mut String,
    max_transient_retries: usize,
    retry_backoff: Duration,
) -> Result<(), std::io::Error> {
    // A read timeout is post-send and the daemon may still be applying the
    // command. Keep waiting on this connection; reconnecting or rewriting the
    // frame would turn a transport delay into an ambiguous duplicate effect.
    let mut transient_retries = 0;
    loop {
        match reader.read_line(line) {
            Ok(_) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && transient_retries < max_transient_retries =>
            {
                transient_retries += 1;
                std::thread::sleep(retry_backoff);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Read one bounded provider-owned Session page through the exact local
/// NodeDaemon. The response never contains a filesystem path and the daemon
/// revalidates placement, lease generation, viewer scope and native identity.
pub(crate) fn native_session_read_via_socket(
    firm_home: &Path,
    node_id: &str,
    request: &crate::provider_event_api::PersistedSessionReadRequest,
) -> Result<crate::provider_event_api::PersistedSessionReadResponse, std::io::Error> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let command = serde_json::json!({"cmd": "read_native_session", "request": request});
    writeln!(
        stream,
        "{}",
        serde_json::to_string(&command).map_err(std::io::Error::other)?
    )?;
    stream.flush()?;
    let mut line = String::new();
    std::io::BufReader::new(&mut stream).read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "NodeDaemon returned an empty persisted Session response",
        ));
    }
    let envelope: serde_json::Value =
        serde_json::from_str(line.trim()).map_err(std::io::Error::other)?;
    if envelope["ok"] != true {
        return Err(std::io::Error::other(
            envelope["error"]
                .as_str()
                .unwrap_or("NodeDaemon rejected persisted Session read"),
        ));
    }
    serde_json::from_value(envelope["response"].clone()).map_err(std::io::Error::other)
}

/// Start a NodeDaemon for an exact observed predecessor generation. Unlike the
/// convenience CLI helper, this never treats an already-running or concurrently
/// winning daemon as the requested external effect. The child PID in the
/// server-owned status response must be the process spawned by this request.
pub(crate) fn start_daemon_process_fenced(
    firm_home: &Path,
    node_id: &str,
    max_concurrency: usize,
    execution_space_id: &str,
    observed_generation: u64,
) -> CliResult<String> {
    if max_concurrency == 0 {
        return Err(CliError::Usage(
            "daemon max_concurrency must be greater than zero".into(),
        ));
    }
    if daemon_status_via_socket(firm_home, node_id).is_some() {
        return Err(CliError::Usage(
            "SUPERVISOR_GENERATION_FENCED: a NodeDaemon is already live".into(),
        ));
    }
    let space = crate::execution_space::context_for_id(firm_home, execution_space_id)
        .map_err(|error| CliError::Usage(error.to_string()))?
        .ok_or_else(|| {
            CliError::Usage(format!("Execution Space not found: {execution_space_id}"))
        })?;
    let store = HarnessStore::new(space.store_root);
    let current_generation = store
        .latest_node_daemon_lease(node_id)?
        .map(|lease| lease.generation)
        .unwrap_or(0);
    if current_generation != observed_generation {
        return Err(CliError::Usage(format!(
            "SUPERVISOR_GENERATION_FENCED: observed generation {observed_generation}, current generation {current_generation}"
        )));
    }
    let (log_path, stdout, stderr) =
        crate::daemon_cli::node_daemon_log_streams(firm_home, node_id)?;
    let executable = std::env::current_exe().map_err(|error| {
        CliError::Usage(format!(
            "cannot resolve NodeDaemon executable (log: {}): {error}",
            log_path.display()
        ))
    })?;
    let mut command = std::process::Command::new(executable);
    command
        .arg("daemon")
        .arg("serve")
        .arg("--max-concurrency")
        .arg(max_concurrency.to_string())
        .arg("--idle-timeout-secs")
        .arg(
            harness_runtime_contract::CycleTimeouts::DEFAULT_INPUT_ACCEPTANCE
                .as_secs()
                .to_string(),
        )
        .arg("--scan-interval-secs")
        .arg("5")
        .stdin(std::process::Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| {
        CliError::Usage(format!(
            "cannot start NodeDaemon (log: {}): {error}",
            log_path.display()
        ))
    })?;
    // Recovery/adoption can legitimately take longer than a trivial socket
    // bind. A failed start must not leave a child that later acquires the
    // NodeDaemon generation behind the caller's back.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = daemon_status_via_socket(firm_home, node_id) {
            let status_value = serde_json::from_str::<serde_json::Value>(&status).ok();
            let process_id = status_value
                .as_ref()
                .and_then(|value| value["process_id"].as_u64());
            if process_id == Some(u64::from(child.id())) {
                let instance_id = status_value
                    .as_ref()
                    .and_then(|value| value["instance_id"].as_str());
                let lease = store.latest_node_daemon_lease(node_id)?;
                if lease.as_ref().is_some_and(|lease| {
                    lease.daemon_id == format!("node-daemon:{node_id}")
                        && Some(lease.instance_id.as_str()) == instance_id
                        && lease.generation > observed_generation
                        && lease.status == harness_core::NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > current_unix_ms_u64()
                }) {
                    return crate::daemon_cli::daemon_status_with_log_path(&status, &log_path);
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            return Err(CliError::Usage(
                "SUPERVISOR_GENERATION_FENCED: another NodeDaemon generation won startup".into(),
            ));
        }
        if let Some(status) = child.try_wait()? {
            return Err(crate::daemon_cli::daemon_start_failure(
                child.id(),
                &format!("exited before acquiring generation ({status})"),
                &log_path,
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::daemon_cli::daemon_start_failure(
                child.id(),
                "did not become ready within 60s and was stopped",
                &log_path,
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Send a stop command to the multi-team daemon.
pub(crate) fn daemon_stop_via_socket(
    firm_home: &Path,
    node_id: &str,
    execution_space_id: &str,
    daemon_generation: u64,
) -> Option<String> {
    daemon_stop_via_socket_with_policy(
        firm_home,
        node_id,
        execution_space_id,
        daemon_generation,
        ControlSocketReadPolicy {
            timeout: NODE_DAEMON_STOP_DRAIN_BOUND.saturating_add(Duration::from_secs(25)),
            transient_retries: CONTROL_TRANSIENT_READ_RETRIES,
            retry_backoff: CONTROL_TRANSIENT_READ_BACKOFF,
        },
    )
}

fn daemon_stop_via_socket_with_policy(
    firm_home: &Path,
    node_id: &str,
    execution_space_id: &str,
    daemon_generation: u64,
    read_policy: ControlSocketReadPolicy,
) -> Option<String> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    // Stop answers with the drain result, not with its acceptance, so the
    // client must outwait the documented drain bound plus the bounded
    // settle/release writes that follow it (#584).
    stream.set_read_timeout(Some(read_policy.timeout)).ok()?;

    let cmd = serde_json::json!({
        "cmd": "stop",
        "execution_space_id": execution_space_id,
        "daemon_generation": daemon_generation,
    });
    writeln!(stream, "{cmd}").ok()?;
    stream.flush().ok()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    read_control_response_line(
        &mut reader,
        &mut buf,
        read_policy.transient_retries,
        read_policy.retry_backoff,
    )
    .ok()?;
    Some(buf.trim().to_string())
}

pub(crate) fn reconcile_team_run_start_postcondition(
    store: &HarnessStore,
    status_json: &str,
    node_id: &str,
    execution_space_id: &str,
    run_id: &str,
) -> Option<CliResult<serde_json::Value>> {
    let status = serde_json::from_str::<serde_json::Value>(status_json).ok()?;
    if status["ok"].as_bool() != Some(true) || status["node_id"].as_str() != Some(node_id) {
        return None;
    }
    let instance_id = status["instance_id"].as_str()?;
    let process_id = status["process_id"].as_u64()?;
    let process_id = u32::try_from(process_id).ok()?;
    let matching_runs = status["runs"]
        .as_array()?
        .iter()
        .filter(|candidate| {
            candidate["execution_space_id"].as_str() == Some(execution_space_id)
                && candidate["run_id"].as_str() == Some(run_id)
                && candidate["status"].as_str() == Some("running")
        })
        .collect::<Vec<_>>();
    let [run] = matching_runs.as_slice() else {
        return None;
    };
    let daemon_generation = run["daemon_generation"].as_u64()?;
    let project_binding_id = run["project_binding_id"].as_str()?;
    let supervisor_id = run["supervisor_id"].as_str()?;
    let supervisor_generation = run["supervisor_generation"].as_u64()?;
    let now = current_unix_ms_u64();
    let daemon = match store.latest_node_daemon_lease(node_id) {
        Ok(Some(daemon)) => daemon,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    let supervisor = match store.latest_team_supervisor_lease(run_id) {
        Ok(Some(supervisor)) => supervisor,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    let exact = start_postcondition_matches(
        &daemon,
        &supervisor,
        node_id,
        instance_id,
        daemon_generation,
        execution_space_id,
        project_binding_id,
        run_id,
        supervisor_id,
        supervisor_generation,
        process_id,
        now,
    );
    exact.then(|| {
        Ok(serde_json::json!({
            "node_id": node_id,
            "execution_space_id": execution_space_id,
            "team_run_id": run_id,
            "daemon_response": {
                "ok": true,
                "reconciled_after_transport_error": true,
                "daemon_generation": daemon_generation,
                "supervisor_id": supervisor_id,
                "supervisor_generation": supervisor_generation,
            },
        }))
    })
}

/// Client observation policy after an ambiguous start send: how long the CLI
/// may keep reading the reserved status lane for exact postcondition proof.
/// This is a named client observation budget, not an inferred bound on
/// serialized boot adoption; exhausting it preserves the honest UNKNOWN.
pub(crate) const TEAM_RUN_START_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(60);
/// Interval between status observations. Each sleep is clamped to the
/// remaining observation budget and each status I/O is bounded by it.
pub(crate) const TEAM_RUN_START_OBSERVATION_INTERVAL: Duration = Duration::from_secs(1);

/// Bounded observation opportunity after a start request whose transport
/// failed post-send: poll the reserved status lane until the exact
/// postcondition proves, the monotonic wall-clock deadline passes, or the
/// status evidence itself fails. Daemon-alive plus a missing run proves only
/// the absence of success evidence — never that adoption is in progress or
/// guaranteed to finish — so an unreachable, malformed, or untrusted status
/// ends the observation promptly, and an authoritative reconcile error
/// returns without retry. Every status I/O is bounded by the remaining
/// budget, no poll launches once the deadline has passed, and each sleep is
/// clamped to what remains.
///
/// The deadline bounds how long we look, never what we found: a
/// postcondition that `prove_postcondition` already proved is returned
/// without a post-proof clock check, because a store-verified proof must not
/// decay into a false UNKNOWN merely because time passed while looking.
/// Explicitly outside the cancellable budget: the synchronous canonical
/// lease reads inside the proof are uncancellable local filesystem reads,
/// and arbitrary OS scheduling delay surrounds every call — this is a named
/// client observation policy, not a hard process wall-clock limit.
pub(crate) fn reconcile_team_run_start_with_observation(
    node_id: &str,
    observation_budget: Duration,
    poll_interval: Duration,
    fetch_status: &mut dyn FnMut(Duration) -> Option<String>,
    prove_postcondition: &mut dyn FnMut(&str) -> Option<CliResult<serde_json::Value>>,
    now: &mut dyn FnMut() -> std::time::Instant,
    sleep: &mut dyn FnMut(Duration),
) -> Option<CliResult<serde_json::Value>> {
    let deadline = now().checked_add(observation_budget)?;
    loop {
        let remaining = deadline.checked_duration_since(now())?;
        if remaining.is_zero() {
            return None;
        }
        let status = fetch_status(remaining)?;
        let envelope = serde_json::from_str::<serde_json::Value>(&status).ok()?;
        if envelope["ok"].as_bool() != Some(true) || envelope["node_id"].as_str() != Some(node_id) {
            return None;
        }
        if let Some(proved) = prove_postcondition(&status) {
            return Some(proved);
        }
        let remaining = deadline.checked_duration_since(now())?;
        if remaining.is_zero() {
            return None;
        }
        sleep(remaining.min(poll_interval));
    }
}

#[allow(clippy::too_many_arguments)]
fn start_postcondition_matches(
    daemon: &harness_core::NodeDaemonLease,
    supervisor: &harness_core::TeamSupervisorLease,
    node_id: &str,
    instance_id: &str,
    daemon_generation: u64,
    execution_space_id: &str,
    project_binding_id: &str,
    run_id: &str,
    supervisor_id: &str,
    supervisor_generation: u64,
    process_id: u32,
    now: u64,
) -> bool {
    daemon.node_id == node_id
        && daemon.daemon_id == format!("node-daemon:{node_id}")
        && daemon.instance_id == instance_id
        && daemon.generation == daemon_generation
        && daemon.status == harness_core::NodeDaemonLeaseStatus::Active
        && daemon.expires_unix_ms > now
        && supervisor.team_run_id == run_id
        && supervisor.node_id == node_id
        && supervisor.node_daemon_id == daemon.daemon_id
        && supervisor.node_daemon_generation == daemon.generation
        && supervisor.execution_space_id == execution_space_id
        && supervisor.project_binding_id == project_binding_id
        && supervisor.supervisor_id == supervisor_id
        && supervisor.generation == supervisor_generation
        && supervisor.owner_process_id == process_id
        && supervisor.status == harness_core::TeamSupervisorLeaseStatus::Active
        && supervisor.expires_unix_ms > now
}

#[cfg(test)]
mod bounded_status_tests;
#[cfg(test)]
mod observation_tests;
#[cfg(test)]
mod retry_tests;
