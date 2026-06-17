//! Integration coverage for the GLOBAL / non-git workflow policy
//! (goal-multi-project, `global-workflow-policy` task).
//!
//! The reserved `_global` project is rooted at `~/` and is normally NOT a git
//! repo. A `writable` / `isolation="worktree"` workflow node needs a git worktree,
//! which cannot exist there, so it must FAIL LOUD with an actionable, non-git
//! message — surfaced BEFORE any provider spawn (so the test needs no live
//! provider). A read-only `isolation="none"` node must NOT hit that policy gate.
//!
//! Drives the real `harness` binary against an isolated HOME so the developer's
//! real `~/.harness` is never touched. The non-dry-run path is used deliberately:
//! the policy gate lives in `spawn_ephemeral_worker`, which `--dry-run` skips.

use std::path::Path;
use std::process::Command;

mod harness_env;
use harness_env::TempHome;

/// Initialize the reserved `_global` (`~/`) project so its central store +
/// metadata exist and it is the active project.
fn init_global(home: &TempHome) {
    let out = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(["--project", "_global", "init"])
        .current_dir(home.home())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run init _global");
    assert!(out.status.success(), "init _global failed: {:?}", out);
}

/// Write a one-node Starlark workflow program and return its path.
fn write_program(dir: &Path, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).expect("mk program dir");
    let path = dir.join("w.star");
    std::fs::write(&path, body).expect("write program");
    path
}

/// Run `harness --project _global workflow run-script <prog> [extra]` and return
/// the parsed JSON result.
///
/// For the writable/isolation cases we run NON-dry-run on purpose: the policy gate
/// fires in `spawn_ephemeral_worker` (which `--dry-run` skips) and returns BEFORE
/// any provider spawn, so no live provider is involved. For the read-only case we
/// pass `--dry-run` so the full pipeline runs WITHOUT spawning a provider.
fn run_global_workflow(home: &TempHome, prog: &Path, extra: &[&str]) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
    cmd.args(["--project", "_global", "workflow", "run-script"])
        .arg(prog);
    for a in extra {
        cmd.arg(a);
    }
    let out = cmd
        .current_dir(home.base()) // cwd intentionally != the project root (~/)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run workflow");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("run-script stdout was not JSON ({e}):\n{stdout}"))
}

#[test]
fn global_writable_node_fails_with_actionable_non_git_message() {
    let home = TempHome::new("global-writable");
    init_global(&home);

    let prog = write_program(
        &home.base().join("prog"),
        r#"
workflow("globalwrite", "a writable node against the non-git global project must fail loud")
phase("edit")
agent("edit a file", provider = "claude", writable = True, label = "editor")
"#,
    );
    // NON-dry-run: the gate is in the spawn path and returns before any provider.
    let result = run_global_workflow(&home, &prog, &[]);

    let step = &result["steps"][0];
    assert_eq!(
        step["status"], "failed",
        "writable node in the non-git global project must fail: {step}"
    );
    let summary = step["output_summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains("not a git repository"),
        "names the non-git cause: {summary}"
    );
    assert!(
        summary.contains("_global"),
        "names the offending project id: {summary}"
    );
    assert!(
        summary.contains("get-output") && summary.contains("isolation"),
        "offers the read-only fix: {summary}"
    );
    // It failed at the policy gate (a SETUP error) — never reached a provider turn.
    assert_eq!(
        step["result"]["failure"]["reason"], "spawn",
        "the policy gate is a setup/spawn-class failure, not a provider exit: {step}"
    );
}

#[test]
fn global_isolation_worktree_node_also_fails_loud() {
    // An explicit isolation="worktree" node is rejected even when not `writable` —
    // both need a git worktree the non-git global project cannot provide.
    let home = TempHome::new("global-iso");
    init_global(&home);

    let prog = write_program(
        &home.base().join("prog"),
        r#"
workflow("globaliso", "an isolation=worktree node against the non-git global project must fail loud")
phase("scan")
agent("scan a file", provider = "claude", isolation = "worktree", label = "scanner")
"#,
    );
    // NON-dry-run: the gate is in the spawn path and returns before any provider.
    let result = run_global_workflow(&home, &prog, &[]);

    let step = &result["steps"][0];
    assert_eq!(step["status"], "failed", "isolation node must fail: {step}");
    assert!(
        step["output_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("not a git repository"),
        "names the non-git cause: {step}"
    );
}

#[test]
fn global_readonly_node_runs_successfully() {
    // A read-only (default isolation="none") node needs no worktree, so the non-git
    // policy gate must NOT fire and the node runs to completion in the shared global
    // root. Driven with `--dry-run` so the full pipeline runs WITHOUT spawning a
    // provider (the acceptance: "_global read-only isolation=none nodes run").
    let home = TempHome::new("global-readonly");
    init_global(&home);

    let prog = write_program(
        &home.base().join("prog"),
        r#"
workflow("globalread", "a read-only node against the non-git global project runs successfully")
phase("scan")
agent("just read and report", provider = "claude", label = "reader")
"#,
    );
    let result = run_global_workflow(&home, &prog, &["--dry-run"]);

    let run = &result["run"];
    assert_eq!(
        run["status"], "completed",
        "a read-only node in the non-git global project must run successfully: {run}"
    );
    let step = &result["steps"][0];
    assert_eq!(
        step["status"], "completed",
        "the read-only step completes: {step}"
    );
    let summary = step["output_summary"].as_str().unwrap_or_default();
    assert!(
        !summary.contains("not a git repository"),
        "a read-only node must NOT be rejected by the non-git worktree gate: {summary}"
    );
}
