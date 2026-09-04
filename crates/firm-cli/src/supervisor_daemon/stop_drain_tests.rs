//! Regressions for the honest Stop answer (#584).
//!
//! `daemon stop` used to reply `ok:true` from acceptance alone, so
//! `NODE_DAEMON_DRAIN_INCOMPLETE` could never reach the caller and
//! `daemon status` reported absent while the exact serve process still spun.

use super::tests::TestTree;
use super::*;

const STOP_TEST_NODE_ID: &str = "22222222-2222-4222-8222-222222222221";

struct StopFixture {
    _tree: TestTree,
    socket_path: PathBuf,
    daemon: Arc<MultiTeamDaemon>,
    daemon_generation: u64,
    listener: UnixListener,
}

fn stop_fixture(
    label: &str,
    context: Option<MultiTeamContext>,
    drain_ms: (u64, u64),
) -> StopFixture {
    let tree = TestTree::new(label);
    let firm_home = tree.0.join("home");
    crate::execution_space::register_and_activate(
        &firm_home,
        "stop-space",
        "Stop Space",
        Some("stop-project".to_string()),
        None,
        "unix-ms:1",
    )
    .expect("register stop Execution Space");
    let space = crate::execution_space::list_spaces(&firm_home)
        .expect("list stop Spaces")
        .into_iter()
        .next()
        .expect("exactly one stop Space");
    let store = HarnessStore::new(space.store_root.clone());
    store.init().expect("initialize stop Store");
    store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: STOP_TEST_NODE_ID.into(),
            display_name: "Stop Node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert stop Node");
    store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: STOP_TEST_NODE_ID.into(),
                execution_space_id: space.id.clone(),
                project_binding_id: "stop-project".into(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            &space.id,
        )
        .expect("register stop project");
    let daemon_id = format!("node-daemon:{STOP_TEST_NODE_ID}");
    let lease = store
        .acquire_node_daemon_lease(
            STOP_TEST_NODE_ID,
            &daemon_id,
            "stop-instance",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("acquire stop daemon lease");

    let socket_path = tree.0.join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind stop control socket");
    listener
        .set_nonblocking(true)
        .expect("configure nonblocking stop listener");

    let daemon = Arc::new(MultiTeamDaemon {
        firm_home,
        node_id: STOP_TEST_NODE_ID.into(),
        daemon_id,
        instance_id: "stop-instance".into(),
        contexts: Mutex::new(context.into_iter().collect()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(60),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: Some(600_000),
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: Some(drain_ms),
    });

    StopFixture {
        _tree: tree,
        socket_path,
        daemon,
        daemon_generation: lease.generation,
        listener,
    }
}

fn managed_context(
    thread: std::thread::JoinHandle<CliResult<TeamRunDriveOutcome>>,
    heartbeat: Arc<AtomicBool>,
) -> MultiTeamContext {
    MultiTeamContext {
        execution_space_id: "stop-space".into(),
        project_binding_id: "stop-project".into(),
        run_id: "stop-run".into(),
        daemon_generation: 1,
        supervisor_id: "stop-supervisor".into(),
        supervisor_generation: 1,
        heartbeat_valid: heartbeat,
        thread: Some(thread),
        started_at: Instant::now(),
    }
}

/// Read the lease this daemon generation actually left in the Store. The stop
/// receipt's `authority_released` must agree with this, never with a guess
/// derived from whether some phase reported a failure (DEV-149-REVIEW-02).
fn observed_lease_is_released(fixture: &StopFixture) -> bool {
    HarnessStore::new(
        crate::execution_space::list_spaces(&fixture.daemon.firm_home)
            .expect("list stop Spaces")
            .remove(0)
            .store_root,
    )
    .latest_node_daemon_lease(STOP_TEST_NODE_ID)
    .expect("read stop lease")
    .expect("stop lease remains auditable")
    .status
        == harness_core::NodeDaemonLeaseStatus::Released
}

fn request_stop(fixture: &StopFixture) -> serde_json::Value {
    let mut client = UnixStream::connect(&fixture.socket_path).expect("connect stop client");
    client
        .set_read_timeout(Some(Duration::from_secs(20)))
        .expect("bound stop response wait");
    let request = serde_json::json!({
        "cmd": "stop",
        "execution_space_id": "stop-space",
        "daemon_generation": fixture.daemon_generation,
    });
    writeln!(client, "{request}").expect("send stop request");
    client.flush().expect("flush stop request");
    let mut response = String::new();
    std::io::BufReader::new(&mut client)
        .read_line(&mut response)
        .expect("stop answers within the bounded drain window");
    serde_json::from_str(response.trim()).expect("stop response is complete JSON")
}

#[test]
fn stop_answers_only_after_the_managed_runtime_drains() {
    let heartbeat = Arc::new(AtomicBool::new(true));
    let thread_heartbeat = Arc::clone(&heartbeat);
    let converged = Arc::new(AtomicBool::new(false));
    let thread_converged = Arc::clone(&converged);
    // A Supervisor that only exits once Stop revokes its heartbeat, i.e. the
    // ordinary cooperative drain.
    let thread = std::thread::spawn(move || -> CliResult<TeamRunDriveOutcome> {
        while thread_heartbeat.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(150));
        thread_converged.store(true, Ordering::Release);
        Ok(TeamRunDriveOutcome::Progressed {
            team_run_status: harness_core::TeamRunStatus::Completed,
        })
    });
    let fixture = stop_fixture(
        "stop-drained",
        Some(managed_context(thread, heartbeat)),
        (5_000, 1_000),
    );

    std::thread::scope(|scope| {
        let daemon = Arc::clone(&fixture.daemon);
        let listener = &fixture.listener;
        let server = scope.spawn(move || daemon.serve_loop(listener));
        let response = request_stop(&fixture);
        assert_eq!(
            response["ok"], true,
            "a complete drain is the only thing that may report success: {response}"
        );
        assert_eq!(response["drained"], true);
        assert_eq!(response["daemon_generation"], fixture.daemon_generation);
        assert!(
            converged.load(Ordering::Acquire),
            "the stop answer must not precede the managed runtime's exit"
        );
        server.join().expect("serve thread").expect("serve result");
        assert_eq!(
            response["authority_released"], true,
            "a clean drain releases authority: {response}"
        );
        assert_eq!(
            response["authority_released"].as_bool(),
            Some(observed_lease_is_released(&fixture)),
            "the receipt must report the release that actually happened, not a prediction"
        );
    });
}

#[test]
fn stop_reports_drain_incomplete_without_releasing_authority() {
    let heartbeat = Arc::new(AtomicBool::new(true));
    let release = Arc::new(AtomicBool::new(false));
    let thread_release = Arc::clone(&release);
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    // A Supervisor that ignores the drain entirely: exactly the spinning
    // process #584 observed after `stop` had already reported success.
    let thread = std::thread::spawn(move || -> CliResult<TeamRunDriveOutcome> {
        while !thread_release.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        finished_tx.send(()).expect("publish provider thread exit");
        Ok(TeamRunDriveOutcome::Progressed {
            team_run_status: harness_core::TeamRunStatus::Completed,
        })
    });
    let fixture = stop_fixture(
        "stop-incomplete",
        Some(managed_context(thread, heartbeat)),
        (50, 50),
    );

    std::thread::scope(|scope| {
        let daemon = Arc::clone(&fixture.daemon);
        let listener = &fixture.listener;
        let server = scope.spawn(move || daemon.serve_loop(listener));
        let response = request_stop(&fixture);
        assert_eq!(
            response["ok"], false,
            "an incomplete drain must never report success: {response}"
        );
        assert_eq!(response["drained"], false);
        assert_eq!(response["authority_released"], false);
        assert_eq!(
            response["failed_phase"], "supervisor_drain",
            "the operator must be told which phase denied the stop: {response}"
        );
        assert!(
            response["error"]
                .as_str()
                .is_some_and(|error| error.contains("NODE_DAEMON_DRAIN_INCOMPLETE")),
            "the caller must see the drain failure code: {response}"
        );
        let serve_result = server.join().expect("serve thread");
        assert!(serve_result.is_err(), "a failed drain fails the generation");
        let released = observed_lease_is_released(&fixture);
        assert!(
            !released,
            "an incomplete drain must retain machine authority"
        );
        assert_eq!(
            response["authority_released"].as_bool(),
            Some(released),
            "the receipt must report the lease state the Store actually holds"
        );
    });

    release.store(true, Ordering::Release);
    finished_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("detached test thread exits");
    harness_runtime_host::complete_registered_process_group_shutdown()
        .expect("reset process-group admission after the drain-timeout test");
}
