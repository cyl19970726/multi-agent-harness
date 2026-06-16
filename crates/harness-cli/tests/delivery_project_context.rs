//! Integration coverage for threading the selected [`ProjectContext`] through
//! persistent delivery (goal-multi-project P3, Stage 3 — `delivery-context`
//! task).
//!
//! `deliver_agent_messages_value` must resolve the SELECTED project (from the
//! centralized store's pinned identity) and hand it down to the provider-specific
//! delivery functions, so the worker's cwd derives from `project.project_root`
//! rather than the harness process cwd. These tests prove the context reaches the
//! provider spawn (the worker runs in the project root) for BOTH providers, and
//! that the no-provider dry-run path is unaffected (back-compat).

#![cfg(unix)]

use std::path::Path;
use std::process::Command;

mod fake_provider;
mod harness_env;

use fake_provider::{install_provider_shim, read_recorded_cwd, DeliveryDriver};
use harness_env::TempHome;

fn canon(p: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The selected project's context reaches the CODEX provider delivery: the worker
/// spawns in `project.project_root`, not the harness process cwd.
#[test]
fn delivery_context_threads_to_codex_provider() {
    let home = TempHome::new("ctx-codex");
    let project_root = home.base().join("proj-codex");
    std::fs::create_dir_all(&project_root).unwrap();
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let marker = home.base().join("codex_cwd.txt");
    let fake_bin = install_provider_shim(home.base(), "codex", &marker);

    let driver = DeliveryDriver::new(&project_root, &elsewhere, home.envs(), &fake_bin);
    driver.init_project();
    let member_id = driver.create_member("codex", None);
    driver.send_message(&member_id, "ctx codex");
    let _ = driver.deliver(&member_id);

    assert_eq!(
        read_recorded_cwd(&marker),
        canon(&project_root),
        "the selected ProjectContext must reach codex delivery (cwd = project_root)"
    );
}

/// The selected project's context reaches the CLAUDE provider delivery too.
#[test]
fn delivery_context_threads_to_claude_provider() {
    let home = TempHome::new("ctx-claude");
    let project_root = home.base().join("proj-claude");
    std::fs::create_dir_all(&project_root).unwrap();
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let marker = home.base().join("claude_cwd.txt");
    let fake_bin = install_provider_shim(home.base(), "claude", &marker);

    let driver = DeliveryDriver::new(&project_root, &elsewhere, home.envs(), &fake_bin);
    driver.init_project();
    let member_id = driver.create_member("claude", None);
    driver.send_message(&member_id, "ctx claude");
    let _ = driver.deliver(&member_id);

    assert_eq!(
        read_recorded_cwd(&marker),
        canon(&project_root),
        "the selected ProjectContext must reach claude delivery (cwd = project_root)"
    );
}

/// BACK-COMPAT: the dry-run delivery path resolves the context but never spawns a
/// provider, so it succeeds with no provider binary present and is unaffected by
/// cwd routing. This guards the existing delivery contract while the context is
/// threaded through.
#[test]
fn dry_run_delivery_still_succeeds_without_a_provider() {
    let home = TempHome::new("ctx-dryrun");
    let project_root = home.base().join("proj-dry");
    std::fs::create_dir_all(&project_root).unwrap();

    let bin = env!("CARGO_BIN_EXE_harness");
    let run = |args: &[&str]| -> (bool, String, String) {
        let out = Command::new(bin)
            .arg("--project")
            .arg(&project_root)
            .args(args)
            .current_dir(home.base())
            .envs(home.envs())
            .env_remove("HARNESS_ROOT")
            .output()
            .expect("run harness");
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };

    assert!(run(&["init"]).0, "init failed");
    let (ok, stdout, stderr) = run(&[
        "agent",
        "create",
        "--name",
        "w",
        "--role",
        "worker",
        "--provider",
        "codex",
    ]);
    assert!(ok, "create failed: {stderr}");
    let member: serde_json::Value = serde_json::from_str(&stdout).expect("create stdout JSON");
    let member_id = member["id"].as_str().expect("member id");

    assert!(
        run(&[
            "agent",
            "send",
            "--to",
            member_id,
            "--from",
            "lead",
            "--content",
            "hi"
        ])
        .0,
        "send failed"
    );
    // --dry-run with --start-runtime: the runtime is recorded but the dry-run
    // branch records a synthetic terminal WITHOUT spawning a provider.
    let (ok, stdout, stderr) = run(&[
        "agent",
        "deliver",
        "--agent",
        member_id,
        "--start-runtime",
        "--dry-run",
    ]);
    assert!(ok, "dry-run deliver failed: {stderr}");
    let result: serde_json::Value = serde_json::from_str(&stdout).expect("deliver stdout JSON");
    let delivered = result["delivered"].as_array().expect("delivered array");
    assert_eq!(
        delivered.len(),
        1,
        "exactly one message delivered: {stdout}"
    );
    assert_eq!(
        delivered[0]["terminal_source"].as_str(),
        Some("dry_run"),
        "dry-run path must not spawn a provider: {stdout}"
    );
}
