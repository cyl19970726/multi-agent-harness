//! Integration tests for the multi-team supervisor daemon (#366).
//!
//! These tests exercise the daemon lifecycle: spawn, delegate, status, stop,
//! and verify that the control socket protocol works correctly.
//!
//! Tests use the same TempHome + fake provider infrastructure as
//! `supervisor_daemon.rs` and `team_run_start.rs`.

use std::path::Path;
use std::time::Duration;

mod fake_provider;
mod firm_env;

use firm_env::{current_project_id, run_firm, TempHome};

fn wait_for_child(child: &mut std::process::Child, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
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
#[ignore = "#415 owns the multi-team daemon CLI/socket product path"]
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
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn daemon");

    // Wait a moment for the daemon to start and bind the socket.
    std::thread::sleep(Duration::from_secs(2));

    // Verify the supervisor socket exists.
    let store_root = home.spaces_dir().join(&project_id);
    let socket_path = store_root.join("supervisor.sock");
    assert!(
        socket_path.exists(),
        "supervisor socket not found at {}",
        socket_path.display()
    );

    // Connect and send a status command.
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(&socket_path).expect("connect to socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    writeln!(stream, r#"{{"cmd":"status"}}"#).unwrap();
    stream.flush().unwrap();

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf).unwrap();

    let resp: serde_json::Value = serde_json::from_str(buf.trim()).expect("parse status response");
    assert_eq!(resp["ok"], true);
    assert!(resp["runs"].is_array());

    // Stop the daemon.
    let mut stream2 = UnixStream::connect(&socket_path).expect("connect again");
    writeln!(stream2, r#"{{"cmd":"stop"}}"#).unwrap();
    stream2.flush().unwrap();

    wait_for_child(&mut child, Duration::from_secs(5));
    // Clean up: kill if still running.
    let _ = child.kill();
}

// ---------------------------------------------------------------------------
// Test: delegate start to daemon via socket
// ---------------------------------------------------------------------------

#[test]
#[ignore = "#415 owns the multi-team daemon CLI/socket product path"]
fn delegate_start_to_daemon() {
    let home = TempHome::new("mt-delegate");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
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
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    std::thread::sleep(Duration::from_secs(2));

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

    let rows = store_rows(&home, &project_id, "member_runs.jsonl");
    assert!(!rows.is_empty());
    let run_id = rows[0]["team_run_id"].as_str().unwrap().to_string();

    // Start via CLI — should delegate to daemon.
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
            &run_id,
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
        stdout.contains("delegated to supervisor daemon") || stdout.contains("running"),
        "expected delegation or running in output: {stdout}"
    );

    // Verify status via socket.
    let store_root = home.spaces_dir().join(&project_id);
    let socket_path = store_root.join("supervisor.sock");

    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(&socket_path).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    writeln!(stream, r#"{{"cmd":"status"}}"#).unwrap();
    stream.flush().unwrap();
    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf).unwrap();

    let resp: serde_json::Value = serde_json::from_str(buf.trim()).expect("parse status");
    assert_eq!(resp["ok"], true);
    let runs = resp["runs"].as_array().unwrap();
    assert!(!runs.is_empty(), "no runs reported");

    // Stop daemon.
    let mut stream2 = UnixStream::connect(&socket_path).unwrap();
    writeln!(stream2, r#"{{"cmd":"stop"}}"#).unwrap();
    stream2.flush().unwrap();

    wait_for_child(&mut daemon, Duration::from_secs(5));
    let _ = daemon.kill();
}

// ---------------------------------------------------------------------------
// Test: daemon status reports empty when no runs
// ---------------------------------------------------------------------------

#[test]
#[ignore = "#415 owns the multi-team daemon CLI/socket product path"]
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

    std::thread::sleep(Duration::from_secs(2));

    let store_root = home.spaces_dir().join(&project_id);
    let socket_path = store_root.join("supervisor.sock");
    assert!(socket_path.exists(), "socket not found");

    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(&socket_path).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    writeln!(stream, r#"{{"cmd":"status"}}"#).unwrap();
    stream.flush().unwrap();

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf).unwrap();

    let resp: serde_json::Value = serde_json::from_str(buf.trim()).expect("parse status");
    assert_eq!(resp["ok"], true);
    let runs = resp["runs"].as_array().unwrap();
    assert!(runs.is_empty(), "expected empty runs, got {runs:?}");

    // Stop daemon.
    let mut stream2 = UnixStream::connect(&socket_path).unwrap();
    writeln!(stream2, r#"{{"cmd":"stop"}}"#).unwrap();
    stream2.flush().unwrap();

    wait_for_child(&mut daemon, Duration::from_secs(5));
    let _ = daemon.kill();
}
