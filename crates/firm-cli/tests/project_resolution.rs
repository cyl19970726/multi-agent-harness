//! Integration coverage for store resolution precedence
//! (goal-multi-project, project-resolution task).
//!
//! Drives the real `harness` binary with `--store-source` (a debug flag that
//! prints `store-source: <Source> root=<path>` to stderr) so each precedence rung
//! is observable without a live store. Runs against an isolated HOME so the
//! developer's real `~/.firm` is never read.

use std::path::Path;
use std::process::Command;

mod firm_env;
use firm_env::TempHome;

/// Run `harness --store-source <args...>` in `cwd` with the given extra env, and
/// return (stdout, stderr). `mission list` is a harmless read used as the subcommand.
fn resolve(
    home: &TempHome,
    cwd: &Path,
    extra_args: &[&str],
    extra_env: &[(&str, &str)],
) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_firm"));
    cmd.arg("--store-source");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.arg("mission").arg("list");
    cmd.current_dir(cwd)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE");
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
    let out = Command::new(env!("CARGO_BIN_EXE_firm"))
        .arg("init")
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .output()
        .expect("run init");
    assert!(out.status.success(), "init failed: {:?}", out);
}

/// After `init`, two commands from DIFFERENT cwds resolve the SAME native
/// Execution Space. cwd only affects provider workspace discovery when no
/// explicit Project Binding is selected.
#[test]
fn commands_from_different_cwds_converge_via_registry() {
    let home = TempHome::new("res-converge");
    let proj = home.base().join("repo");
    let sub = proj.join("a").join("b");
    std::fs::create_dir_all(&sub).unwrap();
    init(&home, &proj);

    let (_o1, e1) = resolve(&home, &proj, &[], &[]);
    let (_o2, e2) = resolve(&home, &sub, &[], &[]);
    assert!(
        e1.contains("SpaceCurrent"),
        "expected SpaceCurrent from project root, got: {e1}"
    );
    assert!(
        e2.contains("SpaceCurrent"),
        "expected SpaceCurrent from subdir, got: {e2}"
    );
    // Same central store root regardless of cwd.
    let root1 = root_of(&e1);
    let root2 = root_of(&e2);
    assert_eq!(root1, root2, "stores diverged: {root1} vs {root2}");
    assert!(
        root1.contains("/execution-spaces/"),
        "not an Execution Space store: {root1}"
    );
}

#[test]
fn dynamic_workflow_cli_is_explicitly_retired() {
    let home = TempHome::new("dynamic-workflow-retired");
    let project = home.base().join("repo");
    std::fs::create_dir_all(&project).unwrap();
    init(&home, &project);

    let output = Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(["workflow", "list"])
        .current_dir(&project)
        .envs(home.envs())
        .output()
        .expect("run retired workflow command");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("`harness workflow` is retired"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn active_execution_space_wins_over_legacy_cwd_store() {
    // An explicitly active Execution Space is cwd-independent. A legacy
    // repo-local `.harness` remains available only when no native space is active.
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
        e.contains("SpaceCurrent"),
        "active Execution Space must win over cwd compatibility storage, got: {e}"
    );
    assert!(root_of(&e).contains("/execution-spaces/"), "stderr: {e}");
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
fn firm_root_is_canonical_and_wins_over_harness_root() {
    let home = TempHome::new("res-firm-root");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    init(&home, &proj);

    let (_o, e) = resolve(
        &home,
        &proj,
        &[],
        &[
            ("FIRM_ROOT", "/tmp/firm-root-canonical"),
            ("HARNESS_ROOT", "/tmp/harness-root-alias"),
        ],
    );
    assert!(e.contains("FirmRootEnv"), "stderr: {e}");
    assert!(
        root_of(&e).contains("/tmp/firm-root-canonical"),
        "stderr: {e}"
    );
    assert!(!e.contains("HARNESS_ROOT is deprecated"), "stderr: {e}");
}

#[test]
fn project_flag_selects_binding_without_switching_execution_store() {
    let home = TempHome::new("res-project-flag");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    init(&home, &proj); // registers an id we can select

    // Read the registered id back from the registry to select it explicitly.
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let id = registry["current_project_id"].as_str().unwrap().to_string();

    // From an unrelated cwd, --project <id> selects the provider binding but
    // leaves coordination in the current Execution Space.
    let (_o, e) = resolve(&home, home.base(), &["--project", &id], &[]);
    assert!(e.contains("SpaceCurrent"), "stderr: {e}");
    assert!(root_of(&e).contains("/execution-spaces/"), "stderr: {e}");
}

#[test]
fn project_env_selects_binding_without_switching_execution_store() {
    let home = TempHome::new("res-project-env");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    init(&home, &proj);
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let id = registry["current_project_id"].as_str().unwrap().to_string();

    let (_o, e) = resolve(
        &home,
        home.base(),
        &[],
        &[
            ("FIRM_PROJECT", &id),
            ("HARNESS_PROJECT", "missing-project-alias"),
        ],
    );
    assert!(e.contains("SpaceCurrent"), "stderr: {e}");
    assert!(root_of(&e).contains("/execution-spaces/"), "stderr: {e}");
    assert!(!e.contains("HARNESS_PROJECT is deprecated"), "stderr: {e}");

    let (_o, alias_stderr) = resolve(&home, home.base(), &[], &[("HARNESS_PROJECT", &id)]);
    assert!(
        alias_stderr.contains("SpaceCurrent"),
        "stderr: {alias_stderr}"
    );
    assert!(
        alias_stderr.contains("HARNESS_PROJECT is deprecated; prefer `FIRM_PROJECT`"),
        "stderr: {alias_stderr}"
    );
}

#[test]
fn firm_space_is_canonical_and_harness_space_is_a_fallback() {
    let home = TempHome::new("res-space-env-precedence");
    let first = home.base().join("first");
    let second = home.base().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    init(&home, &first);
    let mut spaces_after_first: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.space_registry_path()).unwrap())
            .unwrap();
    let first_space = spaces_after_first["current_space_id"]
        .as_str()
        .unwrap()
        .to_string();
    spaces_after_first["current_space_id"] = serde_json::Value::Null;
    std::fs::write(
        home.space_registry_path(),
        serde_json::to_vec_pretty(&spaces_after_first).unwrap(),
    )
    .unwrap();
    match std::fs::remove_file(home.active_space_marker_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("clear active space marker: {error}"),
    }
    init(&home, &second);
    let spaces_after_second: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.space_registry_path()).unwrap())
            .unwrap();
    let second_space = spaces_after_second["current_space_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(first_space, second_space);

    let (_o, canonical_stderr) = resolve(
        &home,
        home.base(),
        &[],
        &[
            ("FIRM_SPACE", &first_space),
            ("HARNESS_SPACE", &second_space),
        ],
    );
    assert!(
        canonical_stderr.contains("SpaceEnv"),
        "stderr: {canonical_stderr}"
    );
    assert!(
        root_of(&canonical_stderr).ends_with(&first_space),
        "stderr: {canonical_stderr}"
    );
    assert!(!canonical_stderr.contains("HARNESS_SPACE is deprecated"));

    let (_o, alias_stderr) = resolve(&home, home.base(), &[], &[("HARNESS_SPACE", &first_space)]);
    assert!(
        root_of(&alias_stderr).ends_with(&first_space),
        "stderr: {alias_stderr}"
    );
    assert!(
        alias_stderr.contains("HARNESS_SPACE is deprecated; prefer `FIRM_SPACE`"),
        "stderr: {alias_stderr}"
    );
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
