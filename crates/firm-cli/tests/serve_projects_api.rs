//! Integration coverage for independent Execution Space and Project Binding
//! selectors in the serve HTTP API.
//!
//! Spawns the real `harness serve` against an isolated HOME with TWO registered
//! projects, then asserts:
//!   - `GET /v1/projects` lists both registry projects + the reserved `_global`,
//!   - `GET /v1/projects/current` reflects the registry's active project,
//!   - `GET /v1/snapshot?space=<id>` reads that coordination store,
//!   - `?project=<id>` only selects provider cwd/config/Skill boundaries,
//!   - project and space switches update their independent active markers.

use std::path::Path;

mod firm_env;
use firm_env::{current_project_id, current_space_id, run_firm, ServeHandle, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its derived id.
fn init_project(home: &TempHome, name: &str) -> (std::path::PathBuf, String) {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let id = current_project_id(home);
    (root, id)
}

fn create_space(home: &TempHome, id: &str, project_binding: &str) {
    let out = run_firm(
        home,
        home.base(),
        &[
            "space",
            "init",
            "--id",
            id,
            "--name",
            id,
            "--project-binding",
            project_binding,
        ],
    );
    assert!(out.status.success(), "space init failed: {out:?}");
}

/// Seed one historical Mission row in a specific Execution Space (DOC-108
/// retired the `mission create` writer this fixture used; pre-cutover rows
/// are the only Missions that exist).
fn create_goal(home: &TempHome, space_id: &str, project_id: &str, goal_id: &str, title: &str) {
    let _ = project_id;
    firm_env::seed_historical_mission(home, space_id, goal_id, title);
}

fn goal_ids(snapshot: &serde_json::Value) -> Vec<String> {
    snapshot["missions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| g["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn projects_endpoint_lists_registry_projects_and_global() {
    let home = TempHome::new("api-list");
    let (_a, id_a) = init_project(&home, "alpha");
    let (_b, id_b) = init_project(&home, "beta");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, body) = serve.get_json("/v1/projects");
    assert_eq!(status, 200, "body: {body}");

    let ids: Vec<String> = body["projects"]
        .as_array()
        .expect("projects array")
        .iter()
        .filter_map(|p| p["id"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(ids.contains(&id_a), "missing {id_a} in {ids:?}");
    assert!(ids.contains(&id_b), "missing {id_b} in {ids:?}");
    assert!(
        ids.iter().any(|i| i == "_global"),
        "reserved _global missing in {ids:?}"
    );
}

#[test]
fn current_endpoint_reflects_active_project() {
    let home = TempHome::new("api-current");
    let (_a, id_a) = init_project(&home, "alpha");
    // beta init makes beta the active project (init activates the last-inited).
    let (_b, id_b) = init_project(&home, "beta");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, body) = serve.get_json("/v1/projects/current");
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["current"].as_str(),
        Some(id_b.as_str()),
        "current should be the last-inited project; a={id_a} b={id_b}; body={body}"
    );
}

#[test]
fn snapshot_space_selector_isolated_while_project_selector_does_not_switch_store() {
    let home = TempHome::new("api-scoped");
    let (_a, id_a) = init_project(&home, "alpha");
    let (_b, id_b) = init_project(&home, "beta");
    create_space(&home, "space-alpha", &id_a);
    create_space(&home, "space-beta", &id_b);
    create_goal(&home, "space-alpha", &id_a, "goal-in-alpha", "Alpha goal");
    create_goal(&home, "space-beta", &id_b, "goal-in-beta", "Beta goal");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (sa, snap_a) = serve.get_json(&format!("/v1/snapshot?space=space-alpha&project={id_a}"));
    assert_eq!(sa, 200);
    let ga = goal_ids(&snap_a);
    assert!(
        ga.contains(&"goal-in-alpha".to_string()),
        "alpha goals: {ga:?}"
    );
    assert!(
        !ga.contains(&"goal-in-beta".to_string()),
        "alpha snapshot leaked beta's goal: {ga:?}"
    );

    let (sb, snap_b) = serve.get_json(&format!("/v1/snapshot?space=space-beta&project={id_b}"));
    assert_eq!(sb, 200);
    let gb = goal_ids(&snap_b);
    assert!(
        gb.contains(&"goal-in-beta".to_string()),
        "beta goals: {gb:?}"
    );
    assert!(
        !gb.contains(&"goal-in-alpha".to_string()),
        "beta snapshot leaked alpha's goal: {gb:?}"
    );

    let (_status, same_space_other_binding) =
        serve.get_json(&format!("/v1/snapshot?space=space-alpha&project={id_b}"));
    let ids = goal_ids(&same_space_other_binding);
    assert!(ids.contains(&"goal-in-alpha".to_string()));
    assert!(!ids.contains(&"goal-in-beta".to_string()));
}

#[test]
fn snapshot_without_space_uses_active_execution_space() {
    let home = TempHome::new("api-default");
    let (_a, id_a) = init_project(&home, "alpha");
    let (_b, id_b) = init_project(&home, "beta"); // beta is active
    create_space(&home, "space-alpha", &id_a);
    create_space(&home, "space-beta", &id_b);
    create_goal(&home, "space-alpha", &id_a, "goal-in-alpha", "Alpha goal");
    create_goal(&home, "space-beta", &id_b, "goal-in-beta", "Beta goal");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    // No ?space → the active Execution Space (space-beta).
    let (status, snap) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    let g = goal_ids(&snap);
    assert!(
        g.contains(&"goal-in-beta".to_string()),
        "default snapshot should use active space-beta: {g:?}"
    );
    assert!(
        !g.contains(&"goal-in-alpha".to_string()),
        "default snapshot leaked alpha: {g:?}"
    );
}

#[test]
fn post_switch_updates_registry_and_marker() {
    let home = TempHome::new("api-switch");
    let (_a, id_a) = init_project(&home, "alpha");
    let (_b, id_b) = init_project(&home, "beta"); // beta active initially
    assert_eq!(current_project_id(&home), id_b);
    let space_before = current_space_id(&home);

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, body) =
        serve.post_json("/v1/projects/switch", &serde_json::json!({"project": id_a}));
    assert_eq!(status, 200, "switch body: {body}");
    assert_eq!(body["ok"], serde_json::json!(true), "body: {body}");
    assert_eq!(body["result"]["current"].as_str(), Some(id_a.as_str()));

    // Registry + ACTIVE_PROJECT marker now point at alpha.
    assert_eq!(current_project_id(&home), id_a, "registry not switched");
    let marker = std::fs::read_to_string(home.active_marker_path()).expect("marker");
    assert_eq!(marker.trim(), id_a, "ACTIVE_PROJECT marker not switched");

    // GET current reflects the switch live (no serve restart).
    let (_s, cur) = serve.get_json("/v1/projects/current");
    assert_eq!(
        cur["current"].as_str(),
        Some(id_a.as_str()),
        "live current: {cur}"
    );

    // Project switching must not move coordination storage.
    let other = home.base().join("somewhere").join("else");
    std::fs::create_dir_all(&other).unwrap();
    let (_src, src_stderr) = store_source(&home, &other);
    assert!(src_stderr.contains("SpaceCurrent"), "{src_stderr}");
    assert!(src_stderr.contains(&space_before), "{src_stderr}");
    assert_eq!(current_space_id(&home), space_before);
}

/// Run `harness --store-source mission list` and return (stdout, stderr).
fn store_source(home: &TempHome, cwd: &Path) -> (String, String) {
    let out = run_firm(home, cwd, &["--store-source", "mission", "list"]);
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}
