//! Integration coverage for `GET /v1/meta` (issue #307 — dashboard provenance):
//!   - the shape is exactly `{ git_rev, built_at, store_root, latest_op_seq,
//!     server_version }`, so a stale frontend build can always cross-check
//!     what it's actually talking to;
//!   - `git_rev`/`built_at` are compile-time (never request-time) values;
//!   - `store_root` names the coordination store this exact response read;
//!   - `latest_op_seq` is a monotonic cursor over the store's WorkOperation
//!     log: it starts at zero and advances as Works are created.

mod firm_env;
use firm_env::{
    clear_inherited_native_firm_env, create_canonical_agent_member, current_project_id, run_firm,
    ServeHandle, TempHome,
};

#[test]
fn build_info_is_storeless_and_reports_exact_or_unknown_revision() {
    let home = TempHome::new("build-info-storeless");
    let poisoned_home = home.base().join("not-a-directory");
    std::fs::write(&poisoned_home, "must remain a file").expect("write poisoned FIRM_HOME");

    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .arg("--build-info")
        .current_dir(home.base())
        .env("HOME", home.home())
        .env("FIRM_HOME", &poisoned_home);
    clear_inherited_native_firm_env(&mut command);
    command.env("FIRM_HOME", &poisoned_home);
    let output = command.output().expect("run firm --build-info");
    assert!(
        output.status.success(),
        "build-info must not resolve the poisoned store: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&poisoned_home).expect("poisoned path remains readable"),
        "must remain a file"
    );

    let info: serde_json::Value = serde_json::from_slice(&output.stdout).expect("build-info JSON");
    let git_rev = info["git_rev"].as_str().expect("git_rev string");
    assert!(
        git_rev == "unknown"
            || (git_rev.len() == 40 && git_rev.bytes().all(|byte| byte.is_ascii_hexdigit())),
        "git_rev must be a full 40-hex SHA or unknown: {git_rev}"
    );
    assert_eq!(
        info["package_version"].as_str(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}

fn seed_team(
    home: &TempHome,
    root: &std::path::Path,
    project_id: &str,
    space_id: Option<&str>,
    suffix: &str,
) -> String {
    let run = |args: &[&str]| {
        let mut selected = Vec::new();
        if let Some(space_id) = space_id {
            selected.extend(["--space", space_id, "--project", project_id]);
        }
        selected.extend_from_slice(args);
        let output = run_firm(home, root, &selected);
        assert!(
            output.status.success(),
            "fixture command {args:?} failed: {output:?}"
        );
        output
    };
    let node = run(&["node", "init"]);
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    let mut registration = vec![
        "node",
        "project",
        "register",
        "--node-id",
        node_id,
        "--project-binding-id",
        project_id,
    ];
    if let Some(space_id) = space_id {
        registration.extend(["--execution-space-id", space_id]);
    }
    run(&registration);
    // DOC-108 retired the Mission writers; legacy Mission provenance is
    // seeded directly as pre-cutover history, into the same Execution Space
    // the Team will live in (the default space when none is selected).
    let mission_id = format!("mission-meta-{suffix}");
    firm_env::seed_historical_mission(
        home,
        space_id.unwrap_or(project_id),
        &mission_id,
        &format!("Meta mission {suffix}"),
    );
    let host_id = format!("agent-meta-host-{suffix}");
    let host = create_canonical_agent_member(
        home,
        root,
        project_id,
        &host_id,
        &format!("meta-host-{suffix}"),
        "host",
        "codex",
        &space_id
            .map(|id| ("FIRM_SPACE", id))
            .into_iter()
            .collect::<Vec<_>>(),
    );
    assert!(host.status.success(), "canonical host failed: {host:?}");
    let team = run(&[
        "team",
        "create",
        "--name",
        &format!("Meta team {suffix}"),
        "--description",
        "Flat dashboard metadata test team",
        "--mission-id",
        &mission_id,
        "--host-agent-id",
        &host_id,
        "--node-id",
        node_id,
        "--member",
        &host_id,
    ]);
    let team: serde_json::Value = serde_json::from_slice(&team.stdout).expect("team JSON");
    team["id"].as_str().expect("team id").to_string()
}

/// `harness init` a project and seed the TeamRun admission relation.
fn init_project(home: &TempHome, name: &str) -> (String, String) {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);
    let team_id = seed_team(home, &root, &project_id, None, name);
    (project_id, team_id)
}

#[test]
fn meta_shape_and_provenance_fields_on_an_empty_store() {
    let home = TempHome::new("meta-empty");
    let (_project_id, _team_id) = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (status, meta) = serve.get_json("/v1/meta");
    assert_eq!(status, 200, "body: {meta}");

    // git_rev: compile-time, embedded by build.rs — either one exact 40-hex
    // commit or the graceful "unknown" fallback; never fetched live.
    let git_rev = meta["git_rev"].as_str().expect("git_rev is a string");
    assert!(!git_rev.is_empty(), "git_rev must not be empty: {meta}");
    assert!(
        git_rev == "unknown"
            || (git_rev.len() == 40 && git_rev.chars().all(|c| c.is_ascii_hexdigit())),
        "git_rev should be a full 40-hex SHA or \"unknown\": {git_rev}"
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
    let (project_id, team_id) = init_project(&home, "alpha");
    let credentials = serde_json::json!([{
        "token":"meta-host-token",
        "actor":{"kind":"agent_member","id":"agent-meta-host-alpha"},
        "authority_actors":[]
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );

    let (_status, before) = serve.get_json("/v1/meta");
    assert_eq!(before["latest_op_seq"].as_u64(), Some(0));

    // One member with `initial_work` appends the responsibility-neutral Create
    // followed by the canonical stable-membership assignment.
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "agent_team_id": team_id,
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
        Some(2),
        "body: {after_one}"
    );

    let team_run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id");
    let (status, work) = serve.post_json_with_headers(
        &format!("/v1/agentfirm/team-runs/{team_run_id}/works?project={project_id}"),
        &serde_json::json!({
            "action": "create_work",
            "work_id": "work-meta-second",
            "title": "Second Work",
            "completion_criteria_markdown": "Exercise a second WorkOperation",
        }),
        &[
            ("X-AgentFirm-Token", "meta-host-token"),
            ("Idempotency-Key", "meta-second-work"),
            ("If-Match", "0"),
        ],
    );
    assert_eq!(status, 200, "body: {work}");

    let (_status, after_two) = serve.get_json("/v1/meta");
    assert_eq!(
        after_two["latest_op_seq"].as_u64(),
        Some(3),
        "body: {after_two}"
    );

    // A read-only GET must never itself advance the cursor.
    let (_status, unchanged) = serve.get_json("/v1/meta");
    assert_eq!(unchanged["latest_op_seq"].as_u64(), Some(3));
}

#[test]
fn meta_reads_the_space_selected_store_not_a_sibling_space() {
    // Mirrors serve_projects_api.rs's snapshot isolation coverage: two
    // Execution Spaces must never leak into each other's /v1/meta response.
    let home = TempHome::new("meta-space-scoped");
    let (id_a, _default_team_a) = init_project(&home, "alpha");
    let (id_b, _default_team_b) = init_project(&home, "beta");
    for (space_id, project_id) in [("space-alpha", &id_a), ("space-beta", &id_b)] {
        let out = run_firm(
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

    let team_a = seed_team(
        &home,
        &home.base().join("alpha"),
        &id_a,
        Some("space-alpha"),
        "space-alpha",
    );

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, created) = serve.post_json(
        &format!("/v1/team-runs?space=space-alpha&project={id_a}"),
        &serde_json::json!({
            "agent_team_id": team_a,
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
        Some(2),
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
