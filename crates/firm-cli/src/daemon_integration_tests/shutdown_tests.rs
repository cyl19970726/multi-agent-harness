use super::*;
use std::os::unix::process::CommandExt;
use std::sync::OnceLock;

fn shutdown_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[test]
fn shutdown_force_reaps_an_owned_group_before_returning() {
    let _test_lock = shutdown_test_lock();
    let heartbeat = Arc::new(AtomicBool::new(true));
    let thread_heartbeat = Arc::clone(&heartbeat);
    let (pid_tx, pid_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || -> CliResult<TeamRunDriveOutcome> {
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg("sleep 30").process_group(0);
        let mut child = command.spawn()?;
        let pid = child.id();
        let mut registration = harness_runtime_host::OwnedProcessGroupRegistration::new(&mut child)
            .expect("register shutdown test process group");
        pid_tx.send(pid).expect("publish owned process group");
        let status = registration.kill_and_reap(&mut child)?;
        assert!(status.is_some(), "shutdown child must be terminal-reaped");
        assert!(!thread_heartbeat.load(Ordering::Acquire));
        Ok(TeamRunDriveOutcome::Progressed {
            team_run_status: harness_core::TeamRunStatus::Completed,
        })
    });
    let pid = pid_rx.recv().expect("owned provider group pid");
    let daemon = TestDaemon::new(TestDaemonConfig {
        firm_home: std::env::temp_dir(),
        node_id: "shutdown-test-node".into(),
        daemon_id: "node-daemon:shutdown-test-node".into(),
        instance_id: "shutdown-test-instance".into(),
        contexts: vec![OwnedTestContext::new(TestContextConfig {
            execution_space_id: "shutdown-test-space".into(),
            project_binding_id: "shutdown-test-project".into(),
            run_id: "shutdown-test-run".into(),
            daemon_generation: 1,
            supervisor_id: "shutdown-test-supervisor".into(),
            supervisor_generation: 1,
            heartbeat_valid: heartbeat,
            serving_status: Arc::new(Mutex::new("running".into())),
            thread: Some(thread),
            started_at: Instant::now(),
        })],
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: None,
        drain_timeout_override_ms: None,
    });

    daemon
        .graceful_shutdown_with_deadline(Duration::from_millis(20))
        .expect("shutdown reaps exact owned provider group");
    let alive = unsafe { libc::kill(-(pid as libc::pid_t), 0) };
    assert_eq!(alive, -1, "owned provider process group survived shutdown");
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn shutdown_returns_drain_incomplete_when_provider_thread_does_not_converge() {
    let _test_lock = shutdown_test_lock();
    let heartbeat = Arc::new(AtomicBool::new(true));
    let release = Arc::new(AtomicBool::new(false));
    let thread_release = Arc::clone(&release);
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || -> CliResult<TeamRunDriveOutcome> {
        while !thread_release.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        finished_tx.send(()).expect("publish provider thread exit");
        Ok(TeamRunDriveOutcome::Progressed {
            team_run_status: harness_core::TeamRunStatus::Completed,
        })
    });
    let daemon = TestDaemon::new(TestDaemonConfig {
        firm_home: std::env::temp_dir(),
        node_id: "shutdown-timeout-test-node".into(),
        daemon_id: "node-daemon:shutdown-timeout-test-node".into(),
        instance_id: "shutdown-timeout-test-instance".into(),
        contexts: vec![OwnedTestContext::new(TestContextConfig {
            execution_space_id: "shutdown-timeout-space".into(),
            project_binding_id: "shutdown-timeout-project".into(),
            run_id: "shutdown-timeout-run".into(),
            daemon_generation: 1,
            supervisor_id: "shutdown-timeout-supervisor".into(),
            supervisor_generation: 1,
            heartbeat_valid: heartbeat,
            serving_status: Arc::new(Mutex::new("running".into())),
            thread: Some(thread),
            started_at: Instant::now(),
        })],
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: None,
        drain_timeout_override_ms: None,
    });

    let started = Instant::now();
    let error = daemon
        .graceful_shutdown_with_deadlines(Duration::from_millis(10), Duration::from_millis(20))
        .expect_err("unfinished provider thread must fail closed");
    assert!(error.to_string().contains("NODE_DAEMON_DRAIN_INCOMPLETE"));
    assert!(started.elapsed() < Duration::from_secs(1));
    release.store(true, Ordering::Release);
    finished_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("detached test thread exits");
    harness_runtime_host::complete_registered_process_group_shutdown()
        .expect("reset process-group admission after timeout test");
}
