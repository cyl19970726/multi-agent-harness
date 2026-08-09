//! Integration tests for the multi-team supervisor daemon (#366).
//!
//! These tests exercise the daemon lifecycle: spawn, delegate, status, stop,
//! and verify that the control socket protocol works correctly.
//!
//! Tests use the same TempHome + fake provider infrastructure as
//! `supervisor_daemon.rs` and `team_run_start.rs`.

use std::path::Path;
use std::time::Duration;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use harness_core::TeamSupervisorLeaseStatus;
use harness_store::HarnessStore;

mod fake_provider;
mod firm_env;

use firm_env::{current_project_id, run_firm, TempHome};

fn supervisor_socket_path(store_root: &Path) -> std::path::PathBuf {
    let direct = store_root.join("supervisor.sock");
    if direct.to_string_lossy().len() < 100 {
        return direct;
    }
    let mut hasher = DefaultHasher::new();
    store_root.to_string_lossy().hash(&mut hasher);
    std::path::Path::new("/tmp").join(format!("firm-supervisor-{:x}.sock", hasher.finish()))
}

fn wait_for_child(child: &mut std::process::Child, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn wait_for_socket(child: &mut std::process::Child, socket_path: &Path, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::os::unix::net::UnixStream::connect(socket_path).is_ok() {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect daemon") {
            panic!(
                "daemon exited before binding {}: {status}",
                socket_path.display()
            );
        }
        assert!(
            std::time::Instant::now() < deadline,
            "daemon did not bind {} before timeout",
            socket_path.display()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn socket_request(socket_path: &Path, request: &str) -> serde_json::Value {
    use std::io::{BufRead, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    writeln!(stream, "{request}").unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    std::io::BufReader::new(&mut stream)
        .read_line(&mut response)
        .unwrap_or_else(|error| panic!("read daemon response for {request:?}: {error}"));
    serde_json::from_str(response.trim()).unwrap_or_else(|error| {
        panic!("parse daemon response for {request:?} from {response:?}: {error}")
    })
}

fn stop_daemon(child: &mut std::process::Child, socket_path: &Path) {
    let response = socket_request(socket_path, r#"{"cmd":"stop"}"#);
    assert_eq!(response["ok"], true);
    assert!(
        wait_for_child(child, Duration::from_secs(5)),
        "daemon did not drain before timeout"
    );
    assert!(
        !socket_path.exists(),
        "daemon left its control socket behind"
    );
}

fn wait_for_managed_run(socket_path: &Path, run_id: &str, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let status = socket_request(socket_path, r#"{"cmd":"status"}"#);
        if status["runs"]
            .as_array()
            .is_some_and(|runs| runs.iter().any(|run| run["run_id"] == run_id))
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "daemon did not manage {run_id} before timeout: {status}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Init a project and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Run `firm <args...>` with the fake kimi shim on PATH.
fn run_with_fake_kimi(
    home: &TempHome,
    fake_bin: &Path,
    fake_result: &str,
    args: &[&str],
) -> std::process::Output {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(args)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", fake_result)
        .env(
            "FAKE_KIMI_ENV_MARKER",
            home.base().join("kimi-collaboration.env"),
        )
        .env(
            "FAKE_CODEX_ENV_MARKER",
            home.base().join("codex-collaboration.env"),
        )
        .env(
            "FAKE_CODEX_NAME_MARKER",
            home.base().join("codex-thread-name.jsonl"),
        )
        .env(
            "FAKE_CODEX_PLAN_MARKER",
            home.base().join("codex-execution-driver.log"),
        )
        .env("FAKE_CODEX_AUTO_COMPLETE", "1")
        .env(
            "FAKE_CLAUDE_ENV_MARKER",
            home.base().join("claude-collaboration.env"),
        )
        .env_remove("KIMI_CODE_BIN")
        .output()
        .expect("run firm")
}

/// Store rows from a JSONL file.
fn store_rows(home: &TempHome, project_id: &str, file: &str) -> Vec<serde_json::Value> {
    let path = home.spaces_dir().join(project_id).join(file);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut ids: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: serde_json::Value =
            serde_json::from_str(trimmed).unwrap_or_else(|e| panic!("{file} row not JSON: {e}"));
        let id = row["id"].as_str().expect("row id").to_string();
        ids.retain(|known| known != &id);
        ids.push(id.clone());
        by_id.insert(id, row);
    }
    ids.into_iter()
        .map(|id| by_id.remove(&id).unwrap())
        .collect()
}

// ---------------------------------------------------------------------------
// Test: daemon serve starts and binds the control socket
// ---------------------------------------------------------------------------

#[test]
fn daemon_serve_binds_socket() {
    let home = TempHome::new("mt-daemon-bind");
    let project_id = init_project(&home, "proj");

    // Start the multi-team daemon in the background.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args([
            "--project",
            &project_id,
            "daemon",
            "serve",
            "--idle-timeout-secs",
            "5",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn daemon");

    let store_root = home.spaces_dir().join(&project_id);
    let socket_path = supervisor_socket_path(&store_root);
    wait_for_socket(&mut child, &socket_path, Duration::from_secs(5));
    assert!(
        socket_path.exists(),
        "supervisor socket not found at {}",
        socket_path.display()
    );

    let resp = socket_request(&socket_path, r#"{"cmd":"status"}"#);
    assert_eq!(resp["ok"], true);
    assert!(resp["runs"].is_array());
    stop_daemon(&mut child, &socket_path);
}

// ---------------------------------------------------------------------------
// Test: delegate start to daemon via socket
// ---------------------------------------------------------------------------

#[test]
fn delegate_start_to_daemon() {
    let home = TempHome::new("mt-delegate");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let project_id = init_project(&home, "proj");

    // Start the daemon.
    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args([
            "--project",
            &project_id,
            "daemon",
            "serve",
            "--scan-interval-secs",
            "1",
            "--idle-timeout-secs",
            "5",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env("PATH", fake_path)
        .env("KIMI_CODE_BIN", fake_bin.join("kimi"))
        .env("FAKE_KIMI_VERSION", "0.31.0")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    let socket_path = supervisor_socket_path(&home.spaces_dir().join(&project_id));
    wait_for_socket(&mut daemon, &socket_path, Duration::from_secs(5));

    // Create a team run.
    let out = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "MT daemon test",
            "--member",
            "worker:implementer:kimi@crates/a#Implement",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let second = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "MT daemon second run",
            "--member",
            "worker-2:reviewer:kimi@crates/b#Review",
        ],
    );
    assert!(
        second.status.success(),
        "second create failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let run_ids = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .map(|row| row["team_run_id"].as_str().unwrap().to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(run_ids.len(), 2);

    for run_id in &run_ids {
        let start_out = run_with_fake_kimi(
            &home,
            &fake_bin,
            "done",
            &[
                "--project",
                &project_id,
                "team-run",
                "start",
                "--id",
                run_id,
                "--idle-timeout-s",
                "5",
            ],
        );
        let stdout = String::from_utf8_lossy(&start_out.stdout);
        assert!(
            start_out.status.success(),
            "start failed: {}",
            String::from_utf8_lossy(&start_out.stderr)
        );
        assert!(
            stdout.contains("delegated to supervisor daemon"),
            "expected delegation in output: {stdout}"
        );
    }

    let replay = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_ids[0],
        ],
    );
    assert!(
        replay.status.success(),
        "idempotent start failed: {replay:?}"
    );

    let resp = socket_request(&socket_path, r#"{"cmd":"status"}"#);
    assert_eq!(resp["ok"], true);
    let runs = resp["runs"].as_array().unwrap();
    for run_id in &run_ids {
        assert!(
            runs.iter().any(|run| run["run_id"] == *run_id),
            "daemon status omitted {run_id}: {runs:?}"
        );
    }

    let status = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "status",
            "--id",
            &run_ids[0],
            "--json",
        ],
    );
    assert!(status.status.success(), "status failed: {status:?}");
    let status_json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_json["multi_team_daemon"]["ok"], true);
    assert_eq!(
        status_json["multi_team_daemon"]["runs"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    stop_daemon(&mut daemon, &socket_path);
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    for run_id in &run_ids {
        let lease = store
            .latest_team_supervisor_lease(run_id)
            .unwrap()
            .expect("supervisor lease");
        assert_eq!(lease.status, TeamSupervisorLeaseStatus::Released);
    }
}

// ---------------------------------------------------------------------------
// Test: daemon status reports empty when no runs
// ---------------------------------------------------------------------------

#[test]
fn daemon_status_empty() {
    let home = TempHome::new("mt-empty");
    let project_id = init_project(&home, "proj");

    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args([
            "--project",
            &project_id,
            "daemon",
            "serve",
            "--scan-interval-secs",
            "1",
            "--idle-timeout-secs",
            "5",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    let store_root = home.spaces_dir().join(&project_id);
    let socket_path = supervisor_socket_path(&store_root);
    wait_for_socket(&mut daemon, &socket_path, Duration::from_secs(5));
    assert!(socket_path.exists(), "socket not found");

    let resp = socket_request(&socket_path, r#"{"cmd":"status"}"#);
    assert_eq!(resp["ok"], true);
    let runs = resp["runs"].as_array().unwrap();
    assert!(runs.is_empty(), "expected empty runs, got {runs:?}");

    stop_daemon(&mut daemon, &socket_path);
}

#[test]
fn daemon_rejects_second_owner_and_malformed_control_commands() {
    let home = TempHome::new("mt-daemon-negative");
    let project_id = init_project(&home, "proj");
    let daemon_args = [
        "--project",
        project_id.as_str(),
        "daemon",
        "serve",
        "--scan-interval-secs",
        "1",
    ];
    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(daemon_args)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");
    let socket_path = supervisor_socket_path(&home.spaces_dir().join(&project_id));
    wait_for_socket(&mut daemon, &socket_path, Duration::from_secs(5));

    let malformed = socket_request(&socket_path, "{not-json");
    assert_eq!(malformed["ok"], false);
    assert!(malformed["error"]
        .as_str()
        .is_some_and(|error| error.starts_with("invalid json:")));
    let unknown = socket_request(&socket_path, r#"{"cmd":"unknown-command"}"#);
    assert_eq!(unknown["ok"], false);
    assert_eq!(unknown["error"], "unknown command: unknown-command");
    let missing_run = socket_request(&socket_path, r#"{"cmd":"start"}"#);
    assert_eq!(missing_run["ok"], false);
    assert_eq!(missing_run["error"], "run_id is required");

    // A client that disconnects before reading its response must not take the
    // daemon (or its other managed runs) down with its BrokenPipe.
    {
        let mut disconnected = std::os::unix::net::UnixStream::connect(&socket_path).unwrap();
        writeln!(disconnected, "{{\"cmd\":\"status\"}}").unwrap();
        disconnected.flush().unwrap();
    }
    std::thread::sleep(Duration::from_millis(50));
    let after_disconnect = socket_request(&socket_path, r#"{"cmd":"status"}"#);
    assert_eq!(after_disconnect["ok"], true);

    use std::io::{BufRead, Write};
    let mut partial = std::os::unix::net::UnixStream::connect(&socket_path).unwrap();
    partial
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(partial, "{{\"cmd\":").unwrap();
    partial.flush().unwrap();
    let mut partial_response = String::new();
    std::io::BufReader::new(&mut partial)
        .read_line(&mut partial_response)
        .unwrap();
    let partial_json: serde_json::Value = serde_json::from_str(partial_response.trim()).unwrap();
    assert_eq!(partial_json["ok"], false);
    assert_eq!(
        partial_json["error"],
        "control command must be one newline-terminated JSON object"
    );

    let second = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(daemon_args)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .output()
        .expect("start second daemon");
    assert!(
        !second.status.success(),
        "second daemon unexpectedly started"
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("already serves"),
        "unexpected second-daemon error: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    stop_daemon(&mut daemon, &socket_path);
}

#[test]
fn crashed_daemon_is_replaced_and_orphaned_run_is_adopted() {
    let home = TempHome::new("mt-daemon-crash-adopt");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let project_id = init_project(&home, "proj");
    let created = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Crash recovery adoption",
            "--member",
            "worker:implementer:kimi@crates/a#Implement",
        ],
    );
    assert!(created.status.success(), "create failed: {created:?}");
    let run_id = store_rows(&home, &project_id, "member_runs.jsonl")[0]["team_run_id"]
        .as_str()
        .unwrap()
        .to_string();
    let spawn = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
            .args([
                "--project",
                &project_id,
                "daemon",
                "serve",
                "--scan-interval-secs",
                "1",
                "--idle-timeout-secs",
                "5",
            ])
            .current_dir(home.base())
            .envs(home.envs())
            .env_remove("FIRM_ROOT")
            .env_remove("FIRM_PROJECT")
            .env_remove("FIRM_SPACE")
            .env_remove("FIRM_COMPANY")
            .env("PATH", &fake_path)
            .env("KIMI_CODE_BIN", fake_bin.join("kimi"))
            .env("FAKE_KIMI_VERSION", "0.31.0")
            .env("FIRM_TEAM_SUPERVISOR_LEASE_MS", "500")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn daemon")
    };
    let socket_path = supervisor_socket_path(&home.spaces_dir().join(&project_id));
    let mut first = spawn();
    wait_for_socket(&mut first, &socket_path, Duration::from_secs(5));
    let start = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
    );
    assert!(start.status.success(), "start failed: {start:?}");
    wait_for_managed_run(&socket_path, &run_id, Duration::from_secs(5));

    first.kill().expect("crash first daemon");
    first.wait().expect("reap crashed daemon");

    let mut replacement = spawn();
    wait_for_socket(&mut replacement, &socket_path, Duration::from_secs(5));
    wait_for_managed_run(&socket_path, &run_id, Duration::from_secs(5));
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let lease = store
        .latest_team_supervisor_lease(&run_id)
        .unwrap()
        .expect("replacement lease");
    assert!(lease.generation >= 2, "run was not adopted: {lease:?}");

    stop_daemon(&mut replacement, &socket_path);
}
