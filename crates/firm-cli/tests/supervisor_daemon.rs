//! CLI lifecycle coverage for the machine-scoped NodeDaemon.
//!
//! The retired per-TeamRun detached supervisor and launchd plist model is
//! deliberately absent here. A stable local ExecutionNode owns exactly one
//! daemon process, and TeamRun supervisors are children of that authority.

mod firm_env;

use firm_env::{current_project_id, current_space_id, run_firm, TempHome};

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

    let stop = run_firm(&home, home.base(), &["daemon", "stop"]);
    assert!(stop.status.success(), "daemon stop failed: {stop:?}");

    let absent = run_firm(&home, home.base(), &["daemon", "status"]);
    assert!(
        absent.status.success(),
        "post-stop status failed: {absent:?}"
    );
    assert!(String::from_utf8_lossy(&absent.stdout).contains("absent"));
}
