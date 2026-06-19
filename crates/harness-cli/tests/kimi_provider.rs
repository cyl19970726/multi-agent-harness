//! Deterministic end-to-end proof that Kimi is a real, registry-routed third
//! provider (goal-provider-neutral, stage b-kimi / S4) — WITHOUT touching a real
//! Kimi endpoint or the network.
//!
//! The chain under test is exactly the one the design names as the primary gap:
//!
//!   Task.executor="kimi"
//!     -> compile_phase_to_starlark  (emits `agent(provider="kimi", ...)`)
//!       -> provider_adapter("kimi") (resolves KimiAdapter from the registry)
//!         -> spawn_kimi_ephemeral   (spawns a `kimi` CLI by BARE NAME)
//!
//! A fake `kimi` shim is placed first on PATH so the bare-name spawn intercepts
//! it. The shim emits a claude-shaped stream-json success stream (a `system`
//! init + an `assistant` message + a terminal `result`), which is exactly the
//! wire format KimiAdapter assumes, so the phase's leaf SUCCEEDS deterministically
//! and the run is journaled with `provider="kimi"`.
//!
//! Asserting:
//!   1. the fake `kimi` shim actually RAN (it recorded its cwd) — proving the
//!      bare-name spawn reached a `kimi` binary, not codex/claude;
//!   2. a journaled WorkflowStep carries `provider="kimi"` — proving the
//!      Task.executor -> compile -> registry -> spawn chain end-to-end;
//!   3. the orchestration did NOT fail.
//!   4. the DURABLE per-session AgentEvent trace is attributed to provider="kimi"
//!      and NOT misattributed to provider="claude". Kimi is claude-shaped on the
//!      wire and reuses the claude reducer; this guards that the reducer is told
//!      which provider it is reducing for, so trace/dashboard queries that group
//!      by AgentEvent.provider don't credit Kimi turns to claude.
//!
//! NB: this proves the PLUMBING only. The real `kimi` CLI flags + stream format
//! are an operator/S3-spike concern and are deliberately out of scope here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod harness_env;
use harness_env::TempHome;

/// Install a fake `kimi` binary (first on PATH) that records its cwd and emits a
/// claude-shaped stream-json SUCCESS stream so `infer_claude_session_status`
/// returns `Succeeded`. Returns the bin dir to prepend to PATH.
fn install_fake_kimi(base: &Path, cwd_marker: &Path) -> PathBuf {
    let bin_dir = base.join("fakebin-kimi");
    fs::create_dir_all(&bin_dir).expect("mk fake kimi bin dir");
    let shim_path = bin_dir.join("kimi");
    // POSIX shell shim. `pwd -P` records the spawn cwd. The three NDJSON frames
    // are the claude-shaped success shape KimiAdapter parses: system init,
    // assistant message, terminal result.
    let marker = cwd_marker.display().to_string();
    let script = format!(
        "#!/bin/sh\npwd -P > '{marker}'\n\
         printf '%s\\n' '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"kimi-sess-1\",\"model\":\"kimi-for-coding\"}}'\n\
         printf '%s\\n' '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"pong from fake kimi\"}}]}}}}'\n\
         printf '%s\\n' '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"pong from fake kimi\",\"usage\":{{\"input_tokens\":3,\"output_tokens\":4}}}}'\n\
         exit 0\n",
        marker = marker.replace('\'', "'\\''"),
    );
    fs::write(&shim_path, script).expect("write kimi shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir
}

/// Run the real `harness` binary `--project <root>` with the fake kimi shim ahead
/// of the real PATH.
fn run_harness(
    home: &TempHome,
    project_root: &Path,
    fake_bin: &Path,
    args: &[&str],
) -> (bool, String, String) {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--project")
        .arg(project_root)
        .args(args)
        .current_dir(home.base())
        .envs(home.envs())
        .env("PATH", path)
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run harness");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Locate the project store's `workflow_steps.jsonl` by walking the harness home
/// (the centralized `~/.harness/projects/<id>/...` layout).
fn find_workflow_steps(home: &TempHome) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("workflow_steps.jsonl") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(home.harness_home(), &mut files);
    let mut lines = Vec::new();
    for f in files {
        if let Ok(text) = fs::read_to_string(&f) {
            for line in text.lines() {
                if !line.trim().is_empty() {
                    lines.push(line.to_string());
                }
            }
        }
    }
    lines
}

/// Locate every `agent_events.jsonl` row written under the harness home (the
/// durable per-session AgentEvent trace).
fn find_agent_events(home: &TempHome) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("agent_events.jsonl") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(home.harness_home(), &mut files);
    let mut lines = Vec::new();
    for f in files {
        if let Ok(text) = fs::read_to_string(&f) {
            for line in text.lines() {
                if !line.trim().is_empty() {
                    lines.push(line.to_string());
                }
            }
        }
    }
    lines
}

#[test]
fn task_executor_kimi_compiles_dispatches_and_spawns_a_kimi_by_name() {
    let home = TempHome::new("kimi-exec");
    // A git project root so `goal run-phases` has a clean repo to operate on. The
    // single task is READ-ONLY (no owned_paths), so no worktree/landing is
    // involved — the phase is just a registry-routed spawn.
    let project_root = home.base().join("kimi-proj");
    fs::create_dir_all(&project_root).unwrap();
    // Minimal git repo (run-phases works against a repo root).
    for git_args in [
        &["init", "-q"][..],
        &["config", "user.email", "kimi@test.local"][..],
        &["config", "user.name", "kimi-test"][..],
    ] {
        let ok = Command::new("git")
            .args(git_args)
            .current_dir(&project_root)
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok, "git {:?} failed", git_args);
    }
    fs::write(project_root.join("README.md"), "kimi proof\n").unwrap();
    let commit = Command::new("git")
        .args([
            "-c",
            "user.email=kimi@test.local",
            "-c",
            "user.name=kimi-test",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ])
        .current_dir(&project_root)
        .output()
        .expect("git commit");
    assert!(commit.status.success(), "git commit failed: {:?}", commit);

    let cwd_marker = home.base().join("kimi-cwd.txt");
    let fake_bin = install_fake_kimi(home.base(), &cwd_marker);

    // init + register the project.
    let (ok, _o, e) = run_harness(&home, &project_root, &fake_bin, &["init"]);
    assert!(ok, "init failed: {e}");

    // Build a goal with one phase and one READ-ONLY task whose executor = kimi.
    let (ok, _o, e) = run_harness(
        &home,
        &project_root,
        &fake_bin,
        &[
            "goal",
            "create",
            "--id",
            "g-kimi",
            "--title",
            "Kimi proof",
            "--owner",
            "lead",
        ],
    );
    assert!(ok, "goal create failed: {e}");

    let (ok, _o, e) = run_harness(
        &home,
        &project_root,
        &fake_bin,
        &[
            "goal",
            "phase-add",
            "--goal",
            "g-kimi",
            "--phase-id",
            "p1",
            "--name",
            "phase one",
            "--intent",
            "run a single kimi leaf",
        ],
    );
    assert!(ok, "phase-add failed: {e}");

    let (ok, _o, e) = run_harness(
        &home,
        &project_root,
        &fake_bin,
        &[
            "task",
            "create",
            "--id",
            "t-kimi",
            "--goal",
            "g-kimi",
            "--phase-id",
            "p1",
            "--title",
            "say pong",
            "--objective",
            "reply pong",
            "--owner",
            "lead",
            "--executor",
            "kimi",
        ],
    );
    assert!(ok, "task create failed: {e}");

    // Run the phase. This compiles p1 -> `.star` (agent(provider="kimi", ...)) and
    // dispatches the leaf through provider_adapter("kimi") -> spawn_kimi_ephemeral.
    let (ok, stdout, stderr) = run_harness(
        &home,
        &project_root,
        &fake_bin,
        &["goal", "run-phases", "g-kimi", "--timeout-ms", "10000"],
    );
    assert!(
        ok,
        "goal run-phases failed (the kimi leaf should succeed via the fake shim).\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // (1) The fake `kimi` shim actually ran — bare-name spawn reached a `kimi`
    // binary, not codex/claude.
    let recorded_cwd = fs::read_to_string(&cwd_marker).unwrap_or_else(|err| {
        panic!("fake kimi shim never recorded a cwd ({err}); it was not spawned")
    });
    assert!(
        !recorded_cwd.trim().is_empty(),
        "fake kimi shim recorded an empty cwd"
    );

    // (2) A journaled WorkflowStep carries provider="kimi" — the executor flowed
    // all the way through compile -> registry -> spawn -> journal.
    let step_lines = find_workflow_steps(&home);
    assert!(
        !step_lines.is_empty(),
        "no workflow_steps.jsonl rows journaled under {:?}",
        home.harness_home()
    );
    let kimi_step = step_lines.iter().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| {
                v.get("result")
                    .and_then(|r| r.get("provider"))
                    .and_then(|p| p.as_str())
                    .map(|p| p == "kimi")
            })
            .unwrap_or(false)
    });
    assert!(
        kimi_step,
        "no journaled WorkflowStep had result.provider == \"kimi\"; steps:\n{}",
        step_lines.join("\n")
    );

    // (3) The DURABLE per-session AgentEvent trace is attributed to provider="kimi"
    // and never to provider="claude". The default run retains the trace, so
    // KimiAdapter::ingest_ephemeral_trace ran the claude-shaped reducer; this guards
    // that the reducer stamped our provider id, not its hardcoded "claude".
    let event_lines = find_agent_events(&home);
    assert!(
        !event_lines.is_empty(),
        "no agent_events.jsonl rows journaled under {:?} (durable trace path did not run)",
        home.harness_home()
    );
    let event_provider = |line: &str| -> Option<String> {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| {
                v.get("provider")
                    .and_then(|p| p.as_str())
                    .map(str::to_string)
            })
    };
    let any_kimi_event = event_lines
        .iter()
        .filter_map(|l| event_provider(l))
        .any(|p| p == "kimi");
    assert!(
        any_kimi_event,
        "no durable AgentEvent carried provider == \"kimi\"; events:\n{}",
        event_lines.join("\n")
    );
    let misattributed: Vec<&String> = event_lines
        .iter()
        .filter(|l| event_provider(l).as_deref() == Some("claude"))
        .collect();
    assert!(
        misattributed.is_empty(),
        "durable AgentEvent(s) misattributed to provider == \"claude\" for a kimi run:\n{}",
        misattributed
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
