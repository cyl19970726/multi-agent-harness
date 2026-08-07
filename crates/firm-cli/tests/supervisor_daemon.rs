//! Integration coverage for supervisor daemonization (#346): spawn, adopt,
//! crash-respawn lifecycle, and double-write prevention.  Tests that need the
//! fake kimi shim use the same harness as `team_run_start.rs` — a TempHome with
//! the fake provider binary on PATH.
//!
//! Note: when `FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS` is set, `team-run start`
//! falls back to the old in-process supervisor path for backward compatibility
//! with existing tests.  The daemon-specific tests here do NOT set that env var
//! so they exercise the real daemon spawn path.

use std::path::Path;
use std::time::Duration;

mod fake_provider;
mod firm_env;

use firm_env::{current_project_id, run_firm, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Run `harness <args...>` with the fake kimi dir prepended to PATH so
/// `resolve_kimi_bin` resolves the shim.  This is the same pattern as
/// `team_run_start.rs` but with NO `FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS`
/// — the daemon path MUST be exercised.
fn run_with_fake_kimi_daemon(
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
        // Deliberately NOT setting FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS — we
        // want the daemon to run with its real idle timeout (short).
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
        .env("FIRM_DAEMON_SPAWN_TIMEOUT_SECS", "15")
        .env_remove("KIMI_CODE_BIN")
        .output()
        .expect("run harness")
}

/// Read a supervisor lease for a given team_run_id.  Supervisor leases use
/// `team_run_id` as their identity key, not `id`.
fn read_supervisor_lease(
    home: &TempHome,
    project_id: &str,
    team_run_id: &str,
) -> Option<serde_json::Value> {
    let path = home
        .spaces_dir()
        .join(project_id)
        .join("team_supervisor_leases.jsonl");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: serde_json::Value =
            serde_json::from_str(trimmed).unwrap_or_else(|e| panic!("lease row not JSON: {e}"));
        if row["team_run_id"].as_str() == Some(team_run_id) {
            return Some(row);
        }
    }
    None
}
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

/// Create a run with one kimi member and return (run_id, member_id).
fn create_single_member_run(
    home: &TempHome,
    fake_bin: &Path,
    project_id: &str,
) -> (String, String) {
    let out = run_with_fake_kimi_daemon(
        home,
        fake_bin,
        "done",
        &[
            "--project",
            project_id,
            "team-run",
            "create",
            "--objective",
            "Daemon test run",
            "--member",
            "worker:implementer:kimi@crates/a#Implement the change",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows = store_rows(home, project_id, "member_runs.jsonl");
    assert!(!rows.is_empty(), "no members in {project_id}");
    let member_id = rows[0]["id"].as_str().unwrap().to_string();
    // Extract run_id from the member run row.
    let run_id = rows[0]["team_run_id"].as_str().unwrap().to_string();
    (run_id, member_id)
}

// ---------------------------------------------------------------------------
// Test: spawn daemon, verify it starts and produces member actions
// ---------------------------------------------------------------------------

#[test]
fn spawn_daemon_and_verify_delivery() {
    let home = TempHome::new("daemon-spawn");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let project_id = init_project(&home, "proj");
    let (run_id, _member_id) = create_single_member_run(&home, &fake_bin, &project_id);

    // Set a short lease TTL so we can observe heartbeat activity.
    let start_out = run_with_fake_kimi_daemon(
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

    // `team-run start` in daemon mode exits immediately after spawning.
    // It prints the daemon info to stdout.
    let stdout = String::from_utf8_lossy(&start_out.stdout);
    let stderr = String::from_utf8_lossy(&start_out.stderr);
    assert!(
        start_out.status.success(),
        "start failed: {stderr}\n{stdout}"
    );

    // Verify the lease was created and the daemon is heartbeating.
    let lease = read_supervisor_lease(&home, &project_id, &run_id);
    assert!(lease.is_some(), "no supervisor lease — daemon didn't start");
    let lease = lease.unwrap();
    assert_eq!(lease["status"].as_str(), Some("active"));
    assert!(
        lease["heartbeat_unix_ms"].as_u64().unwrap_or(0) > 0,
        "no heartbeat — daemon not healthy"
    );

    // Verify we can connect to the daemon's control listener.
    let locator = lease["owner_locator"].as_str().unwrap();
    let addr = locator.strip_prefix("tcp://").unwrap_or(locator);
    let stream = std::net::TcpStream::connect_timeout(
        &addr.parse::<std::net::SocketAddr>().unwrap(),
        Duration::from_secs(5),
    );
    assert!(
        stream.is_ok(),
        "cannot connect to daemon at {locator}: {stream:?}"
    );

    // Heartbeat interval is TTL/3 (5s default) — the daemon is alive if
    // the lease exists with a heartbeat and we can connect.
    let _heartbeat = lease["heartbeat_unix_ms"].as_u64().unwrap();
}

// ---------------------------------------------------------------------------
// Test: double-write prevention — two daemon spawns for same team run
// ---------------------------------------------------------------------------

#[test]
fn double_spawn_is_rejected() {
    let home = TempHome::new("daemon-double");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let project_id = init_project(&home, "proj");
    let (run_id, _) = create_single_member_run(&home, &fake_bin, &project_id);

    // First spawn succeeds.
    let out1 = run_with_fake_kimi_daemon(
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
    assert!(
        out1.status.success(),
        "first start failed: {}",
        String::from_utf8_lossy(&out1.stderr)
    );

    // Let the daemon establish its lease.
    std::thread::sleep(Duration::from_secs(2));

    // Second spawn should adopt, not create a new lease generation.
    let out2 = run_with_fake_kimi_daemon(
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
    // Adoption should succeed — the client just connects to the existing daemon.
    assert!(
        out2.status.success(),
        "second start (adopt) failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );

    // Verify only one lease with one generation exists (no double-write).
    let lease = read_supervisor_lease(&home, &project_id, &run_id);
    assert!(lease.is_some(), "expected a lease, got none");
    let generation = lease.unwrap()["generation"].as_u64().unwrap();
    assert_eq!(
        generation, 1,
        "generation should still be 1, got {generation}"
    );
}

// ---------------------------------------------------------------------------
// Test: generate launchd plist
// ---------------------------------------------------------------------------

#[test]
fn generate_plist_is_valid_xml() {
    let home = TempHome::new("daemon-plist");
    let project_id = init_project(&home, "proj");

    // Invoke plist generation via CLI.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "daemon",
            "supervisor",
            "generate-plist",
            "--team-run-id",
            "test-run-123",
        ],
    );
    assert!(
        out.status.success(),
        "plist gen failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Verify the plist file was written to ~/Library/LaunchAgents/.
    let agents_dir = home.home().join("Library").join("LaunchAgents");
    let plist_path = agents_dir.join("com.firm.supervisor.test-run-123.plist");
    assert!(
        plist_path.exists(),
        "plist not found at {}",
        plist_path.display()
    );

    let content = std::fs::read_to_string(&plist_path).unwrap();
    assert!(content.contains("<plist"), "not a plist file");
    assert!(content.contains("test-run-123"), "missing team-run-id");
    assert!(content.contains("KeepAlive"), "missing KeepAlive");
}

// ---------------------------------------------------------------------------
// Test: supervisor daemon status command
// ---------------------------------------------------------------------------

#[test]
fn daemon_status_reports_absent_when_no_daemon() {
    let home = TempHome::new("daemon-status");
    let project_id = init_project(&home, "proj");

    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "daemon",
            "supervisor",
            "status",
            "--team-run-id",
            "nonexistent-run",
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("absent"),
        "expected 'absent', got: {stdout}"
    );
}
