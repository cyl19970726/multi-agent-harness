//! CLI lifecycle coverage for the machine-scoped NodeDaemon.
//!
//! The retired per-TeamRun detached supervisor and launchd plist model is
//! deliberately absent here. A stable local ExecutionNode owns exactly one
//! daemon process, and TeamRun supervisors are children of that authority.

mod firm_env;

use firm_env::{current_project_id, current_space_id, run_firm, TempHome};
use harness_core::NodeDaemonLeaseStatus;
use harness_store::HarnessStore;

fn init_node(home: &TempHome) -> String {
    let output = run_firm(home, home.base(), &["node", "init"]);
    assert!(output.status.success(), "node init failed: {output:?}");
    let node: serde_json::Value = serde_json::from_slice(&output.stdout).expect("node JSON");
    node["id"].as_str().expect("node id").to_string()
}

fn init_registered_node(home: &TempHome) -> String {
    let initialized = run_firm(home, home.base(), &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let node_id = init_node(home);
    let project_id = current_project_id(home);
    let space_id = current_space_id(home);
    let registered = run_firm(
        home,
        home.base(),
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--execution-space-id",
            &space_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    assert!(
        registered.status.success(),
        "node registration failed: {registered:?}"
    );
    node_id
}

fn seed_draining_predecessor(home: &TempHome, node_id: &str) {
    let store = HarnessStore::new(home.spaces_dir().join(current_space_id(home)));
    let now = firm_env::unix_ms();
    let lease = store
        .acquire_node_daemon_lease(
            node_id,
            "dead-predecessor",
            "dead-predecessor-instance",
            now,
            60_000,
        )
        .expect("acquire predecessor lease");
    let draining = store
        .drain_node_daemon_lease(
            node_id,
            &lease.daemon_id,
            lease.generation,
            &lease.instance_id,
            now + 1,
            60_000,
        )
        .expect("drain predecessor lease");
    assert_eq!(draining.status, NodeDaemonLeaseStatus::Draining);
}

#[test]
fn daemon_status_reports_absent_for_initialized_node() {
    let home = TempHome::new("node-daemon-status");
    let node_id = init_node(&home);

    let output = run_firm(&home, home.base(), &["daemon", "status"]);
    assert!(output.status.success(), "daemon status failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("absent"), "unexpected status: {stdout}");
    assert!(
        stdout.contains(&node_id),
        "status omitted Node id: {stdout}"
    );
    assert!(
        stdout.contains("node-daemon.log"),
        "status omitted log path: {stdout}"
    );
}

#[test]
fn daemon_status_names_draining_predecessor_and_recovery_action() {
    let home = TempHome::new("node-daemon-draining-status");
    let node_id = init_registered_node(&home);
    seed_draining_predecessor(&home, &node_id);

    let output = run_firm(&home, home.base(), &["daemon", "status"]);
    assert!(output.status.success(), "daemon status failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("absent (no live NodeDaemon"), "{stdout}");
    assert!(stdout.contains("draining"), "{stdout}");
    assert!(stdout.contains("daemon-recover-predecessor"), "{stdout}");
    assert!(
        stdout.contains(
            &home
                .firm_home()
                .join("nodes")
                .join(&node_id)
                .join("node-daemon.log")
                .display()
                .to_string()
        ),
        "status omitted stable log path: {stdout}"
    );
}

#[test]
fn daemon_start_failure_surfaces_detached_log_tail() {
    let home = TempHome::new("node-daemon-start-log-tail");
    let node_id = init_registered_node(&home);
    seed_draining_predecessor(&home, &node_id);

    let output = run_firm(&home, home.base(), &["daemon", "start"]);
    assert!(!output.status.success(), "daemon start unexpectedly passed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let log_path = home
        .firm_home()
        .join("nodes")
        .join(&node_id)
        .join("node-daemon.log");
    assert!(log_path.is_file(), "detached daemon log was not created");
    assert!(
        stderr.contains(&log_path.display().to_string()),
        "start failure omitted log path: {stderr}"
    );
    assert!(
        stderr.contains("last 20 log lines"),
        "start failure omitted log-tail heading: {stderr}"
    );
    assert!(
        stderr.contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "start failure omitted serve cause from log tail: {stderr}"
    );
}

#[test]
fn daemon_start_is_machine_singleton_and_stop_removes_it() {
    let home = TempHome::new("node-daemon-lifecycle");
    let node_id = init_registered_node(&home);

    let start = run_firm(
        &home,
        home.base(),
        &[
            "daemon",
            "start",
            "--scan-interval-secs",
            "1",
            "--idle-timeout-secs",
            "30",
        ],
    );
    assert!(start.status.success(), "daemon start failed: {start:?}");
    let start_stdout = String::from_utf8_lossy(&start.stdout);
    assert!(
        start_stdout.contains(&node_id),
        "start omitted Node id: {start_stdout}"
    );

    let replay = run_firm(&home, home.base(), &["daemon", "start"]);
    assert!(
        replay.status.success(),
        "idempotent daemon start failed: {replay:?}"
    );
    assert!(
        String::from_utf8_lossy(&replay.stdout).contains("already running"),
        "second start did not reuse machine daemon: {}",
        String::from_utf8_lossy(&replay.stdout)
    );

    let status = run_firm(&home, home.base(), &["daemon", "status"]);
    assert!(status.status.success(), "daemon status failed: {status:?}");
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("daemon status JSON");
    assert_eq!(status_json["ok"], true);
    assert_eq!(status_json["node_id"], node_id);
    assert!(status_json["runs"].is_array());
    assert_eq!(
        status_json["log_path"],
        home.firm_home()
            .join("nodes")
            .join(&node_id)
            .join("node-daemon.log")
            .display()
            .to_string()
    );

    let stop = run_firm(&home, home.base(), &["daemon", "stop"]);
    assert!(stop.status.success(), "daemon stop failed: {stop:?}");

    let absent = run_firm(&home, home.base(), &["daemon", "status"]);
    assert!(
        absent.status.success(),
        "post-stop status failed: {absent:?}"
    );
    assert!(String::from_utf8_lossy(&absent.stdout).contains("absent"));
}
