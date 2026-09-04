use super::*;

/// Read the full request head (through `\r\n\r\n`) before a canned response is
/// written. Closing a socket with unread request bytes sends RST on Linux,
/// which can discard the buffered response at the client (#790); macOS
/// delivers the buffered response first, which is why the flake was
/// Linux-only. A single `read()` is not enough: the request can arrive in
/// more than one segment, leaving the tail unread.
fn read_request_head(stream: &mut std::net::TcpStream) {
    use std::io::Read as _;

    let mut head = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).expect("read fixture GET");
        assert!(read > 0, "fixture client closed before the request head");
        head.extend_from_slice(&chunk[..read]);
        if head.windows(4).any(|window| window == b"\r\n\r\n") {
            return;
        }
        assert!(
            head.len() < 64 * 1024,
            "fixture request head exceeded 64 KiB"
        );
    }
}

/// Finish a canned response so the client observes a clean end-of-response:
/// flush, half-close the write side, then close.
fn finish_response(stream: &mut std::net::TcpStream) {
    use std::io::Write as _;

    stream.flush().expect("flush fixture response");
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown fixture response write side");
}

#[test]
fn fixture_get_json_retries_an_empty_non_success_response() {
    use std::io::Write as _;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture server address");
    let server = std::thread::spawn(move || {
        for response in [
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
        ] {
            let (mut stream, _) = listener.accept().expect("accept fixture GET");
            read_request_head(&mut stream);
            stream
                .write_all(response.as_bytes())
                .expect("write fixture response");
            finish_response(&mut stream);
        }
    });

    let (status, body) = firm_env::get_json_at(&addr.to_string(), "/v1/snapshot");
    server.join().expect("fixture server");
    assert_eq!(status, 200);
    assert_eq!(body, serde_json::json!({"ok": true}));
}

#[test]
fn fixture_get_json_does_not_retry_a_malformed_success_body() {
    use std::io::Write as _;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept fixture GET");
        read_request_head(&mut stream);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json",
            )
            .expect("write malformed success");
        finish_response(&mut stream);
        drop(stream);
        listener
            .set_nonblocking(true)
            .expect("make fixture listener nonblocking");
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "malformed 2xx response must not be retried"
        );
    });

    let panic =
        std::panic::catch_unwind(|| firm_env::get_json_at(&addr.to_string(), "/v1/snapshot"))
            .expect_err("malformed 2xx must fail");
    server.join().expect("fixture server");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("panic message");
    assert!(message.contains("status line: \"HTTP/1.1 200 OK\""));
    assert!(message.contains("Content-Type: application/json"));
}

/// Regression for #790: a reset that arrives only AFTER a complete response is
/// already buffered (here forced with an SO_LINGER=0 close) must be treated as
/// end-of-response, never as a request failure.
#[test]
fn fixture_get_json_tolerates_a_reset_after_a_complete_response() {
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::os::unix::io::AsRawFd;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept fixture GET");
        read_request_head(&mut stream);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
            )
            .expect("write fixture response");
        stream.flush().expect("flush fixture response");
        // Let the loopback peer ACK the response before the abortive close:
        // an RST discards the server's unsent send buffer, so without the
        // settle the client could lose the response (the exact #790 race).
        std::thread::sleep(Duration::from_millis(100));
        // SO_LINGER {on, 0}: close sends RST instead of FIN. The response is
        // already complete and acknowledged on the wire, so the client must
        // tolerate the reset.
        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 0,
        };
        let result = unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                &linger as *const libc::linger as *const libc::c_void,
                std::mem::size_of::<libc::linger>() as libc::socklen_t,
            )
        };
        assert_eq!(result, 0, "arm SO_LINGER zero close");
    });

    let (status, body) = firm_env::get_json_at(&addr.to_string(), "/v1/snapshot");
    server.join().expect("fixture server");
    assert_eq!(status, 200);
    assert_eq!(body, serde_json::json!({"ok": true}));
}

#[test]
fn snapshot_read_error_has_a_json_http_error_body() {
    let home = TempHome::new("snapshot-json-error");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    std::fs::write(
        home.spaces_dir()
            .join(project_id)
            .join("provider_launch_profiles.jsonl"),
        "not-json\n",
    )
    .expect("poison snapshot ledger");

    let (status, body) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 400, "body: {body}");
    assert_eq!(body["ok"], false);
    assert!(body["error"]
        .as_str()
        .is_some_and(|error| !error.is_empty()));
}

#[test]
fn post_mutation_response_is_bounded_and_dashboard_can_refresh_from_get_snapshot() {
    let home = TempHome::new("bounded-mutation-response");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    // DOC-108 retired `POST /v1/missions`; the multi-megabyte historical
    // projection is seeded directly into the ledger, and the bounded-mutation
    // proof uses the retained `POST /v1/team-runs` writer.
    let large_context = "x".repeat(20_000);
    {
        use std::io::Write as _;
        let path = home.spaces_dir().join(&_project_id).join("missions.jsonl");
        let mut ledger = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open mission ledger");
        for index in 0..80 {
            writeln!(
                ledger,
                "{}",
                serde_json::json!({
                    "id": format!("mission-large-{index}"),
                    "title": format!("Large mission {index}"),
                    "objective": "inflate the durable read projection",
                    "context": large_context,
                    "status": "planned",
                    "created_at": "unix-ms:1",
                    "updated_at": "unix-ms:1",
                })
            )
            .expect("seed large mission row");
        }
    }

    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "remain reachable from a deep link",
            "members": [{"name": "deep-link-member", "role": "auditor", "provider": "codex"}],
        }),
    );
    assert_eq!(status, 200, "created: {created}");
    assert!(
        created.get("snapshot").is_none(),
        "mutation response leaked a full snapshot"
    );
    assert!(
        serde_json::to_vec(&created).unwrap().len() < 64 * 1024,
        "mutation response exceeded the bounded envelope"
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();

    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200, "snapshot: {snapshot}");
    assert_eq!(
        snapshot["missions"].as_array().map(Vec::len),
        Some(81),
        "the Dashboard refresh GET must still expose every mutation"
    );
    assert!(
        serde_json::to_vec(&snapshot).unwrap().len() > 1_000_000,
        "fixture did not prove the POST response was bounded against a multi-megabyte projection"
    );
    let (status, scoped) = serve.get_json(&format!("/v1/team-runs/{run_id}/snapshot"));
    assert_eq!(status, 200, "scoped: {scoped}");
    assert_eq!(scoped["team_runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(scoped["member_runs"].as_array().map(Vec::len), Some(2));
    assert_eq!(scoped["missions"].as_array().map(Vec::len), Some(0));
    assert!(
        serde_json::to_vec(&scoped).unwrap().len() < 64 * 1024,
        "Team deep-link projection must remain bounded despite a large historical store"
    );
}
