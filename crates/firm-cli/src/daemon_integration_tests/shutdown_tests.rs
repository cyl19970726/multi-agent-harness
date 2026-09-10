use super::process_isolation::{
    assert_single_test_passed, isolation_dir, kill_child_group, read_child_output,
    run_in_isolated_child, spawn_test_child, wait_child_bounded, wait_for_file_bounded,
    CHILD_SIGNAL_TIMEOUT, ISOLATED_CHILD_TIMEOUT,
};
use super::*;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

// Process-global shutdown scenarios run in isolated child test processes
// (#928): the daemon graceful-shutdown sweep and the provider process-group
// registry are process-global, so the old file-local `shutdown_test_lock`
// could never protect them from stop/drain fixtures or incidental
// `serve_loop` exits elsewhere in this binary. Isolation replaces the lock;
// every behavioral assertion of the original scenarios is preserved in the
// moved bodies.
const FORCE_REAPS_EXACT: &str =
    "daemon_integration_tests::shutdown_tests::shutdown_force_reaps_an_owned_group_before_returning";
const DRAIN_INCOMPLETE_EXACT: &str =
    "daemon_integration_tests::shutdown_tests::shutdown_returns_drain_incomplete_when_provider_thread_does_not_converge";
const UNRELATED_GROUP_EXACT: &str =
    "daemon_integration_tests::shutdown_tests::unrelated_owned_group_survives_a_concurrent_external_shutdown_sweep";

/// Set only in the registrar child of the concurrent regression below; the
/// value is the shared coordination directory.
const COORDINATION_DIR_ENV: &str = "FIRM_PROCESS_ISOLATION_COORDINATION_DIR";

#[test]
fn shutdown_force_reaps_an_owned_group_before_returning() {
    run_in_isolated_child(
        FORCE_REAPS_EXACT,
        shutdown_force_reaps_an_owned_group_before_returning_body,
    );
}

fn shutdown_force_reaps_an_owned_group_before_returning_body() {
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
    run_in_isolated_child(
        DRAIN_INCOMPLETE_EXACT,
        shutdown_returns_drain_incomplete_when_provider_thread_does_not_converge_body,
    );
}

fn shutdown_returns_drain_incomplete_when_provider_thread_does_not_converge_body() {
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

/// The unrelated owned fixture of the concurrent regression. In a coordinated
/// child it registers a real process group, publishes it, holds it across the
/// scenario child's entire run, and proves the external shutdown sweep never
/// reached it. Run standalone (an ordinary harness run still executes it in
/// its own isolated child) it performs the same register/hold/reap cycle
/// against a fixed short window, so it is a real test in both modes.
#[test]
fn unrelated_owned_group_survives_a_concurrent_external_shutdown_sweep() {
    run_in_isolated_child(
        UNRELATED_GROUP_EXACT,
        unrelated_owned_group_survives_a_concurrent_external_shutdown_sweep_body,
    );
}

fn unrelated_owned_group_survives_a_concurrent_external_shutdown_sweep_body() {
    let coordination = std::env::var_os(COORDINATION_DIR_ENV).map(PathBuf::from);
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("spawn unrelated fixture process group");
    let mut registration = harness_runtime_host::OwnedProcessGroupRegistration::new(&mut child)
        .expect("register unrelated fixture process group");
    let pid = child.id();
    if let Some(dir) = &coordination {
        std::fs::write(dir.join("registered"), pid.to_string())
            .expect("publish unrelated process group");
        wait_for_file_bounded(
            &dir.join("scenario-done"),
            ISOLATED_CHILD_TIMEOUT,
            "scenario completion signal",
        );
    } else {
        std::thread::sleep(Duration::from_millis(250));
    }
    // The regression's whole point: after a full external shutdown sweep the
    // unrelated group is still exactly as registered — not consumed, not
    // killed.
    assert_eq!(
        unsafe { libc::kill(-(pid as libc::pid_t), 0) },
        0,
        "unrelated registered process group was consumed or killed by the external shutdown sweep"
    );
    assert!(
        registration
            .kill_and_reap(&mut child)
            .expect("reap unrelated fixture")
            .is_some(),
        "unrelated fixture must be terminal-reaped"
    );
    assert_eq!(unsafe { libc::kill(-(pid as libc::pid_t), 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

/// Deterministic concurrent regression (#928): an isolated shutdown scenario
/// runs its full process-group sweep while an unrelated owned group is
/// registered in a sibling child process. Process isolation — not a lock —
/// keeps the sweep from consuming or killing what it does not own, and the
/// parent's own registry is never touched by either child.
#[test]
fn isolated_shutdown_scenario_cannot_consume_an_unrelated_registered_group() {
    let dir = isolation_dir("concurrent-regression");
    let mut registrar = spawn_test_child(
        UNRELATED_GROUP_EXACT,
        &dir,
        "registrar",
        &[(
            COORDINATION_DIR_ENV,
            dir.to_str().expect("utf-8 isolation dir"),
        )],
    )
    .expect("spawn registrar child");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for_file_bounded(
            &dir.join("registered"),
            CHILD_SIGNAL_TIMEOUT,
            "registrar publication",
        );
        let mut scenario = spawn_test_child(FORCE_REAPS_EXACT, &dir, "scenario", &[])
            .expect("spawn scenario child");
        let scenario_status = wait_child_bounded(&mut scenario, ISOLATED_CHILD_TIMEOUT, "scenario");
        assert_single_test_passed(
            scenario_status,
            FORCE_REAPS_EXACT,
            &read_child_output(&dir, "scenario"),
        );
        std::fs::write(dir.join("scenario-done"), "done").expect("signal scenario completion");
        let registrar_status =
            wait_child_bounded(&mut registrar, ISOLATED_CHILD_TIMEOUT, "registrar");
        assert_single_test_passed(
            registrar_status,
            UNRELATED_GROUP_EXACT,
            &read_child_output(&dir, "registrar"),
        );
    }));
    if let Err(error) = outcome {
        // One failed side must never strand the other side's process group.
        kill_child_group(&mut registrar);
        std::panic::resume_unwind(error);
    }
}
