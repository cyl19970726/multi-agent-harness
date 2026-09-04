use super::*;

#[test]
fn fixture_get_json_retries_an_empty_non_success_response() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture server address");
    let server = std::thread::spawn(move || {
        for response in [
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
        ] {
            let (mut stream, _) = listener.accept().expect("accept fixture GET");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).expect("read fixture GET");
            stream
                .write_all(response.as_bytes())
                .expect("write fixture response");
        }
    });

    let (status, body) = firm_env::get_json_at(&addr.to_string(), "/v1/snapshot");
    server.join().expect("fixture server");
    assert_eq!(status, 200);
    assert_eq!(body, serde_json::json!({"ok": true}));
}

#[test]
fn fixture_get_json_does_not_retry_a_malformed_success_body() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept fixture GET");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).expect("read fixture GET");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json",
            )
            .expect("write malformed success");
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
