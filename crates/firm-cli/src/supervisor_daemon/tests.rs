use super::*;

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
fn node_authority_heartbeat_is_independent_of_a_long_discovery_scan() {
    assert_eq!(
        node_authority_refresh_interval(Duration::from_millis(20)),
        Duration::from_secs(1)
    );
    assert_eq!(
        node_authority_refresh_interval(Duration::from_secs(2)),
        Duration::from_secs(2)
    );
    assert_eq!(
        node_authority_refresh_interval(Duration::from_secs(30)),
        Duration::from_secs(5)
    );
}

#[test]
fn control_response_is_one_complete_json_frame_under_backpressure() {
    let (mut server, mut client) = UnixStream::pair().expect("create control socket pair");
    server
        .set_nonblocking(true)
        .expect("model accepted nonblocking daemon socket");
    let response = serde_json::json!({
        "ok": true,
        "result": {"payload": "x".repeat(2 * 1024 * 1024)}
    });
    let writer = std::thread::spawn(move || {
        MultiTeamDaemon::write_control_response(&mut server, &response)
            .expect("write one complete framed response");
    });

    // Let the response exceed the socket's immediate send capacity before
    // the reader drains it. The old nonblocking `writeln!` path exposed a
    // truncated JSON prefix at this boundary.
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        !writer.is_finished(),
        "the delayed reader must force the nonblocking writer through backpressure"
    );
    let mut bytes = Vec::new();
    client
        .read_to_end(&mut bytes)
        .expect("read complete response frame");
    writer.join().expect("writer thread");
    assert!(bytes.ends_with(b"\n"));
    assert_eq!(
        bytes.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "one response is exactly one newline-delimited frame"
    );
    let parsed: serde_json::Value = serde_json::from_slice(&bytes[..bytes.len() - 1])
        .expect("response frame is complete JSON before its delimiter");
    assert_eq!(
        parsed["result"]["payload"].as_str().map(str::len),
        Some(2 * 1024 * 1024)
    );
}

#[test]
fn status_remains_responsive_while_execution_space_scan_is_blocked() {
    use std::ffi::CString;
    use std::fs::OpenOptions;
    use std::io::BufReader;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    let tree = TestTree::new("scan-control");
    let firm_home = tree.0.join("home");
    let registry_path = crate::execution_space::registry_path(&firm_home);
    std::fs::create_dir_all(
        registry_path
            .parent()
            .expect("Execution Space registry has a parent"),
    )
    .expect("create Execution Space registry directory");
    let fifo_path = CString::new(registry_path.as_os_str().as_bytes())
        .expect("registry test path has no interior NUL");
    // SAFETY: `fifo_path` is a live, NUL-terminated path and mode contains
    // only filesystem permission bits.
    let mkfifo_result = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
    assert_eq!(
        mkfifo_result,
        0,
        "create blocking registry FIFO: {}",
        std::io::Error::last_os_error()
    );

    let socket_path = tree.0.join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind test control socket");
    listener
        .set_nonblocking(true)
        .expect("configure nonblocking test listener");
    let shutdown = Arc::new(AtomicBool::new(false));
    let daemon = MultiTeamDaemon {
        firm_home,
        node_id: "test-node".into(),
        daemon_id: "node-daemon:test-node".into(),
        instance_id: "test-instance".into(),
        contexts: Mutex::new(Vec::new()),
        session_runtimes: Mutex::new(HashMap::new()),
        live_provider_activity_endpoint: Arc::new(Mutex::new(None)),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(60),
        shutdown: Arc::clone(&shutdown),
    };

    std::thread::scope(|scope| {
        let server = scope.spawn(|| daemon.serve_loop(&listener));

        // Opening the FIFO writer nonblocking succeeds only after the
        // scanner is waiting in its blocking registry read. Keep the
        // writer open without data so the scan cannot finish while the
        // status request below is served.
        let deadline = Instant::now() + Duration::from_secs(2);
        let registry_writer = loop {
            match OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&registry_path)
            {
                Ok(file) => break file,
                Err(error)
                    if error.raw_os_error() == Some(libc::ENXIO) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    shutdown.store(true, Ordering::SeqCst);
                    panic!("wait for scanner to open registry FIFO: {error}");
                }
            }
        };

        // Always release the scanner before the scoped server is joined,
        // including when a response assertion panics. That keeps a useful
        // failure from turning into a hung test process.
        let status_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut client = UnixStream::connect(&socket_path).expect("connect control client");
            client
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("bound status response wait");
            client
                .write_all(b"{\"cmd\":\"status\"}\n")
                .expect("send status request");
            client.flush().expect("flush status request");

            let mut response = String::new();
            BufReader::new(&mut client)
                .read_line(&mut response)
                .expect("status returns while scan is still blocked");
            let response: serde_json::Value =
                serde_json::from_str(response.trim()).expect("status is complete JSON");
            assert_eq!(response["ok"], true);
            assert_eq!(response["node_id"], "test-node");
        }));
        shutdown.store(true, Ordering::SeqCst);
        drop(registry_writer);

        // The independent authority heartbeat may reach the same FIFO
        // after the discovery reader observes EOF but before it observes
        // shutdown. Release that one late read as well; otherwise the
        // scoped join can hide a successful responsiveness assertion
        // behind an unbounded test hang.
        let release_deadline = Instant::now() + Duration::from_secs(2);
        while !server.is_finished() && Instant::now() < release_deadline {
            match OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&registry_path)
            {
                Ok(mut writer) => {
                    writer
                        .write_all(b"\n")
                        .expect("release late registry FIFO reader");
                    break;
                }
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("release late registry FIFO reader: {error}"),
            }
        }
        let server_result = server.join().expect("daemon control thread");
        if let Err(payload) = status_result {
            std::panic::resume_unwind(payload);
        }
        server_result.expect("daemon exits cleanly after blocked scan is released");
    });
}

#[test]
fn node_daemon_socket_path_short_home() {
    let root = std::path::Path::new("/tmp/firm-test");
    let path = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
    assert_eq!(
        path,
        root.join("nodes")
            .join("00000000-0000-4000-8000-000000000001")
            .join("daemon.sock")
    );
}

#[test]
fn node_daemon_socket_path_long_home_fallback() {
    let long = "/tmp/very-long-directory-name-that-makes-the-path-exceed-the-af-unix-limit-on-macos-which-is-104-bytes".repeat(2);
    let root = std::path::Path::new(&long);
    let path = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
    assert!(path.to_string_lossy().starts_with("/tmp/firm-node-daemon-"));
    assert!(path.to_string_lossy().len() < 104);
}

#[test]
fn node_daemon_socket_path_is_stable_per_node() {
    let root = std::path::Path::new("/some/store/root");
    let p1 = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
    let p2 = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
    assert_eq!(p1, p2);
}

#[test]
fn daemon_control_generation_fences_stale_and_successor_instances() {
    let lease = harness_core::NodeDaemonLease {
        node_id: "00000000-0000-4000-8000-000000000001".into(),
        daemon_id: "node-daemon:00000000-0000-4000-8000-000000000001".into(),
        generation: 8,
        instance_id: "successor-instance".into(),
        status: harness_core::NodeDaemonLeaseStatus::Active,
        acquired_unix_ms: 1,
        renewed_unix_ms: 10,
        expires_unix_ms: 100,
        released_unix_ms: None,
    };
    assert!(!daemon_control_generation_authorized(
        Some(&lease),
        &lease.daemon_id,
        "predecessor-instance",
        7,
        20,
    ));
    assert!(!daemon_control_generation_authorized(
        Some(&lease),
        &lease.daemon_id,
        &lease.instance_id,
        7,
        20,
    ));
    assert!(daemon_control_generation_authorized(
        Some(&lease),
        &lease.daemon_id,
        &lease.instance_id,
        8,
        20,
    ));
}
