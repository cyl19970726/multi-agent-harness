//! Integration coverage for the recursive AgentTeam topology slice (ADR 0052):
//! `harness team create --parent-team/--host-member` persists the durable
//! parent/host relations, the store guard enforces the direct-host and
//! acyclic invariants on every creation path, and `team remove-member`
//! refuses to strand a child Team's host.

mod firm_env;

use firm_env::{current_project_id, run_firm, run_firm_with_env, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
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
        "harness {args:?} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn create_member(home: &TempHome, project_id: &str, id: &str, role: &str) {
    run_json(
        home,
        project_id,
        &[
            "agent",
            "create",
            "--id",
            id,
            "--name",
            id,
            "--role",
            role,
            "--provider",
            "kimi",
        ],
    );
}

#[test]
fn team_create_persists_and_enforces_recursive_topology() {
    let home = TempHome::new("team-topology");
    let project_id = init_project(&home, "topology");
    for (id, role) in [
        ("agent-lead", "lead"),
        ("agent-cto", "coordinator"),
        ("agent-worker", "builder"),
    ] {
        create_member(&home, &project_id, id, role);
    }

    // Root team with a durable Lead host; flat member list stays compatible.
    let root = run_json(
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
            "Root of the recursive Organization",
            "--lead",
            "host",
            "--member",
            "agent-lead",
            "--member",
            "agent-cto",
            "--host-member",
            "agent-lead",
        ],
    );
    assert_eq!(root["parent_team_id"], serde_json::Value::Null);
    assert_eq!(root["host_member_id"], "agent-lead");

    // Child team: the host is a direct member of the parent.
    let child = run_json(
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
            "Executes delegated Work",
            "--lead",
            "agent-cto",
            "--member",
            "agent-worker",
            "--parent-team",
            "team-root",
            "--host-member",
            "agent-cto",
        ],
    );
    assert_eq!(child["parent_team_id"], "team-root");
    assert_eq!(child["host_member_id"], "agent-cto");

    let shown = run_json(&home, &project_id, &["team", "show", "--id", "team-child"]);
    assert_eq!(shown["parent_team_id"], "team-root");
    assert_eq!(shown["host_member_id"], "agent-cto");

    // Unknown parent is rejected.
    let err = run_err(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-orphan",
            "--name",
            "Orphan",
            "--description",
            "Missing parent",
            "--lead",
            "host",
            "--parent-team",
            "team-missing",
            "--host-member",
            "agent-cto",
        ],
    );
    assert!(err.contains("missing parent AgentTeam"), "stderr: {err}");

    // Direct-host invariant: the host must be a direct member of the parent.
    let err = run_err(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-stranger",
            "--name",
            "Stranger Hosted",
            "--description",
            "Host outside parent membership",
            "--lead",
            "host",
            "--parent-team",
            "team-root",
            "--host-member",
            "agent-worker",
        ],
    );
    assert!(err.contains("not a direct member"), "stderr: {err}");

    // One member hosts at most one team in V1.
    let err = run_err(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-second",
            "--name",
            "Second Hosted Team",
            "--description",
            "Duplicate host claim",
            "--lead",
            "host",
            "--parent-team",
            "team-root",
            "--host-member",
            "agent-cto",
        ],
    );
    assert!(err.contains("more than one AgentTeam"), "stderr: {err}");

    // A non-root team must name its durable host.
    let err = run_err(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-hostless",
            "--name",
            "Hostless Child",
            "--description",
            "Non-root without host",
            "--lead",
            "host",
            "--parent-team",
            "team-root",
        ],
    );
    assert!(err.contains("host_member_id"), "stderr: {err}");

    // Removing the child's host from the parent would strand the child team.
    let err = run_err(
        &home,
        &project_id,
        &[
            "team",
            "remove-member",
            "--id",
            "team-root",
            "--member",
            "agent-cto",
        ],
    );
    assert!(err.contains("not a direct member"), "stderr: {err}");

    // Removing an unrelated member stays legal.
    let updated = run_json(
        &home,
        &project_id,
        &[
            "team",
            "remove-member",
            "--id",
            "team-root",
            "--member",
            "agent-lead",
        ],
    );
    assert_eq!(updated["member_ids"], serde_json::json!(["agent-cto"]));
}
