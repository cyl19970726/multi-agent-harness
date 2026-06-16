//! Integration coverage for `harness init` routing
//! (goal-multi-project, init-routing task).
//!
//! `init` must register the selected/cwd project centrally (registry + metadata)
//! instead of blindly creating `./.harness`, must NOT silently adopt an ancestor's
//! local `.harness`, and must preserve the `--store`/`HARNESS_ROOT` override for
//! back-compat. Runs against an isolated HOME.

use std::path::Path;
use std::process::Command;

mod harness_env;
use harness_env::TempHome;

fn init_in(home: &TempHome, cwd: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
    cmd.arg("init");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run init")
}

#[test]
fn init_registers_central_project_not_local_dot_harness() {
    let home = TempHome::new("init-central");
    let proj = home.base().join("myrepo");
    std::fs::create_dir_all(&proj).unwrap();

    let out = init_in(&home, &proj, &[]);
    assert!(out.status.success(), "init failed: {:?}", out);

    // Registry entry + central store + metadata exist.
    assert!(home.registry_path().exists(), "registry.json missing");
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let id = registry["current_project_id"].as_str().unwrap().to_string();
    let store_root = home.projects_dir().join(&id);
    assert!(
        store_root.is_dir(),
        "central store not created: {store_root:?}"
    );
    assert!(
        store_root.join("metadata.json").exists(),
        "metadata.json missing"
    );

    // It did NOT create a local `./.harness` as the canonical store.
    assert!(
        !proj.join(".harness").exists(),
        "init must not materialize a local ./.harness canonical store"
    );
}

#[test]
fn init_does_not_adopt_ancestor_local_dot_harness() {
    let home = TempHome::new("init-no-adopt");
    // An ancestor already has a local `.harness` (a legacy repo).
    let ancestor = home.base().join("legacy");
    std::fs::create_dir_all(ancestor.join(".harness")).unwrap();
    let child = ancestor.join("child");
    std::fs::create_dir_all(&child).unwrap();

    let out = init_in(&home, &child, &[]);
    assert!(out.status.success(), "init failed: {:?}", out);

    // The new project's id is derived from the CHILD path, not the ancestor's,
    // and its store is central — the ancestor `.harness` is not adopted.
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let entry = registry["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == registry["current_project_id"])
        .expect("entry");
    let canonical = entry["path"].as_str().unwrap();
    assert!(
        canonical.ends_with("child"),
        "init registered the wrong root (adopted ancestor?): {canonical}"
    );
    // The ancestor's local store_root is NOT used as the central store_root.
    let store_root = entry["store_root"].as_str().unwrap();
    assert!(
        store_root.contains("/projects/"),
        "store_root is not central: {store_root}"
    );
    assert!(
        !store_root.ends_with("legacy/.harness"),
        "init adopted ancestor local .harness: {store_root}"
    );
}

#[test]
fn init_store_override_uses_raw_path_for_backcompat() {
    let home = TempHome::new("init-override");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();
    let explicit = home.base().join("explicit-store");

    let out = init_in(&home, &proj, &["--store", explicit.to_str().unwrap()]);
    assert!(out.status.success(), "init failed: {:?}", out);

    // Historical behavior: the explicit path is materialized directly.
    assert!(
        explicit.is_dir(),
        "explicit store not created: {explicit:?}"
    );
    assert!(explicit.join("provider-sessions").is_dir());
    // No central registry entry is created for a raw `--store` override.
    assert!(
        !home.registry_path().exists(),
        "raw --store init must not write the central registry"
    );
    // Deprecation warning is emitted.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("deprecated"), "stderr: {stderr}");
}

#[test]
fn init_is_idempotent_for_same_root() {
    let home = TempHome::new("init-idempotent");
    let proj = home.base().join("repo");
    std::fs::create_dir_all(&proj).unwrap();

    assert!(init_in(&home, &proj, &[]).status.success());
    assert!(init_in(&home, &proj, &[]).status.success());

    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).unwrap()).unwrap();
    let id = registry["current_project_id"].as_str().unwrap();
    let count = registry["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["id"] == serde_json::json!(id))
        .count();
    assert_eq!(count, 1, "duplicate registry entries after re-init");
}
