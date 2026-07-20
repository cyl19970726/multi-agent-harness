//! Integration coverage for store resolution precedence
//! (goal-multi-project, project-resolution task).
//!
//! Drives the real `harness` binary with `--store-source` (a debug flag that
//! prints `store-source: <Source> root=<path>` to stderr) so each precedence rung
//! is observable without a live store. Runs against an isolated HOME so the
//! developer's real `~/.harness` is never read.

use std::path::Path;
use std::process::Command;

mod harness_env;
use harness_env::TempHome;

/// Run `harness --store-source <args...>` in `cwd` with the given extra env, and
/// return (stdout, stderr). `mission list` is a harmless read used as the subcommand.
fn resolve(
    home: &TempHome,
    cwd: &Path,
    extra_args: &[&str],
    extra_env: &[(&str, &str)],
) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
    cmd.arg("--store-source");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.arg("mission").arg("list");
    cmd.current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run harness");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn init(home: &TempHome, cwd: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("init")
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run init");
    assert!(out.status.success(), "init failed: {:?}", out);
}

/// #89 invariant: after `init`, two commands from DIFFERENT cwds resolve the SAME
/// central store via the registry's current project (replacing "shared cwd").
#[test]
fn serve_and_run_script_converge_via_registry() {
    let home = TempHome::new("res-converge");
    let proj = home.base().join("repo");
    let sub = proj.join("a").join("b");
    std::fs::create_dir_all(&sub).unwrap();
    init(&home, &proj);

    let (_o1, e1) = resolve(&home, &proj, &[], &[]);
    let (_o2, e2) = resolve(&home, &sub, &[], &[]);
    assert!(
        e1.contains("RegistryCurrent"),
        "expected RegistryCurrent from project root, got: {e1}"
    );
    assert!(
        e2.contains("RegistryCurrent"),
        "expected RegistryCurrent from subdir, got: {e2}"
    );
    // Same central store root regardless of cwd.
    let root1 = root_of(&e1);
    let root2 = root_of(&e2);
    assert_eq!(root1, root2, "stores diverged: {root1} vs {root2}");
    assert!(root1.contains("/projects/"), "not a central store: {root1}");
}

#[test]
fn local_store_wins_over_active_registry_project() {
    // Regression (review MAJOR): a PRESENT repo-local `.harness` must win over the
    // registry-current project, so standing inside a legacy repo never silently
    // shadows its own goals/tasks with an unrelated active project.
    let home = TempHome::new("res-local-wins");
    // Activate a central project elsewhere → registry has a current_project_id.
    let other = home.base().join("other-proj");
    std::fs::create_dir_all(&other).unwrap();
    init(&home, &other);

    // A legacy repo carrying its OWN repo-local `.harness`.
    let repo = home.base().join("legacy-repo");
    std::fs::create_dir_all(repo.join(".harness")).unwrap();

    let (_o, e) = resolve(&home, &repo, &[], &[]);
    assert!(
        e.contains("CwdWalkUp"),
        "a present local .harness must win over the active project, got: {e}"
    );
    assert!(root_of(&e).ends_with(".harness"), "stderr: {e}");
}

#[test]
fn store_flag_overrides_and_warns() {
    let home = TempHome::new("res-store-flag");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    init(&home, &proj); // make registry non-empty so we prove the override wins

    let (_o, e) = resolve(&home, &proj, &["--store", "/tmp/explicit-xyz"], &[]);
    assert!(e.contains("StoreFlag"), "stderr: {e}");
    assert!(root_of(&e).contains("/tmp/explicit-xyz"), "stderr: {e}");
    assert!(
        e.contains("deprecated"),
        "expected deprecation warning, got: {e}"
    );
}

#[test]
fn harness_root_env_overrides_and_warns() {
    let home = TempHome::new("res-harness-root");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    init(&home, &proj);

    let (_o, e) = resolve(&home, &proj, &[], &[("HARNESS_ROOT", "/tmp/hr-xyz")]);
    assert!(e.contains("HarnessRootEnv"), "stderr: {e}");
    assert!(root_of(&e).contains("/tmp/hr-xyz"), "stderr: {e}");
    assert!(e.contains("deprecated"), "stderr: {e}");
}

#[test]
fn project_flag_selects_by_id() {
    let home = TempHome::new("res-project-flag");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    init(&home, &proj); // registers an id we can select

    // Read the registered id back from the registry to select it explicitly.
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let id = registry["current_project_id"].as_str().unwrap().to_string();

    // From an unrelated cwd, --project <id> still resolves that project.
    let (_o, e) = resolve(&home, home.base(), &["--project", &id], &[]);
    assert!(e.contains("ProjectFlag"), "stderr: {e}");
    assert!(root_of(&e).ends_with(&id), "stderr: {e}");
}

#[test]
fn project_env_selects_by_id() {
    let home = TempHome::new("res-project-env");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    init(&home, &proj);
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let id = registry["current_project_id"].as_str().unwrap().to_string();

    let (_o, e) = resolve(&home, home.base(), &[], &[("HARNESS_PROJECT", &id)]);
    assert!(e.contains("ProjectEnv"), "stderr: {e}");
    assert!(root_of(&e).ends_with(&id), "stderr: {e}");
}

#[test]
fn legacy_cwd_walk_up_is_warned_fallback() {
    let home = TempHome::new("res-walkup");
    // No project ever activated → empty registry. A repo-local `.harness` exists.
    let repo = home.base().join("legacy-repo");
    let local_store = repo.join(".harness");
    std::fs::create_dir_all(&local_store).unwrap();
    let sub = repo.join("deep").join("nested");
    std::fs::create_dir_all(&sub).unwrap();

    let (_o, e) = resolve(&home, &sub, &[], &[]);
    assert!(e.contains("CwdWalkUp"), "stderr: {e}");
    assert!(e.contains("deprecated"), "stderr: {e}");
    // Resolves to the nearest ancestor `.harness`, preserving #89 back-compat.
    assert!(root_of(&e).ends_with(".harness"), "stderr: {e}");
}

#[test]
fn global_default_when_nothing_selected() {
    let home = TempHome::new("res-global");
    // Empty registry AND no local `.harness` up the tree → reserved _global.
    let bare = home.base().join("bare").join("dir");
    std::fs::create_dir_all(&bare).unwrap();

    let (_o, e) = resolve(&home, &bare, &[], &[]);
    assert!(e.contains("GlobalDefault"), "stderr: {e}");
    assert!(root_of(&e).ends_with("/projects/_global"), "stderr: {e}");
}

/// Extract the `root=<path>` value from a `store-source: ... root=<path>` line.
fn root_of(stderr: &str) -> String {
    stderr
        .lines()
        .find(|l| l.contains("store-source:"))
        .and_then(|l| l.split("root=").nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| panic!("no store-source line in stderr: {stderr}"))
}
