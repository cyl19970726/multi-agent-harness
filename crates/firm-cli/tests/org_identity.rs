//! ADR 0052 durable AgentMember identity and explicit root Lead bootstrap.

mod firm_env;

use firm_env::{current_project_id, run_firm, run_firm_with_env, TempHome};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
    current_project_id(home)
}

fn run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm_with_env(home, home.base(), &full, &[]);
    assert!(
        out.status.success(),
        "harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|error| panic!("harness {args:?} stdout was not JSON ({error})"))
}

fn run_err(home: &TempHome, project_id: &str, args: &[&str]) -> String {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm_with_env(home, home.base(), &full, &[]);
    assert!(
        !out.status.success(),
        "harness {args:?} unexpectedly passed"
    );
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn root_lead_bootstrap_separates_durable_identity_from_runtime_and_cuts_over_authority() {
    let home = TempHome::new("org-identity-bootstrap");
    let project_id = init_project(&home, "identity");

    run_json(
        &home,
        &project_id,
        &[
            "agent",
            "create",
            "--id",
            "agent-lead",
            "--name",
            "Lead",
            "--role",
            "lead",
            "--provider",
            "kimi",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-root",
            "--title",
            "Root Mission",
            "--objective",
            "Exercise durable Host identity",
            "--json",
        ],
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();
    run_json(
        &home,
        &project_id,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--project-binding-id",
            &project_id,
        ],
    );

    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-root",
            "--name",
            "Root Team",
            "--description",
            "Compatibility root before durable Lead bootstrap",
            "--mission-id",
            "mission-root",
            "--host-agent-id",
            "agent-lead",
            "--node-id",
            &node_id,
            "--member",
            "agent-lead",
        ],
    );
    let before = run_err(&home, &project_id, &["org", "cutover-audit"]);
    assert!(before.contains("missing Host Agent"), "stderr: {before}");

    let bootstrapped = run_json(
        &home,
        &project_id,
        &[
            "org",
            "bootstrap-lead",
            "--team",
            "team-root",
            "--id",
            "agent-lead",
            "--name",
            "Lead",
            "--description",
            "Durable root Lead",
            "--role",
            "lead",
            "--provider-profile",
            "kimi/qwen3.8-max",
            "--model",
            "qwen/qwen3.8-max",
            "--project-binding",
            &project_id,
            "--business-access-ceiling",
            "company_os.read",
        ],
    );
    assert_eq!(bootstrapped["member"]["status"], "active");
    assert_eq!(
        bootstrapped["member"]["native_session"],
        serde_json::Value::Null
    );
    assert_eq!(bootstrapped["team"]["host_agent_id"], "agent-lead");

    let host = run_json(&home, &project_id, &["org", "host", "--team", "team-root"]);
    assert_eq!(host["host_agent_id"], "agent-lead");
    assert_eq!(host["source"], "agent_team");

    let audit = run_json(&home, &project_id, &["org", "cutover-audit"]);
    assert_eq!(audit["ready"], true);
    assert_eq!(audit["authority"], "host_agent_id");
    assert_eq!(audit["team_count"], 1);
    assert_eq!(audit["durable_member_count"], 1);

    // Durable identities can Host a peer flat Team without inventing a
    // ProviderRuntimeProjection, Session, or parent/child topology.
    run_json(
        &home,
        &project_id,
        &[
            "org",
            "member",
            "create",
            "--id",
            "agent-cto",
            "--name",
            "CTO",
            "--description",
            "Durable child Host",
            "--role",
            "cto",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-child",
            "--title",
            "Peer Mission",
            "--objective",
            "Prove flat peer Team identity",
            "--json",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-child",
            "--name",
            "Child Team",
            "--description",
            "Hosted by the durable CTO identity as a flat peer",
            "--mission-id",
            "mission-child",
            "--host-agent-id",
            "agent-cto",
            "--node-id",
            &node_id,
            "--member",
            "agent-cto",
        ],
    );
    let recursive_audit = run_json(&home, &project_id, &["org", "cutover-audit"]);
    assert_eq!(recursive_audit["team_count"], 2);
    assert_eq!(recursive_audit["durable_member_count"], 2);
}

#[test]
fn compatibility_registry_convergence_is_explicit_and_idempotent() {
    let home = TempHome::new("org-identity-converge");
    let project_id = init_project(&home, "converge");
    run_json(
        &home,
        &project_id,
        &[
            "agent",
            "create",
            "--id",
            "agent-cto",
            "--name",
            "CTO",
            "--description",
            "Compatibility runtime registry row",
            "--role",
            "cto",
            "--provider",
            "kimi",
            "--model",
            "qwen/qwen3.8-max",
        ],
    );
    let first = run_json(
        &home,
        &project_id,
        &[
            "org",
            "member",
            "converge",
            "--id",
            "agent-cto",
            "--project-binding",
            &project_id,
        ],
    );
    let second = run_json(
        &home,
        &project_id,
        &[
            "org",
            "member",
            "converge",
            "--id",
            "agent-cto",
            "--project-binding",
            &project_id,
        ],
    );
    assert_eq!(first, second);
    assert_eq!(first["provider_profile"], "kimi");
    assert!(first.get("provider_runtime_id").is_none());
    assert!(first.get("native_session").is_none());

    let members = run_json(&home, &project_id, &["org", "member", "list"]);
    assert_eq!(members.as_array().unwrap().len(), 1);
}

#[test]
fn member_show_returns_durable_identity_fields() {
    let home = TempHome::new("org-identity-show");
    let project_id = init_project(&home, "show");

    let created = run_json(
        &home,
        &project_id,
        &[
            "org",
            "member",
            "create",
            "--id",
            "agent-alpha",
            "--name",
            "Alpha",
            "--description",
            "Durable member for show test",
            "--role",
            "developer",
            "--provider-profile",
            "kimi/qwen3.8-max",
            "--model",
            "qwen/qwen3.8-max",
            "--project-binding",
            &project_id,
        ],
    );
    assert_eq!(created["id"], "agent-alpha");
    assert_eq!(created["status"], "active");

    let shown = run_json(
        &home,
        &project_id,
        &["org", "member", "show", "--id", "agent-alpha"],
    );
    assert_eq!(shown["id"], "agent-alpha");
    assert_eq!(shown["name"], "Alpha");
    assert_eq!(shown["role"], "developer");
    assert_eq!(shown["status"], "active");
    assert_eq!(shown["provider_profile"], "kimi/qwen3.8-max");
    assert!(shown.get("native_session").is_none());

    // show a non-existent member should error
    let err = run_err(
        &home,
        &project_id,
        &["org", "member", "show", "--id", "nonexistent"],
    );
    assert!(err.contains("not found"), "stderr: {err}");

    // list should include the member
    let list = run_json(&home, &project_id, &["org", "member", "list"]);
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[test]
fn converge_missing_member_reports_error() {
    let home = TempHome::new("org-identity-converge-err");
    let project_id = init_project(&home, "converge-err");

    let err = run_err(
        &home,
        &project_id,
        &["org", "member", "converge", "--id", "agent-missing"],
    );
    assert!(
        err.contains("not found"),
        "expected 'not found' in stderr, got: {err}"
    );
}

#[test]
fn bootstrap_lead_rejects_duplicate_team_id() {
    let home = TempHome::new("org-identity-bootstrap-dup");
    let project_id = init_project(&home, "bootstrap-dup");

    run_json(
        &home,
        &project_id,
        &[
            "agent",
            "create",
            "--id",
            "agent-lead",
            "--name",
            "Lead",
            "--role",
            "lead",
            "--provider",
            "codex",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-root",
            "--title",
            "Root Mission",
            "--objective",
            "Test Host bootstrap",
            "--json",
        ],
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();

    // Bootstrap-lead expects the team to already exist (created separately).
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-root",
            "--name",
            "Root Team",
            "--description",
            "Pre-created root team",
            "--mission-id",
            "mission-root",
            "--host-agent-id",
            "agent-lead",
            "--node-id",
            &node_id,
            "--member",
            "agent-lead",
        ],
    );

    // First bootstrap succeeds.
    run_json(
        &home,
        &project_id,
        &[
            "org",
            "bootstrap-lead",
            "--team",
            "team-root",
            "--id",
            "agent-lead",
            "--name",
            "Lead",
            "--description",
            "Root Lead",
            "--role",
            "lead",
        ],
    );

    // Second bootstrap on the same team must fail.
    let err = run_err(
        &home,
        &project_id,
        &[
            "org",
            "bootstrap-lead",
            "--team",
            "team-root",
            "--id",
            "agent-lead-2",
            "--name",
            "Lead 2",
            "--description",
            "Duplicate attempt",
            "--role",
            "lead",
        ],
    );
    assert!(
        err.contains("conflicting")
            || err.contains("already exists")
            || err.contains("duplicate")
            || err.contains("Host is"),
        "expected duplicate/conflict error, got: {err}"
    );
}
