use super::*;
use std::io::Read;
use std::path::PathBuf;
use std::{os::unix::net::UnixListener, sync::atomic::Ordering};

struct TestTree(PathBuf);

impl TestTree {
    fn new(label: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "firm-node-daemon-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create test tree");
        Self(path)
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn runtime_command_client_retries_a_transient_response_timeout_on_the_same_socket() {
    let tree = TestTree::new("runtime-control-read-retry");
    let firm_home = tree.0.join("home");
    let node_id = "runtime-control-retry-node";
    let socket_path = node_daemon_socket_path(&firm_home, node_id);
    std::fs::create_dir_all(socket_path.parent().expect("control socket parent"))
        .expect("create control socket parent");
    let listener = UnixListener::bind(&socket_path).expect("bind control socket");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept runtime command");
        let mut request = String::new();
        std::io::BufReader::new(&mut stream)
            .read_line(&mut request)
            .expect("read one runtime command");
        assert_eq!(request.lines().count(), 1, "client writes the command once");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(request.trim())
                .expect("runtime command JSON")["cmd"],
            "runtime"
        );
        std::thread::sleep(Duration::from_millis(45));
        stream
            .write_all(b"{\"ok\":true,\"result\":{\"retried\":true}}\n")
            .expect("write delayed runtime response");
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("bound duplicate request check");
        let mut extra = [0_u8; 1];
        match stream.read(&mut extra) {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Ok(count) => panic!("client replayed {count} unexpected request byte(s)"),
            Err(error) => panic!("check for a replayed request: {error}"),
        }
    });
    let payload = serde_json::json!({});
    let envelope = harness_core::agentfirm_api::ControlCommandEnvelope {
        id: "runtime-command-read-retry".into(),
        execution_space_id: "space-test".into(),
        target_node_id: node_id.into(),
        target_node_daemon_id: format!("node-daemon:{node_id}"),
        target_node_daemon_generation: 1,
        authenticated_actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: "test-client".into(),
        },
        command: harness_core::agentfirm_api::RuntimeCommandKind::AuthorMessage,
        required_capability: "message.author".into(),
        idempotency_key: "runtime-command-read-retry".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms_u64().saturating_add(30_000),
        binding: Default::default(),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: harness_store::canonical_json_fingerprint(&payload),
        payload,
        issued_at: format!("unix-ms:{}", current_unix_ms_u64()),
    };

    let response = runtime_command_via_socket_with_policy(
        &firm_home,
        node_id,
        &envelope,
        ControlSocketReadPolicy {
            timeout: Duration::from_millis(20),
            transient_retries: 20,
            retry_backoff: Duration::from_millis(2),
        },
    )
    .expect("same-socket retry receives delayed response");

    assert_eq!(response["result"]["retried"], true);
    server.join().expect("control server");
}

#[test]
fn daemon_stop_client_retries_a_transient_response_timeout_on_the_same_socket() {
    let tree = TestTree::new("stop-control-read-retry");
    let firm_home = tree.0.join("home");
    let node_id = "stop-control-retry-node";
    let socket_path = node_daemon_socket_path(&firm_home, node_id);
    std::fs::create_dir_all(socket_path.parent().expect("control socket parent"))
        .expect("create control socket parent");
    let listener = UnixListener::bind(&socket_path).expect("bind control socket");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept stop command");
        let mut request = String::new();
        std::io::BufReader::new(&mut stream)
            .read_line(&mut request)
            .expect("read one stop command");
        let request =
            serde_json::from_str::<serde_json::Value>(request.trim()).expect("stop command JSON");
        assert_eq!(request["cmd"], "stop");
        assert_eq!(request["execution_space_id"], "space-test");
        assert_eq!(request["daemon_generation"], 7);
        std::thread::sleep(Duration::from_millis(45));
        stream
            .write_all(b"{\"ok\":true,\"state\":\"released\"}\n")
            .expect("write delayed stop response");
    });

    let response = daemon_stop_via_socket_with_policy(
        &firm_home,
        node_id,
        "space-test",
        7,
        ControlSocketReadPolicy {
            timeout: Duration::from_millis(20),
            transient_retries: 20,
            retry_backoff: Duration::from_millis(2),
        },
    )
    .expect("same-socket retry receives accepted stop response");

    assert_eq!(response, r#"{"ok":true,"state":"released"}"#);
    server.join().expect("control server");
}
