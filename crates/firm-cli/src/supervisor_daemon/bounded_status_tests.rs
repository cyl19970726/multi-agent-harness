//! Regression tests for `control_socket_request_line_bounded`: one monotonic
//! deadline across connect, write, and read of a single-line control
//! response, with post-deadline bytes never parsed as proof.

use super::*;

fn bounded_request_socket_path(label: &str) -> PathBuf {
    Path::new("/tmp").join(format!(
        "firm-bst-{label}-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn serve_once(socket_path: &Path, respond: impl FnOnce(UnixStream) + Send + 'static) {
    let listener = UnixListener::bind(socket_path).expect("bind bounded test socket");
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept bounded test client");
        respond(stream);
    });
}

#[test]
fn bounded_request_reads_a_complete_line_promptly() {
    let socket = bounded_request_socket_path("complete");
    serve_once(&socket, |mut stream| {
        use std::io::Write as _;
        writeln!(stream, "{{\"ok\":true}}").expect("write complete line");
    });
    let response =
        control_socket_request_line_bounded(&socket, r#"{"cmd":"status"}"#, Duration::from_secs(5));
    let _ = std::fs::remove_file(&socket);
    assert_eq!(response.as_deref(), Some("{\"ok\":true}"));
}

#[test]
fn bounded_request_refuses_a_missing_socket_promptly() {
    let socket = bounded_request_socket_path("missing");
    let started = std::time::Instant::now();
    let response =
        control_socket_request_line_bounded(&socket, r#"{"cmd":"status"}"#, Duration::from_secs(5));
    assert!(response.is_none());
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "a missing socket must not consume the budget: {:?}",
        started.elapsed()
    );
}

#[test]
fn bounded_request_cuts_a_partial_frame_drip_at_the_deadline() {
    let socket = bounded_request_socket_path("drip");
    serve_once(&socket, |mut stream| {
        use std::io::{Read as _, Write as _};
        let mut request = [0u8; 64];
        let _ = stream.read(&mut request);
        // Drip one byte per 50 ms without a newline: per-operation timeouts
        // would let this run forever, the single deadline must not.
        for byte in b"{\"ok\":true,partial".iter() {
            stream.write_all(&[*byte]).expect("drip byte");
            stream.flush().expect("flush drip byte");
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_secs(2));
    });
    let started = std::time::Instant::now();
    let response = control_socket_request_line_bounded(
        &socket,
        r#"{"cmd":"status"}"#,
        Duration::from_millis(300),
    );
    let elapsed = started.elapsed();
    let _ = std::fs::remove_file(&socket);
    assert!(
        response.is_none(),
        "a partial frame drip must never be parsed: {response:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the drip outlived the single deadline: {elapsed:?}"
    );
}

#[test]
fn bounded_request_never_parses_proof_completed_after_the_deadline() {
    let socket = bounded_request_socket_path("late-proof");
    serve_once(&socket, |mut stream| {
        use std::io::{Read as _, Write as _};
        let mut request = [0u8; 64];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"{\"ok\":true,\"runs\":[]")
            .expect("write partial proof");
        stream.flush().expect("flush partial proof");
        // Complete the valid frame only after the client's deadline: those
        // bytes are not proof and must be discarded with the partial frame.
        std::thread::sleep(Duration::from_millis(600));
        stream.write_all(b"}\n").expect("complete late frame");
        stream.flush().expect("flush late frame");
        std::thread::sleep(Duration::from_millis(200));
    });
    let started = std::time::Instant::now();
    let response = control_socket_request_line_bounded(
        &socket,
        r#"{"cmd":"status"}"#,
        Duration::from_millis(300),
    );
    let elapsed = started.elapsed();
    let _ = std::fs::remove_file(&socket);
    assert!(
        response.is_none(),
        "a frame completed after the deadline is not proof: {response:?}"
    );
    assert!(
        elapsed < Duration::from_millis(550),
        "the client must stop at the deadline instead of reading the late completion: {elapsed:?}"
    );
}

#[test]
fn bounded_request_refuses_a_saturated_accept_backlog_promptly() {
    let socket = bounded_request_socket_path("backlog");
    let listener = UnixListener::bind(&socket).expect("bind backlog socket");
    let address = socket2::SockAddr::unix(&socket).expect("backlog SockAddr");
    // Fill the accept backlog with non-blocking connects. Once it is full a
    // blocking connect would hang; the helper must return no observation
    // promptly instead of treating a backlog EAGAIN as a connection in
    // progress.
    let mut fillers = Vec::new();
    for _ in 0..8192 {
        let filler = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)
            .expect("filler socket");
        filler.set_nonblocking(true).expect("filler nonblocking");
        filler.set_cloexec(true).expect("filler cloexec");
        match filler.connect(&address) {
            Ok(()) => fillers.push(filler),
            Err(error) if error.raw_os_error() == Some(libc::EINPROGRESS) => fillers.push(filler),
            Err(_) => break,
        }
    }
    assert!(!fillers.is_empty(), "backlog fixture never connected");
    let started = std::time::Instant::now();
    let response = control_socket_request_line_bounded(
        &socket,
        r#"{"cmd":"status"}"#,
        Duration::from_millis(400),
    );
    let elapsed = started.elapsed();
    drop(fillers);
    drop(listener);
    let _ = std::fs::remove_file(&socket);
    assert!(
        response.is_none(),
        "a saturated backlog yields no observation: {response:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "a saturated backlog must not wait past the deadline: {elapsed:?}"
    );
}

#[test]
fn bounded_request_cuts_a_partial_write_at_the_deadline() {
    let socket = bounded_request_socket_path("backpressure");
    serve_once(&socket, |stream| {
        // Never read: the client's large frame fills the socket buffer, and
        // the write side must cut at the deadline instead of re-arming a
        // per-syscall timeout the way write_all would.
        std::thread::sleep(Duration::from_secs(3));
        drop(stream);
    });
    let huge = "x".repeat(8 * 1024 * 1024);
    let started = std::time::Instant::now();
    let response = control_socket_request_line_bounded(&socket, &huge, Duration::from_millis(400));
    let elapsed = started.elapsed();
    let _ = std::fs::remove_file(&socket);
    assert!(response.is_none());
    assert!(
        elapsed < Duration::from_secs(2),
        "a write against a non-reading peer outlived the deadline: {elapsed:?}"
    );
}

#[test]
fn bounded_connect_marks_the_fd_close_on_exec() {
    let socket = bounded_request_socket_path("cloexec");
    serve_once(&socket, |stream| {
        std::thread::sleep(Duration::from_millis(200));
        drop(stream);
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let stream =
        control_socket_connect_deadline(&socket, deadline).expect("connect for CLOEXEC check");
    use std::os::unix::io::AsRawFd as _;
    // SAFETY: fcntl on a live fd owned by `stream`; no memory access.
    let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
    let _ = std::fs::remove_file(&socket);
    assert!(
        flags >= 0,
        "fcntl F_GETFD: {}",
        std::io::Error::last_os_error()
    );
    assert_ne!(
        flags & libc::FD_CLOEXEC,
        0,
        "the control fd must not survive a provider exec"
    );
}

#[test]
fn bounded_request_rejects_nul_and_overlong_paths() {
    use std::os::unix::ffi::OsStringExt as _;
    let nul_path = PathBuf::from(std::ffi::OsString::from_vec(
        b"/tmp/firm-bst-nul\0x.sock".to_vec(),
    ));
    let started = std::time::Instant::now();
    assert!(
        control_socket_request_line_bounded(&nul_path, "{}", Duration::from_secs(5)).is_none(),
        "an interior NUL must be rejected, never truncated into another endpoint"
    );
    let overlong = Path::new("/tmp").join("firm-bst-long-".repeat(20));
    assert!(
        control_socket_request_line_bounded(&overlong, "{}", Duration::from_secs(5)).is_none(),
        "a path beyond the platform sun_path must be refused, not truncated"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "path validation must be prompt: {:?}",
        started.elapsed()
    );
}
