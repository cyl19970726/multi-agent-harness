//! Deterministic integration test: Pi Agent Team member orchestration.
//!
//! A fake `pi` shim on PATH answers the pi RPC handshake and streams canned
//! `agent_start` → `turn_end` → `agent_settled` frames. The full Harness loop
//! runs against a temp HOME. No real pi binary is invoked.

use std::path::Path;

mod fake_provider;
mod harness_env;

use harness_env::{current_project_id, run_harness, TempHome};

/// Init a project and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
    current_project_id(home)
}

/// Run harness with the fake pi shim on PATH and PI_BIN set.
fn run_with_fake_pi(
    home: &TempHome,
    fake_bin: &Path,
    result_word: &str,
    args: &[&str],
) -> std::process::Output {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(args)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_SPACE")
        .env_remove("HARNESS_COMPANY")
        .env("PATH", path)
        .env("PI_BIN", fake_bin.join("pi").to_string_lossy().to_string())
        .env("FAKE_PI_RESULT", result_word)
        .env(
            "FAKE_PI_SESSION_DIR",
            home.base().join("pi-sessions").to_string_lossy().to_string(),
        )
        .env(
            "FAKE_PI_CWD_MARKER",
            home.base().join("pi-cwd.txt").to_string_lossy().to_string(),
        )
        .env(
            "HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS",
            "100",
        )
        .output()
        .expect("run harness")
}

/// Read store JSONL rows (latest-wins-per-id projection).
fn store_rows(home: &TempHome, project_id: &str, file: &str) -> Vec<serde_json::Value> {
    let path = home.spaces_dir().join(project_id).join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut ids: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: serde_json::Value =
            serde_json::from_str(trimmed)
                .unwrap_or_else(|e| panic!("{file} row not JSON: {e}"));
        let id = row["id"].as_str().expect("row id").to_string();
        ids.retain(|known| known != &id);
        ids.push(id.clone());
        by_id.insert(id, row);
    }
    ids.into_iter()
        .map(|id| by_id.remove(&id).unwrap())
        .collect()
}

#[test]
fn pi_rpc_team_member_completes_round() {
    let home = TempHome::new("pi-team-member-round");
    let project_id = init_project(&home, "pi-test");

    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-cwd.txt"),
        &home.base().join("pi-sessions/fake-session.jsonl"),
        "DONE",
    );

    // Create the team run with a pi member directly
    let create_out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &[
            "team-run",
            "create",
            "--objective",
            "Verify pi integration",
            "--member",
            "pi-member:reviewer:pi/pi_rpc:any-model@#Review the work and report",
        ],
    );
    assert!(
        create_out.status.success(),
        "team-run create failed: stderr={}",
        String::from_utf8_lossy(&create_out.stderr),
    );
    let run_id = String::from_utf8_lossy(&create_out.stdout).trim().to_string();
    assert!(
        run_id.starts_with("team-run-"),
        "expected team-run-* id, got: {run_id}"
    );

    // Start the team run — this exercises the full orchestration loop
    let start_out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &[
            "team-run",
            "start",
            "--id",
            &run_id,
            "--max-concurrency",
            "1",
        ],
    );
    assert!(
        start_out.status.success(),
        "team-run start failed: stdout={} stderr={}",
        String::from_utf8_lossy(&start_out.stdout),
        String::from_utf8_lossy(&start_out.stderr),
    );

    let stdout = String::from_utf8_lossy(&start_out.stdout);
    assert!(
        stdout.contains(&format!("team run {run_id}\trunning")),
        "summary line should show running: {stdout}"
    );

    // Verify the fake shim was actually called (cwd recorded)
    let cwd_marker = home.base().join("pi-cwd.txt");
    assert!(
        cwd_marker.exists(),
        "fake pi shim should have been called and recorded cwd"
    );
    let recorded_cwd =
        std::fs::read_to_string(&cwd_marker).expect("read cwd marker");
    assert!(
        !recorded_cwd.trim().is_empty(),
        "recorded cwd should not be empty"
    );
}

#[test]
fn pi_rpc_provider_profile_validation() {
    let home = TempHome::new("pi-profile-validation");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-cwd.txt"),
        &home.base().join("pi-sessions/fake.jsonl"),
        "DONE",
    );

    // Verify that pi_rpc is a valid mode for Agent Team
    let out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &["member", "providers"],
    );
    assert!(out.status.success(), "member providers failed");

    let providers: Vec<serde_json::Value> =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
            .expect("member providers JSON");
    let pi = providers
        .iter()
        .find(|p| p.get("provider").and_then(|v| v.as_str()) == Some("pi"))
        .expect("pi should be in provider list");
    let team_profile = pi.get("team_member_profile").expect("team_member_profile");
    assert_eq!(
        team_profile.get("execution_mode").and_then(|v| v.as_str()),
        Some("pi_rpc"),
        "pi team member mode should be pi_rpc"
    );
    assert_eq!(
        team_profile.get("supports_cancel").and_then(|v| v.as_bool()),
        Some(true),
        "pi should support cancel"
    );
    assert_eq!(
        team_profile.get("supports_resume").and_then(|v| v.as_bool()),
        Some(true),
        "pi should support resume"
    );
}
