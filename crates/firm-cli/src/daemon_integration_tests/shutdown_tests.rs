use super::process_isolation::{
    assert_single_test_passed, isolation_dir, kill_child_group, kill_published_fixtures,
    process_group_alive, read_child_output, run_in_isolated_child, spawn_owned_fixture,
    spawn_test_child, unpublish_owned_fixture, wait_child_bounded, wait_for_file_bounded,
    CHILD_SIGNAL_TIMEOUT, ISOLATED_CHILD_TIMEOUT, ISOLATION_DIR_ENV, REAP_TIMEOUT,
};
use super::*;
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
const TIMEOUT_FIXTURE_EXACT: &str =
    "daemon_integration_tests::shutdown_tests::timeout_fixture_child_parks_until_released_by_signal";
const FAILING_CHILD_EXACT: &str =
    "daemon_integration_tests::shutdown_tests::failing_child_exits_nonzero_with_diagnostics";
const PUBLICATION_FAILURE_EXACT: &str =
    "daemon_integration_tests::shutdown_tests::publication_write_failure_cleans_the_exact_owned_fixture";
const GROUP_PROBE_EXACT: &str =
    "daemon_integration_tests::shutdown_tests::process_group_alive_proves_liveness_and_esrch_absence";

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
        let (mut child, mut registration) =
            spawn_owned_fixture(std::process::Command::new("sh").arg("-c").arg("sleep 30"));
        let pid = child.id();
        pid_tx.send(pid).expect("publish owned process group");
        let status = registration.kill_and_reap(&mut child);
        // Retain the publication unless the reap is proven: a failed
        // kill/reap leaves the parent's kill_published_fixtures a reachable
        // fixture (#928).
        unpublish_owned_fixture(pid, &status);
        let status = status?;
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
    // The fixture outlives every designed wait; its only designed release is
    // an explicit signal with a bound — never its own timer (#928).
    let (mut child, mut registration) =
        spawn_owned_fixture(std::process::Command::new("sleep").arg("120"));
    let pid = child.id();
    let hold = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(dir) = &coordination {
            std::fs::write(dir.join("registered"), pid.to_string())
                .expect("publish unrelated process group");
            // The scenario runs in seconds; this bound stays far below the
            // fixture's own lifetime so a hung scenario can never make the
            // aliveness proof vacuous.
            wait_for_file_bounded(
                &dir.join("scenario-done"),
                Duration::from_secs(30),
                "scenario completion signal",
            );
        } else {
            std::thread::sleep(Duration::from_millis(250));
        }
        // The regression's whole point: after a full external shutdown sweep
        // the unrelated group is still exactly as registered — not consumed,
        // not killed.
        assert!(
            process_group_alive(pid),
            "unrelated registered process group was consumed or killed by the external shutdown sweep"
        );
    }));
    // The fixture is reaped on every exit path — including a wait-timeout
    // panic — before any panic propagates. The publication is retained
    // unless the reap is proven, so a failed kill/reap still leaves the
    // parent's kill_published_fixtures a reachable fixture (#928).
    let reap = registration.kill_and_reap(&mut child);
    unpublish_owned_fixture(pid, &reap);
    assert!(
        reap.expect("reap unrelated fixture").is_some(),
        "unrelated fixture must be terminal-reaped"
    );
    if let Err(error) = hold {
        std::panic::resume_unwind(error);
    }
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
        let fixture_pid: u32 = std::fs::read_to_string(dir.join("registered"))
            .expect("read published fixture pid")
            .trim()
            .parse()
            .expect("published fixture pid is numeric");
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
        // Release-by-signal end to end: the registrar reaped its fixture
        // after the completion signal, so the group is provably gone — not
        // merely counting down the sleep's own timer.
        assert!(
            !process_group_alive(fixture_pid),
            "registrar fixture group {fixture_pid} survived its explicit release"
        );
    }));
    if let Err(error) = outcome {
        // One failed side must never strand the other side's processes:
        // every fixture either child published is released by explicit
        // signal with a bounded absence proof (killing a child's own group
        // never reaches independent groups), then the registrar itself is
        // killed and reaped.
        kill_published_fixtures(&dir, REAP_TIMEOUT, "regression fixtures");
        kill_child_group(&mut registrar);
        std::panic::resume_unwind(error);
    }
}

/// Fixture of the forced-timeout cleanup regression. In a coordinated child
/// it spawns an independent fixture process group, publishes the leader pid,
/// then parks until the parent releases it by signal (the backstop bound
/// only fires if the parent itself dies). Standalone it performs the same
/// spawn/register/reap cycle against a short fixed window.
#[test]
fn timeout_fixture_child_parks_until_released_by_signal() {
    run_in_isolated_child(
        TIMEOUT_FIXTURE_EXACT,
        timeout_fixture_child_parks_until_released_by_signal_body,
    );
}

fn timeout_fixture_child_parks_until_released_by_signal_body() {
    let coordination = std::env::var_os(COORDINATION_DIR_ENV).map(PathBuf::from);
    // Same contract as the registrar: the fixture outlives every designed
    // wait and is released only by explicit signal, never by its own timer.
    let (mut child, mut registration) =
        spawn_owned_fixture(std::process::Command::new("sleep").arg("120"));
    let pid = child.id();
    let park = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(dir) = &coordination {
            std::fs::write(dir.join("fixture-published"), pid.to_string())
                .expect("publish timeout fixture pid");
            // The parent kills this child long before this backstop bound
            // fires; the bound only keeps a parentless child finite.
            wait_for_file_bounded(
                &dir.join("release-fixture"),
                CHILD_SIGNAL_TIMEOUT,
                "explicit fixture release signal",
            );
        } else {
            std::thread::sleep(Duration::from_millis(250));
        }
    }));
    // Reap on every exit path before any panic propagates. Same retention
    // rule: unpublish only after a proven reap (#928).
    let reap = registration.kill_and_reap(&mut child);
    unpublish_owned_fixture(pid, &reap);
    assert!(
        reap.expect("reap timeout fixture").is_some(),
        "timeout fixture must be terminal-reaped"
    );
    if let Err(error) = park {
        std::panic::resume_unwind(error);
    }
}

/// Forced-timeout cleanup coverage (#928): a bounded wait kills the hung
/// child's own process group — and provably does NOT remove the child's
/// independent fixture group, which is then released by an explicit signal
/// with a bound. This is the honest shape of "killpg cleans up": it never
/// reaches independent process groups.
#[test]
fn isolated_child_timeout_kills_child_group_but_not_independent_groups() {
    let dir = isolation_dir("timeout-cleanup");
    let mut hanger = spawn_test_child(
        TIMEOUT_FIXTURE_EXACT,
        &dir,
        "hanger",
        &[(
            COORDINATION_DIR_ENV,
            dir.to_str().expect("utf-8 isolation dir"),
        )],
    )
    .expect("spawn hanger child");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for_file_bounded(
            &dir.join("fixture-published"),
            CHILD_SIGNAL_TIMEOUT,
            "hanger fixture publication",
        );
        // Deliberately short bound: the hanger parks far longer, so the
        // timeout kill fires deterministically here.
        wait_child_bounded(&mut hanger, Duration::from_secs(3), "hanger");
    }));
    let timeout_error = outcome.expect_err("hanger child must hit its short wall-clock bound");
    let message = timeout_error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| timeout_error.downcast_ref::<&str>().copied())
        .expect("timeout panic carries a message");
    assert!(
        message.contains("exceeded its") && message.contains("wall-clock bound"),
        "timeout panic must name the bound: {message}"
    );
    // The child's own process group was killed and reaped...
    assert!(
        !process_group_alive(hanger.id()),
        "timed-out child's own process group survived the kill"
    );
    // ...but the kill did not reach the independent fixture group.
    let fixture_pid: u32 = std::fs::read_to_string(dir.join("fixture-published"))
        .expect("read published fixture pid")
        .trim()
        .parse()
        .expect("published fixture pid is numeric");
    assert!(
        process_group_alive(fixture_pid),
        "killing the child's process group must not remove the independent fixture group"
    );
    // The ACTUAL cleanup path, not a special case: every fixture the hanger
    // published is released by explicit signal with a bounded absence proof.
    kill_published_fixtures(&dir, REAP_TIMEOUT, "hanger fixtures");
    assert!(!process_group_alive(fixture_pid));
}

/// Fixture of the forced-failure honesty regression. In a coordinated child
/// it fails deterministically with a recognizable diagnostic; standalone it
/// returns cleanly so the family run never carries a planted failure.
#[test]
fn failing_child_exits_nonzero_with_diagnostics() {
    run_in_isolated_child(
        FAILING_CHILD_EXACT,
        failing_child_exits_nonzero_with_diagnostics_body,
    );
}

fn failing_child_exits_nonzero_with_diagnostics_body() {
    if std::env::var_os(COORDINATION_DIR_ENV).is_some() {
        panic!("planted child failure: failure-output-marker-928");
    }
}

/// Forced-failure honesty coverage (#928): a failing child surfaces its own
/// real output through the zero-match guard — never a fake pass, never a
/// bare exit status — and its already-exited process group needs no further
/// cleanup claim.
#[test]
fn isolated_child_failure_reports_child_output_and_never_fakes_a_pass() {
    let dir = isolation_dir("failure-honesty");
    let mut failing = spawn_test_child(
        FAILING_CHILD_EXACT,
        &dir,
        "failing",
        &[(
            COORDINATION_DIR_ENV,
            dir.to_str().expect("utf-8 isolation dir"),
        )],
    )
    .expect("spawn failing child");
    let status = wait_child_bounded(&mut failing, ISOLATED_CHILD_TIMEOUT, "failing");
    assert!(!status.success(), "planted-failure child must exit nonzero");
    let output = read_child_output(&dir, "failing");
    let guard = std::panic::catch_unwind(|| {
        assert_single_test_passed(status, FAILING_CHILD_EXACT, &output)
    })
    .expect_err("the guard must refuse a failed child");
    let message = guard
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| guard.downcast_ref::<&str>().copied())
        .expect("guard panic carries a message");
    assert!(
        message.contains("failure-output-marker-928"),
        "the guard must surface the child's own diagnostics: {message}"
    );
    assert!(
        output.contains("running 1 test"),
        "the failing child still ran exactly one test — the guard rejected its result, not a zero-match: {output}"
    );
    assert!(
        !process_group_alive(failing.id()),
        "the failed child exited by itself; nothing may claim further cleanup was needed"
    );
}

/// Publication-failure cleanup coverage (#928): when the fixture group id
/// cannot be published (read-only isolation dir), `spawn_owned_fixture` must
/// kill and reap the exact owned child locally before panicking — a failed
/// write must never strand a live, unpublished fixture group.
#[test]
fn publication_write_failure_cleans_the_exact_owned_fixture() {
    run_in_isolated_child(
        PUBLICATION_FAILURE_EXACT,
        publication_write_failure_cleans_the_exact_owned_fixture_body,
    );
}

fn publication_write_failure_cleans_the_exact_owned_fixture_body() {
    use std::os::unix::fs::PermissionsExt;

    let original_dir = std::env::var(ISOLATION_DIR_ENV).expect("isolation dir env in child");
    let read_only = PathBuf::from(&original_dir).join("read-only-publication");
    std::fs::create_dir_all(&read_only).expect("create read-only publication dir");
    std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o555))
        .expect("make publication dir read-only");
    std::env::set_var(ISOLATION_DIR_ENV, &read_only);
    let outcome = std::panic::catch_unwind(|| {
        spawn_owned_fixture(std::process::Command::new("sleep").arg("120"))
    });
    std::env::set_var(ISOLATION_DIR_ENV, &original_dir);
    std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o755))
        .expect("restore publication dir writable");
    let error = match outcome {
        Err(error) => error,
        Ok((mut child, mut registration)) => {
            // A spawn that unexpectedly succeeded must not leak its fixture.
            let _ = registration.kill_and_reap(&mut child);
            panic!("an unwritable publication dir must fail the fixture spawn");
        }
    };
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .expect("spawn panic carries a message");
    let pid: u32 = message
        .split_whitespace()
        .find_map(|token| token.strip_prefix("fixture-pid="))
        .and_then(|value| value.parse().ok())
        .expect("spawn panic names the exact owned fixture pid");
    assert!(
        !process_group_alive(pid),
        "publication failure left fixture group {pid} live and unpublished"
    );
}

/// Probe-semantics pin (#928): a live fixture group probes alive, and after
/// its proven reap the probe proves absence through the ESRCH branch. The
/// non-ESRCH fail-closed arm is defensive and cannot be forced portably.
#[test]
fn process_group_alive_proves_liveness_and_esrch_absence() {
    run_in_isolated_child(
        GROUP_PROBE_EXACT,
        process_group_alive_proves_liveness_and_esrch_absence_body,
    );
}

fn process_group_alive_proves_liveness_and_esrch_absence_body() {
    let (mut child, mut registration) =
        spawn_owned_fixture(std::process::Command::new("sleep").arg("120"));
    let pid = child.id();
    assert!(
        process_group_alive(pid),
        "live fixture group must probe alive"
    );
    let reap = registration.kill_and_reap(&mut child);
    unpublish_owned_fixture(pid, &reap);
    assert!(
        reap.expect("reap probe fixture").is_some(),
        "probe fixture must be terminal-reaped"
    );
    assert!(
        !process_group_alive(pid),
        "a proven-reaped fixture group must prove absence via ESRCH"
    );
}
