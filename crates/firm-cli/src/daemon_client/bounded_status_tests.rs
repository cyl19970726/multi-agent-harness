//! Regression tests for `control_socket_request_line_bounded`: one monotonic
//! deadline across connect, write, and read of a single-line control
//! response, with post-deadline bytes never parsed as proof.

use super::*;
use std::{os::unix::net::UnixListener, path::PathBuf};

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
        use std::io::{BufRead as _, Write as _};
        let mut request = String::new();
        std::io::BufReader::new(&mut stream)
            .read_line(&mut request)
            .expect("consume request before closing peer");
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
fn bounded_request_rejects_nul_before_reaching_a_prefix_listener() {
    use std::os::unix::ffi::OsStringExt as _;
    // A live listener at the prefix path P proves disposition: a crafted
    // P+NUL+suffix must be refused before any connection attempt reaches P,
    // while the valid P control still connects. Bounded and always reaped.
    let prefix = bounded_request_socket_path("nul-prefix");
    let listener = UnixListener::bind(&prefix).expect("bind prefix listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking prefix listener");
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let accept_handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    drop(stream);
                    let _ = accepted_tx.send(true);
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() > deadline {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return,
            }
        }
    });
    let mut crafted = prefix.clone().into_os_string().into_vec();
    crafted.push(0);
    crafted.extend_from_slice(b"-suffix");
    let crafted = PathBuf::from(std::ffi::OsString::from_vec(crafted));
    let started = std::time::Instant::now();
    let response = control_socket_request_line_bounded(
        &crafted,
        r#"{"cmd":"status"}"#,
        Duration::from_secs(5),
    );
    assert!(response.is_none());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "NUL rejection must be prompt: {:?}",
        started.elapsed()
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        accepted_rx.try_recv().is_err(),
        "the crafted NUL path reached the prefix listener — truncation connected to a different endpoint"
    );

    // Valid-path control: the same listener is reachable on the exact path.
    // It writes nothing, so the helper ends without a response, but the
    // accept proves the endpoint itself works.
    let control =
        control_socket_request_line_bounded(&prefix, r#"{"cmd":"status"}"#, Duration::from_secs(2));
    let accepted = accepted_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("prefix listener accept outcome");
    accept_handle.join().expect("join prefix accept thread");
    let _ = std::fs::remove_file(&prefix);
    assert!(accepted, "valid-path control did not reach the listener");
    assert!(
        control.is_none(),
        "the silent control listener cannot produce a response"
    );
}

#[test]
fn bounded_request_refuses_an_overlong_path() {
    let overlong = Path::new("/tmp").join("firm-bst-long-".repeat(20));
    let started = std::time::Instant::now();
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

#[test]
fn general_status_request_has_one_deadline_and_preserves_first_line() {
    let home = bounded_request_socket_path("status-home");
    std::fs::create_dir_all(&home).expect("home");
    let socket = node_daemon_socket_path(&home, "test");
    std::fs::create_dir_all(socket.parent().expect("socket parent")).expect("socket directory");
    serve_once(&socket, |mut stream| {
        let mut request = String::new();
        std::io::BufReader::new(&mut stream)
            .read_line(&mut request)
            .expect("consume status request");
        let _ = stream.write_all(b"{\"ok\":true}\nsecond line\n");
    });
    assert_eq!(
        daemon_status_via_socket(&home, "test").as_deref(),
        Some("{\"ok\":true}")
    );
    std::fs::remove_file(&socket).expect("remove first socket");
    serve_once(&socket, |mut stream| {
        for _ in 0..20 {
            if stream.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    });
    let started = Instant::now();
    assert!(daemon_status_via_socket(&home, "test").is_none());
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "per-read resets exceeded 5s budget"
    );
    std::fs::remove_file(&socket).expect("remove drip socket");
    std::fs::remove_dir_all(&home).expect("cleanup status home");
}
