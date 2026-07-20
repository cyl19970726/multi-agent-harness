//! Integration coverage for the multi-project serve HTTP API
//! (goal-multi-project P6, project-api task).
//!
//! Spawns the real `harness serve` against an isolated HOME with TWO registered
//! projects, then asserts:
//!   - `GET /v1/projects` lists both registry projects + the reserved `_global`,
//!   - `GET /v1/projects/current` reflects the registry's active project,
//!   - `GET /v1/snapshot?project=<id>` reads exactly that project's store,
//!   - `GET /v1/snapshot` (no param) reads the active/default project,
//!   - `POST /v1/projects/switch` flips the active project (registry + marker) so a
//!     later CLI command from a different cwd converges on the same store.

use std::path::Path;

mod harness_env;
use harness_env::{current_project_id, run_harness, ServeHandle, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its derived id.
fn init_project(home: &TempHome, name: &str) -> (std::path::PathBuf, String) {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let id = current_project_id(home);
    (root, id)
}

/// Create a Mission in a specific project's store via `--project <id>`.
fn create_goal(home: &TempHome, project_id: &str, goal_id: &str, title: &str) {
    let out = run_harness(
        home,
        home.base(),
        &[
            "--project",
            project_id,
            "mission",
            "create",
            "--id",
            goal_id,
            "--title",
            title,
            "--objective",
            "Exercise project isolation",
        ],
    );
    assert!(out.status.success(), "mission create failed: {out:?}");
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
fn snapshot_with_project_param_reads_that_store_only() {
    let home = TempHome::new("api-scoped");
    let (_a, id_a) = init_project(&home, "alpha");
    let (_b, id_b) = init_project(&home, "beta");
    create_goal(&home, &id_a, "goal-in-alpha", "Alpha goal");
    create_goal(&home, &id_b, "goal-in-beta", "Beta goal");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (sa, snap_a) = serve.get_json(&format!("/v1/snapshot?project={id_a}"));
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

    let (sb, snap_b) = serve.get_json(&format!("/v1/snapshot?project={id_b}"));
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
}

#[test]
fn snapshot_without_project_uses_active_default() {
    let home = TempHome::new("api-default");
    let (_a, id_a) = init_project(&home, "alpha");
    let (_b, id_b) = init_project(&home, "beta"); // beta is active
    create_goal(&home, &id_a, "goal-in-alpha", "Alpha goal");
    create_goal(&home, &id_b, "goal-in-beta", "Beta goal");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    // No ?project → the active project (beta).
    let (status, snap) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    let g = goal_ids(&snap);
    assert!(
        g.contains(&"goal-in-beta".to_string()),
        "default snapshot should be active (beta): {g:?}"
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

    // A CLI command from a DIFFERENT cwd now converges on the switched store.
    let other = home.base().join("somewhere").join("else");
    std::fs::create_dir_all(&other).unwrap();
    let (_src, src_stderr) = store_source(&home, &other);
    assert!(
        src_stderr.contains(&id_a),
        "CLI from other cwd did not converge on switched project {id_a}: {src_stderr}"
    );
}

/// Run `harness --store-source mission list` and return (stdout, stderr).
fn store_source(home: &TempHome, cwd: &Path) -> (String, String) {
    let out = run_harness(home, cwd, &["--store-source", "mission", "list"]);
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}
