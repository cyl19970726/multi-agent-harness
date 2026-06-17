//! Integration coverage for the `harness project` command group
//! (goal-multi-project, project-cli task).
//!
//! Exercises add / list / current / switch / remove against an isolated HOME so the
//! developer's real `~/.harness` is never touched. The same registry/marker code is
//! consumed by `serve` and `resolve_store`, so a `switch` here is the convergence
//! point a live serve reads (#89 invariant).

use std::path::Path;

mod harness_env;
use harness_env::{run_harness, TempHome};

/// Parse a `harness project ...` JSON stdout into a `serde_json::Value`.
fn json_out(out: &std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout not JSON ({e}): {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// `harness project current` id (or `None` when null).
fn current_id(home: &TempHome, cwd: &Path) -> Option<String> {
    let out = run_harness(home, cwd, &["project", "current"]);
    assert!(out.status.success(), "project current failed: {out:?}");
    json_out(&out)["id"].as_str().map(str::to_string)
}

#[test]
fn add_registers_without_switching_then_switch_makes_current() {
    let home = TempHome::new("proj-add");
    let repo_a = home.home().join("repoA");
    let repo_b = home.home().join("repoB");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::create_dir_all(&repo_b).unwrap();

    // `add` registers but does NOT flip the active project (inspectable first).
    let out = run_harness(&home, &repo_a, &["project", "add"]);
    assert!(out.status.success(), "add failed: {out:?}");
    let added = json_out(&out);
    assert_eq!(added["id"], "repoA");
    assert_eq!(added["is_current"], false);
    assert_eq!(current_id(&home, home.base()), None, "add must not switch");

    // The registry knows the project even though it is not current.
    let listed = json_out(&run_harness(&home, home.base(), &["project", "list"]));
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"repoA"), "repoA missing from list: {ids:?}");
    assert!(ids.contains(&"_global"), "_global missing: {ids:?}");

    // `add --switch` registers AND makes current.
    let out = run_harness(&home, &repo_b, &["project", "add", "--switch"]);
    assert!(out.status.success(), "add --switch failed: {out:?}");
    assert_eq!(current_id(&home, home.base()).as_deref(), Some("repoB"));
}

#[test]
fn switch_updates_registry_and_active_marker_consistently() {
    let home = TempHome::new("proj-switch");
    let repo_a = home.home().join("alpha");
    std::fs::create_dir_all(&repo_a).unwrap();

    // Switching to a not-yet-registered PATH registers + activates it.
    let out = run_harness(
        &home,
        home.base(),
        &["project", "switch", repo_a.to_str().unwrap()],
    );
    assert!(out.status.success(), "switch by path failed: {out:?}");
    assert_eq!(current_id(&home, home.base()).as_deref(), Some("alpha"));

    // The ACTIVE_PROJECT marker and the registry's current_project_id agree.
    let marker = std::fs::read_to_string(home.active_marker_path()).unwrap();
    assert_eq!(marker.trim(), "alpha");
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    assert_eq!(registry["current_project_id"], "alpha");

    // Switching to the reserved _global works even though it is never registered.
    let out = run_harness(&home, home.base(), &["project", "switch", "_global"]);
    assert!(out.status.success(), "switch _global failed: {out:?}");
    assert_eq!(current_id(&home, home.base()).as_deref(), Some("_global"));

    // Switching to an unknown id is rejected (never strands the pointer).
    let out = run_harness(&home, home.base(), &["project", "switch", "no-such-id"]);
    assert!(!out.status.success(), "switch to unknown id should fail");
    assert_eq!(
        current_id(&home, home.base()).as_deref(),
        Some("_global"),
        "rejected switch must leave the active project unchanged"
    );
}

#[test]
fn remove_refuses_current_without_force_and_protects_global() {
    let home = TempHome::new("proj-remove");
    let repo = home.home().join("torem");
    std::fs::create_dir_all(&repo).unwrap();
    run_harness(&home, &repo, &["project", "add", "--switch"]);
    assert_eq!(current_id(&home, home.base()).as_deref(), Some("torem"));

    // Removing the CURRENT project without --force is refused.
    let out = run_harness(&home, home.base(), &["project", "remove", "torem"]);
    assert!(
        !out.status.success(),
        "remove current without --force should fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("current project"), "stderr: {stderr}");
    // Still current after the refusal.
    assert_eq!(current_id(&home, home.base()).as_deref(), Some("torem"));

    // The reserved _global can never be removed.
    let out = run_harness(&home, home.base(), &["project", "remove", "_global"]);
    assert!(!out.status.success(), "remove _global should fail");

    // --force removes the current project and clears the active pointer.
    let out = run_harness(
        &home,
        home.base(),
        &["project", "remove", "torem", "--force"],
    );
    assert!(out.status.success(), "remove --force failed: {out:?}");
    let removed = json_out(&out);
    assert_eq!(removed["removed"], "torem");
    assert_eq!(removed["was_current"], true);
    // No project is current now; resolution falls back safely.
    assert_eq!(current_id(&home, home.base()), None);
    // The registry no longer pins the project (the on-disk store is intentionally
    // left intact — `remove` is a pointer operation, not a destructive delete).
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let registered: Vec<&str> = registry["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(
        !registered.contains(&"torem"),
        "removed project still in registry: {registered:?}"
    );
}

#[test]
fn remove_unknown_id_is_an_error() {
    let home = TempHome::new("proj-remove-unknown");
    let out = run_harness(&home, home.base(), &["project", "remove", "ghost"]);
    assert!(!out.status.success(), "remove of unknown id should fail");
}

#[test]
fn current_is_null_before_any_selection() {
    let home = TempHome::new("proj-current-null");
    // A fresh HOME with no registry and no cwd .harness → no project selected.
    assert_eq!(current_id(&home, home.base()), None);
}

#[test]
fn subcommands_are_documented_in_help() {
    let home = TempHome::new("proj-help");
    let out = run_harness(&home, home.base(), &["help"]);
    assert!(out.status.success(), "help failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    for sub in [
        "project add",
        "project list",
        "project current",
        "project switch",
        "project remove",
        "project show",
        "project migrate",
    ] {
        assert!(stdout.contains(sub), "help missing `{sub}`:\n{stdout}");
    }
}
