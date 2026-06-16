//! Integration coverage for the project identity + registry layer
//! (goal-multi-project, project-registry task).
//!
//! These tests drive the real `harness` binary against an ISOLATED harness home
//! (`HARNESS_HOME` + `HOME` pointed at a temp dir) so they never read or write the
//! developer's real `~/.harness`. Pure-logic unit tests (slug/hash/canonical/
//! round-trip) live alongside the code in `src/project.rs`; this file proves the
//! on-disk artifacts (registry.json, metadata.json, ACTIVE_PROJECT) are produced
//! end-to-end by `harness init`.

use std::path::{Path, PathBuf};
use std::process::Command;

mod harness_env;
use harness_env::TempHome;

fn run_init_in(home: &TempHome, cwd: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("init")
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run harness init")
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {path:?}: {e}"))
}

#[test]
fn init_writes_registry_metadata_and_active_marker() {
    let home = TempHome::new("registry-basic");
    // A project dir UNDER home → its id is the HOME-relative slug.
    let proj = home.home().join("repos").join("alpha");
    std::fs::create_dir_all(&proj).unwrap();

    let out = run_init_in(&home, &proj);
    assert!(out.status.success(), "init failed: {:?}", out);

    let id = "repos-alpha";
    // 1. registry.json records the project and marks it current.
    let registry = read_json(&home.registry_path());
    assert_eq!(registry["current_project_id"], serde_json::json!(id));
    assert!(registry["format_version"].as_u64().unwrap() >= 1);
    let entry = registry["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == serde_json::json!(id))
        .expect("registry entry");
    // Entry carries id, canonical path, store_root, kind, created_at, last_opened_at.
    assert_eq!(entry["kind"], serde_json::json!("repo"));
    assert!(!entry["path"].as_str().unwrap().is_empty());
    assert!(entry["store_root"].as_str().unwrap().ends_with(id));
    assert!(!entry["created_at"].as_str().unwrap().is_empty());
    assert!(!entry["last_opened_at"].as_str().unwrap().is_empty());

    // 2. metadata.json pins identity in the central store.
    let store_root = home.projects_dir().join(id);
    let meta = read_json(&store_root.join("metadata.json"));
    assert_eq!(meta["project_id"], serde_json::json!(id));
    assert_eq!(meta["kind"], serde_json::json!("repo"));
    assert!(meta["canonical_path"].as_str().is_some());

    // 3. ACTIVE_PROJECT marker mirrors current.
    let active = std::fs::read_to_string(home.active_marker_path()).unwrap();
    assert_eq!(active.trim(), id);
}

#[test]
fn init_outside_home_uses_content_hash_id() {
    let home = TempHome::new("registry-external");
    // A project OUTSIDE home → id is proj-<hash>. Use a sibling temp dir.
    let external = home.base().join("external-proj");
    std::fs::create_dir_all(&external).unwrap();

    let out = run_init_in(&home, &external);
    assert!(out.status.success(), "init failed: {:?}", out);

    let registry = read_json(&home.registry_path());
    let current = registry["current_project_id"].as_str().unwrap().to_string();
    assert!(
        current.starts_with("proj-"),
        "expected content-hash id, got {current}"
    );
    // Re-running init on the same external path is idempotent (same id, one entry).
    let out2 = run_init_in(&home, &external);
    assert!(out2.status.success());
    let registry2 = read_json(&home.registry_path());
    assert_eq!(
        registry2["current_project_id"].as_str().unwrap(),
        current,
        "id must be stable across init runs"
    );
    let count = registry2["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["id"] == serde_json::json!(current))
        .count();
    assert_eq!(count, 1, "no duplicate registry entries");
}

#[test]
fn home_itself_is_the_reserved_global_project() {
    let home = TempHome::new("registry-global");
    let home_dir: PathBuf = home.home().to_path_buf();
    let out = run_init_in(&home, &home_dir);
    assert!(out.status.success(), "init failed: {:?}", out);

    let registry = read_json(&home.registry_path());
    assert_eq!(
        registry["current_project_id"],
        serde_json::json!("_global"),
        "HOME must map to the reserved _global id"
    );
    let entry = registry["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == serde_json::json!("_global"))
        .expect("global entry");
    assert_eq!(entry["kind"], serde_json::json!("global"));
}
