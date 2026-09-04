use super::*;

pub(super) struct TestTree(pub(super) PathBuf);

impl TestTree {
    pub(super) fn new(label: &str) -> Self {
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
fn unreadable_space_latches_machine_wide_authority_loss() {
    const NODE_ID: &str = "11111111-1111-4111-8111-111111111112";
    let tree = TestTree::new("parallel-authority-refresh");
    let firm_home = tree.0.join("home");
    for index in 0..3 {
        crate::execution_space::register_and_activate(
            &firm_home,
            &format!("space-{index}"),
            &format!("Space {index}"),
            Some(format!("project-{index}")),
            None,
            "unix-ms:1",
        )
        .expect("register test Execution Space");
    }
    let spaces = crate::execution_space::list_spaces(&firm_home).expect("list test Spaces");
    let (healthy, slow_spaces) = spaces.split_last().expect("at least one test Space");
    for space in slow_spaces {
        std::fs::create_dir_all(&space.store_root).expect("initialize slow Store root");
        std::fs::write(
            space.store_root.join("node_daemon_leases.jsonl"),
            b"{\"generation\":1",
        )
        .expect("write bounded incomplete tail");
    }

    let store = HarnessStore::new(healthy.store_root.clone());
    store.init().expect("initialize healthy Store");
    store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: NODE_ID.into(),
            display_name: "Test Node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert test Node");
    store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: NODE_ID.into(),
                execution_space_id: healthy.id.clone(),
                project_binding_id: "project-healthy".into(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            &healthy.id,
        )
        .expect("register healthy test project");
    let lease = store
        .acquire_node_daemon_lease(
            NODE_ID,
            &format!("node-daemon:{NODE_ID}"),
            "parallel-refresh-instance",
            current_unix_ms_u64(),
            250,
        )
        .expect("acquire deliberately short test lease");

    let daemon = MultiTeamDaemon {
        firm_home,
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "parallel-refresh-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_millis(50),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: Some(3_000),
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    };

    let command_actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: daemon.daemon_id.clone(),
    };
    let command_payload = serde_json::json!({"draft": {}});
    let command = harness_core::agentfirm_api::ControlCommandEnvelope {
        id: "runtime-command-after-machine-loss".into(),
        execution_space_id: healthy.id.clone(),
        target_node_id: NODE_ID.into(),
        target_node_daemon_id: daemon.daemon_id.clone(),
        target_node_daemon_generation: lease.generation,
        authenticated_actor: command_actor.clone(),
        command: harness_core::agentfirm_api::RuntimeCommandKind::AuthorMessage,
        required_capability: "message.author".into(),
        idempotency_key: "runtime-command-after-machine-loss".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms_u64().saturating_add(30_000),
        binding: Default::default(),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: harness_store::canonical_json_fingerprint(&command_payload),
        payload: command_payload,
        issued_at: "unix-ms:2".into(),
    };
    let command_context = harness_core::agentfirm_api::MutationContext {
        execution_space_id: healthy.id.clone(),
        authenticated_actor: command_actor.clone(),
        authority_actor: Some(command_actor),
        command_name: "runtime.message.author".into(),
        idempotency_key: command.idempotency_key.clone(),
        expected_version: 0,
        request_fingerprint: Some(
            harness_store::runtime_command_envelope_fingerprint(&command)
                .expect("fingerprint test RuntimeCommand"),
        ),
    };

    let started = Instant::now();
    let error = std::thread::scope(|scope| {
        let refresh = scope.spawn(|| daemon.refresh_held_node_authorities());
        let deadline = Instant::now() + Duration::from_secs(3);
        while !daemon.authority_lost.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            daemon.authority_lost.load(Ordering::SeqCst),
            "the unreadable Space must latch process authority loss"
        );
        let still_active = store
            .latest_node_daemon_lease(NODE_ID)
            .expect("read healthy lease while another Space blocks drain")
            .expect("healthy lease remains present");
        assert_eq!(
            still_active.status,
            harness_core::NodeDaemonLeaseStatus::Active,
            "the cross-Space regression must observe process admission closed before durable drain reaches the healthy Space"
        );
        let operations_before = store
            .canonical_operations()
            .expect("read operations before fenced admission");
        let admission_error = store
            .prepare_runtime_command(
                &command_context,
                &command,
                current_unix_ms_u64(),
                "unix-ms:3",
            )
            .expect_err("machine authority loss must immediately fence another Space");
        assert!(
            admission_error
                .to_string()
                .contains("SUPERVISOR_GENERATION_FENCED"),
            "unexpected admission error: {admission_error}"
        );
        assert_eq!(
            store
                .canonical_operations()
                .expect("read operations after fenced admission"),
            operations_before,
            "fenced admission must have zero durable effect"
        );
        refresh
            .join()
            .expect("authority refresh worker must not panic")
            .expect_err("an unreadable Space must close machine-wide authority")
    });
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "the fixture must exercise the incomplete-row retry window"
    );
    assert!(
        error
            .to_string()
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "unexpected authority error: {error}"
    );
    assert!(daemon.authority_lost.load(Ordering::SeqCst));
    assert!(daemon.stop_requested.load(Ordering::SeqCst));
    let not_refreshed = store
        .latest_node_daemon_lease(NODE_ID)
        .expect("read fenced lease")
        .expect("fenced lease remains present");
    assert_eq!(not_refreshed.generation, lease.generation);
    assert_eq!(
        not_refreshed.instance_id, lease.instance_id,
        "authority loss never fabricates a successor"
    );
    assert_eq!(
        not_refreshed.status,
        harness_core::NodeDaemonLeaseStatus::Draining,
        "machine authority loss must durably drain every readable exact lease"
    );
}

#[test]
fn authority_bundle_rolls_back_partial_acquisition_until_every_predecessor_is_released() {
    const NODE_ID: &str = "11111111-1111-4111-8111-111111111113";
    let tree = TestTree::new("authority-bundle");
    let firm_home = tree.0.join("home");
    for index in 0..2 {
        crate::execution_space::register_and_activate(
            &firm_home,
            &format!("bundle-space-{index}"),
            &format!("Bundle Space {index}"),
            Some(format!("bundle-project-{index}")),
            None,
            "unix-ms:1",
        )
        .expect("register bundle Space");
    }
    let spaces = crate::execution_space::list_spaces(&firm_home).expect("list bundle Spaces");
    for (index, space) in spaces.iter().enumerate() {
        let store = HarnessStore::new(space.store_root.clone());
        store.init().expect("initialize bundle Store");
        store
            .insert_execution_node(&harness_core::ExecutionNode {
                id: NODE_ID.into(),
                display_name: "Bundle Node".into(),
                status: harness_core::ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })
            .expect("insert bundle Node");
        store
            .register_node_project(
                &harness_core::NodeProjectRegistration {
                    node_id: NODE_ID.into(),
                    execution_space_id: space.id.clone(),
                    project_binding_id: format!("bundle-project-{index}"),
                    status: harness_core::NodeProjectRegistrationStatus::Active,
                    created_at: "unix-ms:1".into(),
                    updated_at: "unix-ms:1".into(),
                },
                &space.id,
            )
            .expect("register bundle project");
    }
    let blocked_store = HarnessStore::new(spaces[1].store_root.clone());
    blocked_store
        .acquire_node_daemon_lease(NODE_ID, "predecessor", "crashed-instance", 1, 1)
        .expect("create expired unsettled predecessor");

    let daemon = MultiTeamDaemon {
        firm_home: firm_home.clone(),
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "candidate-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: Some(60_000),
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    };
    let error = daemon
        .ensure_node_authority_bundle()
        .expect_err("one unsettled predecessor blocks the entire bundle");
    assert!(error
        .to_string()
        .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"));
    for space in &spaces {
        let lease = HarnessStore::new(space.store_root.clone())
            .latest_node_daemon_lease(NODE_ID)
            .expect("read post-rollback lease");
        assert!(lease.as_ref().is_none_or(|lease| {
            lease.status == harness_core::NodeDaemonLeaseStatus::Released
                || lease.instance_id == "crashed-instance"
        }));
    }

    blocked_store
        .release_node_daemon_lease(
            NODE_ID,
            "predecessor",
            1,
            "crashed-instance",
            current_unix_ms_u64(),
        )
        .expect("simulate explicit Operator predecessor recovery");
    let successor = MultiTeamDaemon {
        authority_lost: AtomicBool::new(false),
        stop_requested: Arc::new(AtomicBool::new(false)),
        instance_id: "successor-instance".into(),
        ..daemon
    };
    let bundle = successor
        .ensure_node_authority_bundle()
        .expect("all Released predecessors permit one complete bundle");
    assert_eq!(bundle.len(), 2);
    for space in &spaces {
        let lease = HarnessStore::new(space.store_root.clone())
            .latest_node_daemon_lease(NODE_ID)
            .expect("read successor lease")
            .expect("successor lease exists");
        assert_eq!(lease.instance_id, "successor-instance");
        assert_eq!(lease.status, harness_core::NodeDaemonLeaseStatus::Active);
    }
}

#[test]
fn machine_local_live_sink_rejects_invalid_and_stale_registration_then_replaces_successor() {
    let tree = TestTree::new("private-live-sink");
    let daemon = MultiTeamDaemon {
        firm_home: tree.0.clone(),
        node_id: "node-live".into(),
        daemon_id: "node-daemon:node-live".into(),
        instance_id: "daemon-instance-current".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: None,
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    };
    let first_token = "a".repeat(32);
    let first_instance = "b".repeat(32);
    assert!(!daemon.install_native_session_wake_endpoint(
        "198.51.100.1:19001",
        &first_token,
        "member-owner",
        "daemon-instance-current",
        &first_instance,
    ));
    assert!(!daemon.install_native_session_wake_endpoint(
        "127.0.0.1:19001",
        &first_token,
        "member-owner",
        "daemon-instance-stale",
        &first_instance,
    ));
    assert!(daemon
        .native_session_wake_endpoint
        .lock()
        .expect("live sink registry")
        .is_empty());

    assert!(daemon.install_native_session_wake_endpoint(
        "127.0.0.1:19001",
        &first_token,
        "member-owner",
        "daemon-instance-current",
        &first_instance,
    ));
    let successor_token = "c".repeat(32);
    let successor_instance = "d".repeat(32);
    assert!(daemon.install_native_session_wake_endpoint(
        "127.0.0.1:19002",
        &successor_token,
        "member-owner",
        "daemon-instance-current",
        &successor_instance,
    ));
    let endpoints = daemon
        .native_session_wake_endpoint
        .lock()
        .expect("live sink registry");
    assert_eq!(endpoints.len(), 1);
    let current = endpoints.get("member-owner").expect("exact owner sink");
    assert_eq!(current.authority, "127.0.0.1:19002");
    assert_eq!(current.token, successor_token);
    assert_eq!(current.serve_instance_id, successor_instance);
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
    let authority_shutdown = Arc::new(AtomicBool::new(false));
    let daemon = Arc::new(MultiTeamDaemon {
        firm_home,
        node_id: "test-node".into(),
        daemon_id: "node-daemon:test-node".into(),
        instance_id: "test-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(60),
        stop_requested: Arc::clone(&shutdown),
        authority_shutdown: Arc::clone(&authority_shutdown),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: None,
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    });

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
        authority_shutdown.store(true, Ordering::SeqCst);
        drop(registry_writer);

        // The FIFO deliberately replaces a normal registry file. Release
        // every two-phase shutdown read (scanner, heartbeat and drain), not
        // merely the first late reader, so the fixture does not manufacture
        // an unbounded filesystem operation that production cannot have.
        let release_deadline = Instant::now() + Duration::from_secs(2);
        while !server.is_finished() && Instant::now() < release_deadline {
            match OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&registry_path)
            {
                Ok(mut writer) => {
                    if let Err(error) = writer.write_all(b"\n") {
                        assert_eq!(
                            error.kind(),
                            std::io::ErrorKind::BrokenPipe,
                            "release late registry FIFO reader: {error}"
                        );
                    }
                    std::thread::sleep(Duration::from_millis(5));
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
fn status_remains_responsive_while_a_control_mutation_is_blocked() {
    use std::io::BufReader;

    let tree = TestTree::new("mutation-control");
    let firm_home = tree.0.join("home");
    std::fs::create_dir_all(&firm_home).expect("create test FIRM_HOME");
    let socket_path = tree.0.join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind test control socket");
    listener
        .set_nonblocking(true)
        .expect("configure nonblocking test listener");
    let shutdown = Arc::new(AtomicBool::new(false));
    let daemon = Arc::new(MultiTeamDaemon {
        firm_home,
        node_id: "test-node".into(),
        daemon_id: "node-daemon:test-node".into(),
        instance_id: "test-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(60),
        stop_requested: Arc::clone(&shutdown),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: None,
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    });

    std::thread::scope(|scope| {
        let server = scope.spawn(|| daemon.serve_loop(&listener));
        let (sent_tx, sent_rx) = std::sync::mpsc::sync_channel(1);
        let blocker_socket_path = socket_path.clone();
        let blocker = scope.spawn(move || {
            let mut client =
                UnixStream::connect(&blocker_socket_path).expect("connect blocking client");
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("bound blocking response wait");
            client
                .write_all(b"{\"cmd\":\"test_block\",\"delay_ms\":500}\n")
                .expect("send blocking mutation");
            client.flush().expect("flush blocking mutation");
            sent_tx.send(()).expect("announce blocking request");
            let mut response = String::new();
            BufReader::new(&mut client)
                .read_line(&mut response)
                .expect("blocking mutation eventually responds");
            serde_json::from_str::<serde_json::Value>(response.trim())
                .expect("blocking response is complete JSON")
        });

        sent_rx.recv().expect("blocking request was sent");
        std::thread::sleep(Duration::from_millis(75));

        let status_started = Instant::now();
        let mut client = UnixStream::connect(&socket_path).expect("connect status client");
        client
            .set_read_timeout(Some(Duration::from_millis(250)))
            .expect("bound status response wait");
        client
            .write_all(b"{\"cmd\":\"status\"}\n")
            .expect("send status request");
        client.flush().expect("flush status request");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .expect("status bypasses the blocked mutation worker");
        let response: serde_json::Value =
            serde_json::from_str(response.trim()).expect("status response is complete JSON");
        assert_eq!(response["ok"], true);
        assert!(
            status_started.elapsed() < Duration::from_millis(250),
            "status must use the reserved control lane"
        );

        let blocked_response = blocker.join().expect("blocking control client");
        assert_eq!(blocked_response["ok"], true);
        shutdown.store(true, Ordering::SeqCst);
        server
            .join()
            .expect("daemon control thread")
            .expect("daemon exits after draining the mutation worker");
    });
}

#[test]
fn shutdown_renews_node_authority_until_accepted_worker_finishes() {
    use std::io::BufReader;

    const NODE_ID: &str = "11111111-1111-4111-8111-111111111111";
    let tree = TestTree::new("drain");
    let firm_home = tree.0.join("home");
    let space = crate::execution_space::register_and_activate(
        &firm_home,
        "space-test",
        "Space Test",
        Some("project-test".into()),
        None,
        "unix-ms:1",
    )
    .expect("register test Execution Space");
    let store = HarnessStore::new(space.store_root.clone());
    store.init().expect("initialize test Store");
    store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: NODE_ID.into(),
            display_name: "Test Node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert test Node");
    store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: NODE_ID.into(),
                execution_space_id: space.id.clone(),
                project_binding_id: "project-test".into(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            &space.id,
        )
        .expect("register test Node project");
    let lease = store
        .acquire_node_daemon_lease(
            NODE_ID,
            &format!("node-daemon:{NODE_ID}"),
            "test-instance",
            current_unix_ms_u64(),
            1_500,
        )
        .expect("acquire short test lease");

    let socket_path = tree.0.join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind test control socket");
    listener
        .set_nonblocking(true)
        .expect("configure nonblocking test listener");
    let stop_requested = Arc::new(AtomicBool::new(false));
    let daemon = Arc::new(MultiTeamDaemon {
        firm_home,
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "test-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_millis(50),
        stop_requested,
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: Some(1_500),
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    });

    std::thread::scope(|scope| {
        let server = scope.spawn(|| daemon.serve_loop(&listener));
        let (sent_tx, sent_rx) = std::sync::mpsc::sync_channel(1);
        let blocker_socket_path = socket_path.clone();
        let blocker = scope.spawn(move || {
            let mut client =
                UnixStream::connect(&blocker_socket_path).expect("connect blocking client");
            client
                .set_read_timeout(Some(Duration::from_secs(4)))
                .expect("bound blocking response wait");
            client
                .write_all(b"{\"cmd\":\"test_block\",\"delay_ms\":2000}\n")
                .expect("send accepted slow mutation");
            client.flush().expect("flush accepted slow mutation");
            sent_tx.send(()).expect("announce accepted request");
            let mut response = String::new();
            BufReader::new(&mut client)
                .read_line(&mut response)
                .expect("accepted mutation responds before authority release");
            serde_json::from_str::<serde_json::Value>(response.trim())
                .expect("blocking response is complete JSON")
        });

        sent_rx.recv().expect("slow mutation was sent");
        std::thread::sleep(Duration::from_millis(100));
        // Stop is answered from the drain result, not from its acceptance
        // (#584), so read that answer on its own thread while this one
        // observes the drain it is waiting for.
        let (stop_sent_tx, stop_sent_rx) = std::sync::mpsc::sync_channel(1);
        let stop_socket_path = socket_path.clone();
        let stop_space_id = space.id.clone();
        let stop_generation = lease.generation;
        let stopper = scope.spawn(move || {
            let mut stop_client =
                UnixStream::connect(&stop_socket_path).expect("connect stop client");
            stop_client
                .set_read_timeout(Some(Duration::from_secs(20)))
                .expect("bound stop response wait");
            stop_client
                .write_all(
                    format!(
                        "{{\"cmd\":\"stop\",\"execution_space_id\":\"{stop_space_id}\",\"daemon_generation\":{stop_generation}}}\n"
                    )
                    .as_bytes(),
                )
                .expect("send stop request");
            stop_client.flush().expect("flush stop request");
            stop_sent_tx.send(()).expect("announce stop request");
            let mut stop_response = String::new();
            BufReader::new(&mut stop_client)
                .read_line(&mut stop_response)
                .expect("reserved stop lane answers with its drain result");
            serde_json::from_str::<serde_json::Value>(stop_response.trim())
                .expect("stop response JSON")
        });
        stop_sent_rx.recv().expect("stop request was sent");

        // Cross the lease TTL that existed when Stop was accepted. The
        // heartbeat must have renewed this exact generation while the
        // accepted worker was still running.
        std::thread::sleep(Duration::from_millis(1_600));
        assert!(!server.is_finished(), "daemon still drains accepted worker");
        let during_drain = store
            .latest_node_daemon_lease(NODE_ID)
            .expect("read lease during drain")
            .expect("lease remains present");
        assert_eq!(
            during_drain.status,
            harness_core::NodeDaemonLeaseStatus::Active
        );
        assert_eq!(during_drain.generation, lease.generation);
        assert!(
            during_drain.expires_unix_ms > current_unix_ms_u64(),
            "accepted worker retains unexpired exact Node authority"
        );

        assert_eq!(blocker.join().expect("blocking control client")["ok"], true);
        let stop_response = stopper.join().expect("stop control client");
        assert_eq!(stop_response["ok"], true, "stop response: {stop_response}");
        assert_eq!(stop_response["drained"], true);
        server
            .join()
            .expect("daemon control thread")
            .expect("two-phase shutdown completes");
    });

    let released = store
        .latest_node_daemon_lease(NODE_ID)
        .expect("read released lease")
        .expect("released lease remains auditable");
    assert_eq!(
        released.status,
        harness_core::NodeDaemonLeaseStatus::Released
    );

    // A successor test generation accepts a command whose worker cannot
    // prove completion. Shutdown may fence it as Draining, but must not mint
    // the Released receipt that would authorize another successor.
    let failed_lease = store
        .acquire_node_daemon_lease(
            NODE_ID,
            &format!("node-daemon:{NODE_ID}"),
            "test-failure-instance",
            current_unix_ms_u64(),
            1_500,
        )
        .expect("acquire failure-test lease");
    let failure_socket_path = tree.0.join("fail.sock");
    let failure_listener =
        UnixListener::bind(&failure_socket_path).expect("bind failure control socket");
    failure_listener
        .set_nonblocking(true)
        .expect("configure failure listener");
    let failed_daemon = Arc::new(MultiTeamDaemon {
        firm_home: tree.0.join("home"),
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "test-failure-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_millis(50),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: Some(1_500),
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    });
    std::thread::scope(|scope| {
        let server = scope.spawn(|| failed_daemon.serve_loop(&failure_listener));

        // The accepted command completes successfully, but its client goes
        // away before the response can be written. This is response loss, not
        // an unresolved semantic effect, and must not poison the generation.
        let mut abandoned_client =
            UnixStream::connect(&failure_socket_path).expect("connect abandoned client");
        abandoned_client
            .write_all(b"{\"cmd\":\"test_block\",\"delay_ms\":25}\n")
            .expect("send successful command whose response is abandoned");
        abandoned_client.flush().expect("flush abandoned command");
        drop(abandoned_client);
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !failed_daemon.control_worker_failed.load(Ordering::SeqCst),
            "response delivery failure after semantic completion is nonfatal"
        );

        let mut failed_client =
            UnixStream::connect(&failure_socket_path).expect("connect failed worker client");
        failed_client
            .write_all(b"{\"cmd\":\"test_fail\"}\n")
            .expect("send accepted failing command");
        failed_client.flush().expect("flush failing command");
        drop(failed_client);

        let failure_deadline = Instant::now() + Duration::from_secs(1);
        while !failed_daemon.control_worker_failed.load(Ordering::SeqCst)
            && Instant::now() < failure_deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            failed_daemon.control_worker_failed.load(Ordering::SeqCst),
            "accepted worker failure is latched before shutdown"
        );

        let mut stop_client =
            UnixStream::connect(&failure_socket_path).expect("connect failure stop client");
        stop_client
            .write_all(
                format!(
                    "{{\"cmd\":\"stop\",\"execution_space_id\":\"{}\",\"daemon_generation\":{}}}\n",
                    space.id, failed_lease.generation
                )
                .as_bytes(),
            )
            .expect("send failure-generation stop");
        stop_client.flush().expect("flush failure-generation stop");
        let mut stop_response = String::new();
        BufReader::new(&mut stop_client)
            .read_line(&mut stop_response)
            .expect("failure-generation stop responds");
        // The accepted worker never proved completion, so this generation may
        // drain but never Release. Stop must report that, not success (#584).
        let stop_response = serde_json::from_str::<serde_json::Value>(stop_response.trim())
            .expect("failure stop response JSON");
        assert_eq!(stop_response["ok"], false, "stop response: {stop_response}");
        assert_eq!(stop_response["drained"], false);
        assert_eq!(stop_response["authority_released"], false);
        assert!(stop_response["error"]
            .as_str()
            .is_some_and(|error| error.contains("NODE_DAEMON_CONTROL_DRAIN_INCOMPLETE")));
        let error = server
            .join()
            .expect("failure daemon control thread")
            .expect_err("unresolved accepted worker fails shutdown");
        assert!(
            error
                .to_string()
                .contains("NODE_DAEMON_CONTROL_DRAIN_INCOMPLETE"),
            "unexpected shutdown failure: {error}"
        );
    });
    let not_released = store
        .latest_node_daemon_lease(NODE_ID)
        .expect("read failed generation")
        .expect("failed generation remains auditable");
    assert_eq!(not_released.generation, failed_lease.generation);
    assert_eq!(
        not_released.status,
        harness_core::NodeDaemonLeaseStatus::Draining,
        "unresolved accepted worker must never publish Released"
    );
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
fn node_daemon_socket_path_uses_one_identity_for_alias_equivalent_long_homes() {
    use std::os::unix::fs::symlink;

    let tree = TestTree::new("socket-alias");
    let physical_parent = tree.0.join("physical");
    std::fs::create_dir_all(&physical_parent).expect("create physical home parent");
    let alias_parent = tree.0.join("alias");
    symlink(&physical_parent, &alias_parent).expect("create home path alias");
    let long_suffix = "long-home-segment-".repeat(8);
    let physical_home = physical_parent.join(&long_suffix);
    std::fs::create_dir_all(&physical_home).expect("create long physical home");
    let alias_home = alias_parent.join(&long_suffix);
    let node_id = "00000000-0000-4000-8000-000000000001";

    let physical_socket = node_daemon_socket_path(&physical_home, node_id);
    let alias_socket = node_daemon_socket_path(&alias_home, node_id);

    assert_eq!(physical_socket, alias_socket);
    assert!(physical_socket
        .to_string_lossy()
        .starts_with("/tmp/firm-node-daemon-"));
}

#[test]
fn node_daemon_socket_path_keeps_distinct_homes_and_nodes_isolated() {
    let tree = TestTree::new("socket-isolation");
    let long_suffix = "long-home-segment-".repeat(8);
    let home_a = tree.0.join("home-a").join(&long_suffix);
    let home_b = tree.0.join("home-b").join(&long_suffix);
    std::fs::create_dir_all(&home_a).expect("create first long home");
    std::fs::create_dir_all(&home_b).expect("create second long home");

    let node_a = "00000000-0000-4000-8000-000000000001";
    let node_b = "00000000-0000-4000-8000-000000000002";
    assert_ne!(
        node_daemon_socket_path(&home_a, node_a),
        node_daemon_socket_path(&home_b, node_a)
    );
    assert_ne!(
        node_daemon_socket_path(&home_a, node_a),
        node_daemon_socket_path(&home_a, node_b)
    );
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

#[test]
fn rejected_live_scope_does_not_discard_the_registered_serve_endpoint() {
    let rejected = NativeSessionWakePostError::Rejected("HTTP/1.1 400 Bad Request".into());
    let unavailable = NativeSessionWakePostError::Unavailable(std::io::Error::new(
        std::io::ErrorKind::ConnectionRefused,
        "serve exited",
    ));

    assert!(!rejected.clears_registered_endpoint());
    assert!(unavailable.clears_registered_endpoint());
}
