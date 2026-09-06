//! Test-client deadlines and bounded, payload-free failure diagnostics (#874).
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{ffi::OsStrExt, net::UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};

const ORDINARY_BUDGET: Duration = Duration::from_secs(3);
// Production stop replies only after 30 s cooperative + 5 s forced drain.
// Keep five seconds for socket scheduling/response delivery; do not apply this
// to start/status, whose hang would otherwise be hidden by the stop allowance.
const STOP_BUDGET: Duration = Duration::from_secs(40);

struct Context<'a> {
    socket: &'a Path,
    command: &'static str,
    started: Instant,
    budget: Duration,
}

impl Context<'_> {
    fn failure(&self, stage: &str, cause: impl std::fmt::Display) -> String {
        format!("daemon request failed: cmd={} socket={} stage={stage} elapsed_ms={} budget_ms={} cause={cause}",
            self.command, self.socket.display(), self.started.elapsed().as_millis(), self.budget.as_millis())
    }

    fn wait(&self, stream: &UnixStream, events: i16, stage: &str) -> Result<(), String> {
        loop {
            let left = self
                .budget
                .checked_sub(self.started.elapsed())
                .filter(|left| !left.is_zero())
                .ok_or_else(|| self.failure(stage, "deadline exceeded"))?;
            let mut fd = libc::pollfd {
                fd: stream.as_raw_fd(),
                events,
                revents: 0,
            };
            // Round up so a fractional millisecond does not become busy polling.
            let millis = left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32;
            // SAFETY: fd points to one initialized pollfd for the duration of poll.
            let result = unsafe { libc::poll(&mut fd, 1, millis) };
            if result > 0 {
                return Ok(());
            }
            if result == 0 {
                return Err(self.failure(stage, "deadline exceeded"));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(self.failure(stage, error.kind()));
            }
        }
    }
}

fn connect(context: &Context<'_>) -> Result<UnixStream, String> {
    // Use a nonblocking connect so a full local listen backlog also obeys the
    // request deadline. The resulting fd is owned immediately, including errors.
    // SAFETY: socket returns a new descriptor or -1, with no borrowed memory.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(context.failure("connect", io::Error::last_os_error().kind()));
    }
    // SAFETY: fd is a newly created socket whose sole owner becomes this stream.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: fd remains owned by stream; do not leak the fixture connection
    // into provider/daemon children spawned by other integration tests.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(context.failure("connect", io::Error::last_os_error().kind()));
    }
    stream
        .set_nonblocking(true)
        .map_err(|e| context.failure("connect", e.kind()))?;
    // SAFETY: sockaddr_un is a plain C socket-address struct; zero is valid.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let path = context.socket.as_os_str().as_bytes();
    if path.len() >= address.sun_path.len() || path.contains(&0) {
        return Err(context.failure("connect", "invalid socket path"));
    }
    for (target, byte) in address.sun_path.iter_mut().zip(path) {
        *target = *byte as libc::c_char;
    }
    let size = std::mem::size_of_val(&address) as libc::socklen_t;
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        address.sun_len = size as u8;
    }
    // SAFETY: address and size describe a live initialized sockaddr_un.
    let result = unsafe { libc::connect(fd, (&address as *const libc::sockaddr_un).cast(), size) };
    if result != 0 {
        let error = io::Error::last_os_error();
        // AF_UNIX EAGAIN means a full backlog, not an established connection.
        // Report it immediately instead of pretending POLLOUT completed connect.
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(context.failure("connect", error.kind()));
        }
        context.wait(&stream, libc::POLLOUT, "connect")?;
        if let Some(error) = stream
            .take_error()
            .map_err(|e| context.failure("connect", e.kind()))?
        {
            return Err(context.failure("connect", error.kind()));
        }
    }
    Ok(stream)
}

pub(super) fn request(socket: &Path, request: &str) -> Result<serde_json::Value, String> {
    // Do not echo arbitrary cmd values, request JSON, response bodies or env.
    let command = match serde_json::from_str::<serde_json::Value>(request)
        .ok()
        .and_then(|value| value["cmd"].as_str().map(str::to_owned))
        .as_deref()
    {
        Some("stop") => "stop",
        Some("start") => "start",
        Some("status") => "status",
        Some(_) => "unknown",
        None => "invalid",
    };
    let context = Context {
        socket,
        command,
        started: Instant::now(),
        budget: if command == "stop" {
            STOP_BUDGET
        } else {
            ORDINARY_BUDGET
        },
    };
    exchange(&context, request)
}

fn exchange(context: &Context<'_>, request: &str) -> Result<serde_json::Value, String> {
    let mut stream = connect(context)?;
    let payload = format!("{request}\n");
    let mut remaining = payload.as_bytes();
    while !remaining.is_empty() {
        context.wait(&stream, libc::POLLOUT, "write")?;
        match stream.write(remaining) {
            Ok(0) => return Err(context.failure("write", "zero-byte write")),
            Ok(count) => remaining = &remaining[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(context.failure("write", error.kind())),
        }
    }
    let mut response = Vec::new();
    loop {
        context.wait(&stream, libc::POLLIN, "read")?;
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) if response.is_empty() => return Err(context.failure("empty_response", "EOF")),
            Ok(0) => return Err(context.failure("read", "EOF before newline")),
            Ok(count) => {
                response.extend_from_slice(&chunk[..count]);
                if let Some(end) = response.iter().position(|byte| *byte == b'\n') {
                    if response[..end].iter().all(u8::is_ascii_whitespace) {
                        return Err(context.failure("empty_response", "blank line"));
                    }
                    return serde_json::from_slice(&response[..end]).map_err(|error| {
                        context.failure(
                            "json",
                            format!(
                                "{:?} at line {} column {}",
                                error.classify(),
                                error.line(),
                                error.column()
                            ),
                        )
                    });
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(context.failure("read", error.kind())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn socket() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "firm-874-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn serve(response: &[u8], delay: Duration, command: &str) -> Result<serde_json::Value, String> {
        let path = socket();
        let listener = UnixListener::bind(&path).unwrap();
        let result = std::thread::scope(|scope| {
            let server = scope.spawn(|| {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                std::io::BufReader::new(&mut stream)
                    .read_line(&mut request)
                    .unwrap();
                std::thread::sleep(delay);
                // A bounded client may close while this deliberately slow server waits.
                let _ = stream.write_all(response);
            });
            let result = request(&path, command);
            server.join().unwrap();
            result
        });
        std::fs::remove_file(path).unwrap();
        result
    }

    #[test]
    fn stop_waits_for_delayed_drain_beyond_the_ordinary_deadline() {
        assert_eq!(STOP_BUDGET, Duration::from_secs(40));
        let response = serve(
            b"{\"ok\":true}\n",
            ORDINARY_BUDGET + Duration::from_millis(150),
            r#"{"cmd":"stop"}"#,
        )
        .unwrap();
        assert_eq!(response["ok"], true);
    }

    #[test]
    fn status_remains_bounded_and_names_its_timeout() {
        let error = serve(
            b"{\"ok\":true}\n",
            ORDINARY_BUDGET + Duration::from_millis(150),
            r#"{"cmd":"status"}"#,
        )
        .unwrap_err();
        assert_diagnostic(&error, "status", "read", 3000);
        assert!(error.contains("deadline exceeded"));
    }

    fn assert_diagnostic(error: &str, command: &str, stage: &str, budget: u64) {
        assert!(error.contains(&format!("cmd={command} ")), "{error}");
        assert!(error.contains("socket="), "{error}");
        assert!(error.contains(&format!("stage={stage} ")), "{error}");
        assert!(error.contains("elapsed_ms="), "{error}");
        assert!(error.contains(&format!("budget_ms={budget} ")), "{error}");
        assert!(!error.contains("private-token"), "payload leaked: {error}");
    }

    #[test]
    fn connect_eof_and_json_failures_identify_request_without_payloads() {
        let path = socket();
        let error = request(&path, r#"{"cmd":"start","token":"private-token"}"#).unwrap_err();
        assert_diagnostic(&error, "start", "connect", 3000);
        assert!(error.contains(&path.display().to_string()));
        for (response, stage) in [
            (&b""[..], "empty_response"),
            (&b" \n"[..], "empty_response"),
            (&b"private-token\n"[..], "json"),
            (&b"{\"secret\":\"private-token\"}"[..], "read"),
        ] {
            let error = serve(
                response,
                Duration::ZERO,
                r#"{"cmd":"start","token":"private-token"}"#,
            )
            .unwrap_err();
            assert_diagnostic(&error, "start", stage, 3000);
        }
        let error = request(&path, r#"{"cmd":"private-token"}"#).unwrap_err();
        assert_diagnostic(&error, "unknown", "connect", 3000);
    }

    #[test]
    fn a_nonreading_peer_cannot_leave_writes_unbounded() {
        let path = socket();
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::scope(|scope| {
            let server = scope.spawn(|| {
                let (_stream, _) = listener.accept().unwrap();
                std::thread::sleep(Duration::from_millis(200));
            });
            // Same exchange path with a short injected budget; a payload larger
            // than a Unix socket send buffer forces the write readiness wait.
            let context = Context {
                socket: &path,
                command: "start",
                started: Instant::now(),
                budget: Duration::from_millis(50),
            };
            let error = exchange(&context, &"private-token".repeat(1_000_000)).unwrap_err();
            assert_diagnostic(&error, "start", "write", 50);
            server.join().unwrap();
        });
        std::fs::remove_file(path).unwrap();
    }
}
