//! Integration coverage for `GET /v1/meta` (issue #307 — dashboard provenance):
//!   - the shape is exactly `{ git_rev, built_at, store_root, latest_op_seq,
//!     server_version }`, so a stale frontend build can always cross-check
//!     what it's actually talking to;
//!   - `git_rev`/`built_at` are compile-time (never request-time) values;
//!   - `store_root` names the coordination store this exact response read;
//!   - `latest_op_seq` is a monotonic cursor over the store's WorkOperation
//!     log: it starts at zero and advances as Works are created.

mod harness_env;
use harness_env::{current_project_id, run_harness, ServeHandle, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its derived id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

#[test]
fn meta_shape_and_provenance_fields_on_an_empty_store() {
    let home = TempHome::new("meta-empty");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (status, meta) = serve.get_json("/v1/meta");
    assert_eq!(status, 200, "body: {meta}");

    // git_rev: compile-time, embedded by build.rs — either a short hex commit
    // or the graceful "unknown" fallback; never empty, never fetched live.
    let git_rev = meta["git_rev"].as_str().expect("git_rev is a string");
    assert!(!git_rev.is_empty(), "git_rev must not be empty: {meta}");
    assert!(
        git_rev == "unknown" || git_rev.chars().all(|c| c.is_ascii_hexdigit()),
        "git_rev should be a hex short-sha or \"unknown\": {git_rev}"
    );

    // built_at: either null, or the same "unix-ms:<millis>" convention every
    // other harness timestamp uses.
    match &meta["built_at"] {
        serde_json::Value::Null => {}
        serde_json::Value::String(value) => {
            let millis = value.strip_prefix("unix-ms:").unwrap_or_else(|| {
                panic!("built_at must be \"unix-ms:<millis>\" or null: {value}")
            });
            millis
                .parse::<u128>()
                .unwrap_or_else(|_| panic!("built_at millis must parse: {value}"));
        }
        other => panic!("built_at must be a string or null: {other}"),
    }

    // store_root: an absolute path to the coordination store this response
    // actually read from — the exact thing issue #307 says a panel must be
    // able to prove.
    let store_root = meta["store_root"].as_str().expect("store_root is a string");
    assert!(
        std::path::Path::new(store_root).is_absolute(),
        "store_root must be absolute: {store_root}"
    );

    // A freshly-init'd project has appended no WorkOperations yet.
    assert_eq!(meta["latest_op_seq"].as_u64(), Some(0), "body: {meta}");

    // server_version: this exact crate's Cargo.toml version, not a made-up
    // string — proves the two are wired to the same source of truth.
    assert_eq!(
        meta["server_version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "body: {meta}"
    );
}

#[test]
fn latest_op_seq_advances_as_work_operations_are_appended() {
    let home = TempHome::new("meta-op-seq");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (_status, before) = serve.get_json("/v1/meta");
    assert_eq!(before["latest_op_seq"].as_u64(), Some(0));

    // One member with `initial_work` appends exactly one WorkOperation.
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Prove op_seq advances",
            "members": [
                {"name": "lead", "role": "integrator", "provider": "codex",
                 "initial_work": "Ship the provenance surface"},
            ],
        }),
    );
    assert_eq!(status, 200, "body: {created}");

    let (_status, after_one) = serve.get_json("/v1/meta");
    assert_eq!(
        after_one["latest_op_seq"].as_u64(),
        Some(1),
        "body: {after_one}"
    );

    let team_run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id");
    let (status, work) = serve.post_json(
        &format!("/v1/team-runs/{team_run_id}/works"),
        &serde_json::json!({
            "title": "Second Work",
            "completion_criteria_markdown": "Exercise a second WorkOperation",
        }),
    );
    assert_eq!(status, 200, "body: {work}");

    let (_status, after_two) = serve.get_json("/v1/meta");
    assert_eq!(
        after_two["latest_op_seq"].as_u64(),
        Some(2),
        "body: {after_two}"
    );

    // A read-only GET must never itself advance the cursor.
    let (_status, unchanged) = serve.get_json("/v1/meta");
    assert_eq!(unchanged["latest_op_seq"].as_u64(), Some(2));
}

#[test]
fn meta_reads_the_space_selected_store_not_a_sibling_space() {
    // Mirrors serve_projects_api.rs's snapshot isolation coverage: two
    // Execution Spaces must never leak into each other's /v1/meta response.
    let home = TempHome::new("meta-space-scoped");
    let id_a = init_project(&home, "alpha");
    let id_b = init_project(&home, "beta");
    for (space_id, project_id) in [("space-alpha", &id_a), ("space-beta", &id_b)] {
        let out = run_harness(
            &home,
            home.base(),
            &[
                "space",
                "init",
                "--id",
                space_id,
                "--name",
                space_id,
                "--project-binding",
                project_id,
            ],
        );
        assert!(
            out.status.success(),
            "space init {space_id} failed: {out:?}"
        );
    }

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, created) = serve.post_json(
        &format!("/v1/team-runs?space=space-alpha&project={id_a}"),
        &serde_json::json!({
            "objective": "Scoped to space-alpha",
            "members": [
                {"name": "lead", "role": "integrator", "provider": "codex",
                 "initial_work": "Work only in space-alpha"},
            ],
        }),
    );
    assert_eq!(status, 200, "body: {created}");

    let (_status, alpha_meta) = serve.get_json("/v1/meta?space=space-alpha");
    assert_eq!(
        alpha_meta["latest_op_seq"].as_u64(),
        Some(1),
        "body: {alpha_meta}"
    );

    // space-beta never received that Work — its own /v1/meta must not see it.
    let (_status, beta_meta) = serve.get_json("/v1/meta?space=space-beta");
    assert_eq!(
        beta_meta["latest_op_seq"].as_u64(),
        Some(0),
        "body: {beta_meta}"
    );
    assert_ne!(
        alpha_meta["store_root"], beta_meta["store_root"],
        "space selector must change which store_root /v1/meta reports"
    );
}
