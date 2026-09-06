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
