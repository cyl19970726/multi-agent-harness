//! Proves the workflow completion hook (`HARNESS_WORKFLOW_ON_COMPLETE`) fires when
//! a `WorkflowRun` reaches a terminal status — so a caller (especially one that
//! BACKGROUNDED the run) is notified without polling — and is a strict NO-OP when
//! the env var is unset (zero behavior change for existing runs).
//!
//! Runs the real `harness` binary with an isolated `--store` and a `--dry-run`
//! workflow (no real provider), so it is deterministic and contacts nothing.

use std::fs;
use std::path::Path;
use std::process::Command;

const PROG: &str = "workflow(\"hook-demo\", \"minimal run to prove the on-complete hook fires at the terminal seam\")\noutput(agent(\"say ok\", label=\"leaf\"))\n";

fn harness() -> Command {
    Command::new(env!("CARGO_BIN_EXE_harness"))
}

fn init_store(store: &Path) {
    let out = harness()
        .args(["--store", store.to_str().unwrap(), "init"])
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_WORKFLOW_ON_COMPLETE")
        .output()
        .expect("run init");
    assert!(
        out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn completion_hook_fires_with_run_id_and_status_and_is_noop_without_env() {
    let base = std::env::temp_dir().join(format!(
        "harness-hook-{}-{}",
        std::process::id(),
        env!("CARGO_PKG_NAME")
    ));
    let _ = fs::remove_dir_all(&base);
    let store = base.join("store");
    fs::create_dir_all(&store).unwrap();
    let prog = base.join("hook-demo.star");
    fs::write(&prog, PROG).unwrap();
    init_store(&store);

    // (1) NO env set -> the hook is a strict no-op (no marker file written).
    let noenv_marker = base.join("noenv.txt");
    let out = harness()
        .args([
            "--store",
            store.to_str().unwrap(),
            "workflow",
            "run-script",
            prog.to_str().unwrap(),
            "--dry-run",
        ])
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_WORKFLOW_ON_COMPLETE")
        .output()
        .expect("run-script (no env)");
    assert!(
        out.status.success(),
        "run-script (no env) failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !noenv_marker.exists(),
        "completion hook ran with no HARNESS_WORKFLOW_ON_COMPLETE set — it must be a no-op"
    );

    // (2) Env set -> the hook fires at the terminal seam, receiving the run id +
    // status + name via env. (Backgrounded or not, it fires in the run-owning
    // process at finalization.)
    let marker = base.join("fired.txt");
    let hook_cmd = format!(
        "printf '%s|%s|%s' \"$HARNESS_RUN_ID\" \"$HARNESS_RUN_STATUS\" \"$HARNESS_RUN_NAME\" > {}",
        marker.to_str().unwrap()
    );
    let out = harness()
        .args([
            "--store",
            store.to_str().unwrap(),
            "workflow",
            "run-script",
            prog.to_str().unwrap(),
            "--dry-run",
        ])
        .env_remove("HARNESS_ROOT")
        .env("HARNESS_WORKFLOW_ON_COMPLETE", &hook_cmd)
        .output()
        .expect("run-script (with hook)");
    assert!(
        out.status.success(),
        "run-script (with hook) failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let fired = fs::read_to_string(&marker)
        .expect("completion hook did not fire — marker file was never written");
    let parts: Vec<&str> = fired.split('|').collect();
    assert_eq!(
        parts.len(),
        3,
        "marker should be id|status|name, got {fired:?}"
    );
    assert!(
        parts[0].starts_with("wfrun-"),
        "HARNESS_RUN_ID not a run id: {fired:?}"
    );
    assert_eq!(parts[1], "completed", "HARNESS_RUN_STATUS wrong: {fired:?}");
    assert_eq!(parts[2], "hook-demo", "HARNESS_RUN_NAME wrong: {fired:?}");

    // The run id the hook saw is the SAME run the command journaled.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
    if let Some(rid) = parsed
        .get("run")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
    {
        assert_eq!(parts[0], rid, "hook run id != journaled run id");
    }

    let _ = fs::remove_dir_all(&base);
}
