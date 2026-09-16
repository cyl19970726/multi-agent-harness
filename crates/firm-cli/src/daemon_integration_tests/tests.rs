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

/// ADR 0075 retarget of `unreadable_held_space_latches_only_after_confirmed_deadline`.
///
/// The old test proved a Space whose lease ledger could not be read eventually
/// latched machine-wide authority loss — the right answer while that ledger
/// *was* the authority, because an unreadable Space might hold the live lease.
/// It is not the authority any more. An unreadable Space is a **projection**
/// problem: the machine's owner is one document under the Firm home, and no
/// Space can hide it, contradict it, or cost the daemon the machine by being
/// corrupt. So the successor property is the inverse, and it is the stronger
/// claim: the heartbeat renews straight through two unreadable Spaces, every
/// Space — corrupt ones included — resolves the same lease from the node file,
/// and the corrupt ledgers are still exactly as corrupt afterwards, which is
/// what proves the daemon never needed them.
#[test]
fn an_unreadable_space_is_a_projection_problem_not_an_authority_problem() {
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
    let (healthy, unreadable_spaces) = spaces.split_last().expect("at least one test Space");
    const CORRUPT_TAIL: &[u8] = b"{\"generation\":1";
    for space in unreadable_spaces {
        std::fs::create_dir_all(&space.store_root).expect("initialize unreadable Store root");
        std::fs::write(
            space.store_root.join("node_daemon_leases.jsonl"),
            CORRUPT_TAIL,
        )
        .expect("write bounded incomplete tail");
    }

    let store = HarnessStore::new(healthy.store_root.clone()).with_firm_home(&firm_home);
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
        .seed_machine_authority_for_test(
            NODE_ID,
            &format!("node-daemon:{NODE_ID}"),
            "parallel-refresh-instance",
            current_unix_ms_u64(),
            3_000,
        )
        .expect("acquire the machine lease");

    let daemon = TestDaemon::new(TestDaemonConfig {
        firm_home: firm_home.clone(),
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "parallel-refresh-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_millis(50),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: Some(3_000),
        drain_timeout_override_ms: None,
    });

    // Every Space this instance holds, including the two whose lease ledgers
    // cannot be parsed.
    daemon.remember_node_lease(&healthy.id, &store, &lease);
    for space in unreadable_spaces {
        daemon.remember_node_lease(
            &space.id,
            &HarnessStore::new(space.store_root.clone()).with_firm_home(&firm_home),
            &lease,
        );
    }

    daemon
        .refresh_held_node_authorities()
        .expect("an unreadable Space ledger cannot cost this machine its authority");
    assert!(!daemon.authority_lost());
    assert!(!daemon.stop_requested_flag().load(Ordering::SeqCst));

    let renewed = store
        .current_authorized_machine_lease(NODE_ID)
        .expect("read the machine lease after the heartbeat")
        .expect("the machine lease is still there");
    assert_eq!(renewed.generation, lease.generation);
    assert_eq!(renewed.instance_id, lease.instance_id);
    assert_eq!(renewed.status, harness_core::NodeDaemonLeaseStatus::Active);
    assert!(renewed.renewed_unix_ms >= lease.renewed_unix_ms);
    assert!(renewed.expires_unix_ms > current_unix_ms_u64());

    // The corrupt Spaces answer the machine question with the same document,
    // from the same source — they are projections of it, not competitors.
    for space in unreadable_spaces {
        let (from_corrupt, source) = HarnessStore::new(space.store_root.clone())
            .with_firm_home(&firm_home)
            .current_machine_lease(NODE_ID)
            .expect("an unreadable ledger does not block the machine question")
            .expect("the machine lease resolves through every Space");
        assert_eq!(from_corrupt, renewed);
        assert_eq!(source, harness_store::MachineLeaseSource::NodeFile);
        // Still exactly as corrupt: the heartbeat never touched it, which is
        // the whole reason it could not be hurt by it.
        assert_eq!(
            std::fs::read(space.store_root.join("node_daemon_leases.jsonl")).unwrap(),
            CORRUPT_TAIL
        );
    }
}

/// ADR 0075 retarget of `authority_bundle_rolls_back_partial_acquisition_until_every_predecessor_is_released`.
///
/// "Partial acquisition" was a state only per-Space leases could reach: the
/// bundle acquired Space by Space, so a later Space's refusal left earlier ones
/// already acquired and the daemon had to roll them back by hand. There is one
/// acquire now, on one document, under one lock — so the successor property is
/// the stronger one: acquisition is **atomic on the document**. A publish that
/// fails leaves the previous generation and its history byte-for-byte intact,
/// proven here by injecting a real failure at the atomic replace's staging step
/// rather than by trusting the code path.
///
/// The "until every predecessor is released" half is unchanged, because it is
/// unchanged: an unsettled predecessor still blocks the whole machine.
#[test]
fn authority_acquisition_is_atomic_on_the_document_until_every_predecessor_is_released() {
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
        .seed_machine_authority_for_test(NODE_ID, "predecessor", "crashed-instance", 1, 1)
        .expect("create expired unsettled predecessor");

    let daemon = TestDaemon::new(TestDaemonConfig {
        firm_home: firm_home.clone(),
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "candidate-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: Some(60_000),
        drain_timeout_override_ms: None,
    });
    let error = daemon
        .ensure_node_authority_bundle()
        .expect_err("one unsettled predecessor blocks the entire bundle");
    assert!(error
        .to_string()
        .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"));
    // Nothing partial to roll back: the one document still names the
    // predecessor, and every Space reads that same answer.
    for space in &spaces {
        let lease = HarnessStore::new(space.store_root.clone())
            .with_firm_home(&firm_home)
            .current_authorized_machine_lease(NODE_ID)
            .expect("read the machine lease after the refused acquire")
            .expect("the predecessor lease is still there");
        assert_eq!(lease.instance_id, "crashed-instance");
        assert_ne!(lease.status, harness_core::NodeDaemonLeaseStatus::Released);
    }

    blocked_store
        .release_machine_authority_for_test(
            NODE_ID,
            "predecessor",
            1,
            "crashed-instance",
            current_unix_ms_u64(),
        )
        .expect("simulate explicit Operator predecessor recovery");

    // Fault injection at the one step that can leave a half-state: staging.
    // A directory where the `.tmp` file must be created makes `File::create`
    // fail inside the lease lock, after the decision and before the rename.
    let node_home = blocked_store
        .machine_lease_path(NODE_ID)
        .expect("machine lease path")
        .parent()
        .expect("node home")
        .to_path_buf();
    let released = blocked_store
        .current_authorized_machine_lease(NODE_ID)
        .expect("read the released predecessor")
        .expect("the released predecessor is still the document");
    let document_before = std::fs::read(node_home.join("node-daemon-lease.json"))
        .expect("read the document before the failed acquire");
    let history_before = std::fs::read(node_home.join("node-daemon-lease-history.jsonl"))
        .expect("read the history before the failed acquire");
    std::fs::create_dir(node_home.join("node-daemon-lease.json.tmp"))
        .expect("block the atomic replace's staging file");

    let successor = daemon.successor("successor-instance".into());
    successor
        .ensure_node_authority_bundle()
        .expect_err("an acquire that cannot stage its document must not half-land");
    // Atomic on the document: the previous generation and its history are
    // byte-for-byte what they were. No partial state — not a truncated
    // document, not an orphan history row for a generation nobody owns.
    assert_eq!(
        std::fs::read(node_home.join("node-daemon-lease.json")).unwrap(),
        document_before
    );
    assert_eq!(
        std::fs::read(node_home.join("node-daemon-lease-history.jsonl")).unwrap(),
        history_before
    );
    assert_eq!(
        blocked_store
            .current_authorized_machine_lease(NODE_ID)
            .unwrap()
            .unwrap(),
        released
    );

    std::fs::remove_dir(node_home.join("node-daemon-lease.json.tmp"))
        .expect("clear the staging fault");
    let bundle = successor
        .ensure_node_authority_bundle()
        .expect("all Released predecessors permit one complete bundle");
    assert_eq!(bundle.len(), 2);
    for space in &spaces {
        let lease = HarnessStore::new(space.store_root.clone())
            .with_firm_home(&firm_home)
            .current_authorized_machine_lease(NODE_ID)
            .expect("read successor lease")
            .expect("successor lease exists");
        assert_eq!(lease.instance_id, "successor-instance");
        assert_eq!(lease.status, harness_core::NodeDaemonLeaseStatus::Active);
        assert_eq!(lease.generation, released.generation + 1);
    }
}

#[test]
fn machine_local_live_sink_rejects_invalid_and_stale_registration_then_replaces_successor() {
    let tree = TestTree::new("private-live-sink");
    let daemon = TestDaemon::new(TestDaemonConfig {
        firm_home: tree.0.clone(),
        node_id: "node-live".into(),
        daemon_id: "node-daemon:node-live".into(),
        instance_id: "daemon-instance-current".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: None,
        drain_timeout_override_ms: None,
    });
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
    assert!(daemon.wake_endpoint_count() == 0);

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
    assert_eq!(daemon.wake_endpoint_count(), 1);
    let current = daemon
        .wake_endpoint("member-owner")
        .expect("exact owner sink");
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
        TestDaemon::write_control_response(&mut server, &response)
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
    let daemon = Arc::new(TestDaemon::new(TestDaemonConfig {
        firm_home,
        node_id: "test-node".into(),
        daemon_id: "node-daemon:test-node".into(),
        instance_id: "test-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(60),
        stop_requested: Arc::clone(&shutdown),
        authority_shutdown: Arc::clone(&authority_shutdown),
        lease_ttl_override_ms: None,
        drain_timeout_override_ms: None,
    }));

    // Unscoped spawn: when a phase exceeds its hard deadline the test must
    // fail fast instead of blocking on a scoped join against a thread that is
    // still parked in the blocked FIFO read (the 2h CI hang in run
    // 33900360030). A detached leftover thread only happens on the failure
    // path, where the test has already lost.
    let server_daemon = Arc::clone(&daemon);
    let server = std::thread::spawn(move || server_daemon.serve_loop(&listener));
    let test_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Opening the FIFO writer nonblocking succeeds only after the
        // scanner is waiting in its blocking registry read — that successful
        // open IS the observable blocked-scan state, not a timing guess.
        // Keep the writer open without data so the scan cannot finish while
        // the status request below is served. The 60s budget derives from
        // the product's own daemon-start readiness bound
        // (main_modules/daemon_cli.rs `daemon start`); exhaustion means the
        // daemon never started scanning, not a slow runner.
        let scanner_wait_started = Instant::now();
        let registry_writer = loop {
            match OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&registry_path)
            {
                Ok(file) => break file,
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                    assert!(
                        scanner_wait_started.elapsed() < Duration::from_secs(60),
                        "phase 'wait for scanner to open registry FIFO' exceeded its hard deadline: {:?} elapsed",
                        scanner_wait_started.elapsed()
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    shutdown.store(true, Ordering::SeqCst);
                    panic!("wait for scanner to open registry FIFO: {error}");
                }
            }
        };

        // Always release the scanner before the server is joined, including
        // when a response assertion panics. That keeps a useful failure from
        // turning into a hung test process.
        let status_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut client = UnixStream::connect(&socket_path).expect("connect control client");
            // The "responsive WHILE the scan is blocked" proof is structural:
            // the writer above holds the FIFO open with no data, so the scan
            // cannot finish, and any status response received in that window
            // proves the reserved control lane is not blocked by the scan.
            // This read timeout is only a liveness budget — 10x the original
            // 1s — so a loaded runner's scheduler cannot turn that proof
            // into a flake; the test never asserted a latency figure.
            client
                .set_read_timeout(Some(Duration::from_secs(10)))
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
        // an unbounded filesystem operation that production cannot have. The
        // join budget is a backstop behind the documented 75-second drain
        // bound (20s control workers + 20s scanner + 30s Supervisor drain +
        // 5s settlement, see operations.md) plus slack — a guarantee
        // backstop, not a cadence estimate: a daemon that cannot be released
        // fails this test with the phase name and elapsed time instead of
        // hanging the CI job.
        let release_started = Instant::now();
        loop {
            if server.is_finished() {
                break;
            }
            assert!(
                release_started.elapsed() < Duration::from_secs(120),
                "phase 'release blocked scan and join daemon control thread' exceeded its hard deadline: {:?} elapsed",
                release_started.elapsed()
            );
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
        if let Err(payload) = status_result {
            std::panic::resume_unwind(payload);
        }
    }));
    match test_result {
        Ok(()) => {
            server
                .join()
                .expect("daemon control thread")
                .expect("daemon exits cleanly after blocked scan is released");
        }
        Err(payload) => {
            // The hard-deadline path may leave the daemon thread parked in
            // the blocked FIFO read; detach it rather than re-hang on join.
            if server.is_finished() {
                let _ = server.join();
            }
            std::panic::resume_unwind(payload);
        }
    }
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
    let daemon = Arc::new(TestDaemon::new(TestDaemonConfig {
        firm_home,
        node_id: "test-node".into(),
        daemon_id: "node-daemon:test-node".into(),
        instance_id: "test-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(60),
        stop_requested: Arc::clone(&shutdown),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: None,
        drain_timeout_override_ms: None,
    }));

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
        .seed_machine_authority_for_test(
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
    let daemon = Arc::new(TestDaemon::new(TestDaemonConfig {
        firm_home,
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "test-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_millis(50),
        stop_requested,
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: Some(1_500),
        drain_timeout_override_ms: None,
    }));

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
            .current_authorized_machine_lease(NODE_ID)
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
        .current_authorized_machine_lease(NODE_ID)
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
        .seed_machine_authority_for_test(
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
    let failed_daemon = Arc::new(TestDaemon::new(TestDaemonConfig {
        firm_home: tree.0.join("home"),
        node_id: NODE_ID.into(),
        daemon_id: format!("node-daemon:{NODE_ID}"),
        instance_id: "test-failure-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_millis(50),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: Some(1_500),
        drain_timeout_override_ms: None,
    }));
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
            !failed_daemon.control_worker_failed(),
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
        while !failed_daemon.control_worker_failed() && Instant::now() < failure_deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            failed_daemon.control_worker_failed(),
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
        .current_authorized_machine_lease(NODE_ID)
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
