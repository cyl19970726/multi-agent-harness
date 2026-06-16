//! Integration coverage for threading the resolved project into workflow options
//! (goal-multi-project, `workflow-options` task).
//!
//! `WorkflowDeliveryOptions` carries a `ProjectContext`; a workflow run executes
//! against the *selected* project's root, not the harness process cwd. This file
//! asserts the two ends of that contract through the real binary:
//!   1. A `--project`-selected project routes the worker/worktree to THAT project's
//!      root (observable via the worktree-base named in a non-git rejection).
//!   2. BACK-COMPAT: a raw `--store` override (no project identity) keeps today's
//!      behavior — the worktree base degrades to the harness process cwd.
//!
//! Runs against an isolated HOME; no live provider is required (the writable-node
//! policy gate fires before any provider spawn).

use std::path::Path;
use std::process::Command;

mod harness_env;
use harness_env::TempHome;

fn write_program(dir: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).expect("mk program dir");
    let path = dir.join("w.star");
    std::fs::write(
        &path,
        r#"
workflow("optroute", "the run must execute against the selected project's root")
phase("edit")
agent("edit a file", provider = "claude", writable = True, label = "editor")
"#,
    )
    .expect("write program");
    path
}

/// Parse the single step's `output_summary` from a non-dry-run run-script result.
fn step_summary(stdout: &str) -> String {
    let result: serde_json::Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("run-script stdout was not JSON ({e}):\n{stdout}"));
    result["steps"][0]["output_summary"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn workflow_options_route_to_the_selected_project_root() {
    let home = TempHome::new("opt-route");
    let project_root = home.base().join("selected-proj");
    std::fs::create_dir_all(&project_root).unwrap();
    // Register the project centrally.
    let init = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--project")
        .arg(&project_root)
        .arg("init")
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("init");
    assert!(init.status.success(), "init failed: {:?}", init);

    let prog = write_program(&home.base().join("prog"));
    // Run from a cwd that is NOT the project root, selecting the project by id-path.
    let out = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--project")
        .arg(&project_root)
        .args(["workflow", "run-script"])
        .arg(&prog)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run workflow");
    let summary = step_summary(&String::from_utf8_lossy(&out.stdout));

    assert!(
        summary.contains("selected-proj"),
        "the options must route the worker to the SELECTED project root: {summary}"
    );
}

#[test]
fn workflow_options_backcompat_store_override_uses_process_cwd() {
    // A raw `--store <path>` carries no project identity, so the project_root
    // degrades to the harness process cwd exactly as before this change — proving we
    // only override the cwd when a project is explicitly selected (issue #89).
    let home = TempHome::new("opt-backcompat");
    let store = home.base().join("legacy-store");
    let cwd = home.base().join("legacy-cwd");
    std::fs::create_dir_all(&cwd).unwrap();

    let prog = write_program(&home.base().join("prog"));
    let out = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--store")
        .arg(&store)
        .args(["workflow", "run-script"])
        .arg(&prog)
        .current_dir(&cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run workflow");
    let summary = step_summary(&String::from_utf8_lossy(&out.stdout));

    assert!(
        summary.contains("legacy-cwd"),
        "back-compat: with --store and no project, the worktree base is the process cwd: {summary}"
    );
    assert!(
        !summary.contains("legacy-store"),
        "the centralized/override STORE path must never be the worktree base: {summary}"
    );
}
