//! Integration coverage for the flat durable AgentTeam model: one active Host
//! TeamMembership and one immutable ExecutionNode placement per Team. Mission
//! linkage is optional legacy provenance, never Team creation or identity
//! authority; cross-Team work uses WorkDelegation, never parent/child topology.

mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, run_firm, run_firm_with_env, TempHome,
};

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
    let created =
        create_canonical_agent_member(home, home.base(), project_id, id, id, role, "kimi", &[]);
    assert!(
        created.status.success(),
        "canonical member create failed: {created:?}"
    );
}

#[test]
fn team_create_persists_and_enforces_flat_identity_and_placement() {
    let home = TempHome::new("team-topology");
    let project_id = init_project(&home, "topology");
    for (id, role) in [
        ("agent-lead", "lead"),
        ("agent-cto", "coordinator"),
        ("agent-worker", "builder"),
    ] {
        create_member(&home, &project_id, id, role);
    }

    for (id, title) in [
        ("mission-root", "Root Mission"),
        ("mission-peer", "Peer Mission"),
        ("mission-node-check", "Node Check Mission"),
        ("mission-host-check", "Host Check Mission"),
    ] {
        // DOC-108 retired the Mission writers: legacy Mission provenance is
        // seeded directly as pre-cutover history.
        firm_env::seed_historical_mission(&home, &project_id, id, title);
    }
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();

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
            "Flat peer in the Organization",
            "--mission-id",
            "mission-root",
            "--host-agent-id",
            "agent-lead",
            "--node-id",
            &node_id,
            "--member",
            "agent-lead",
            "--member",
            "agent-cto",
        ],
    );
    assert_eq!(root["legacy_mission_id"], "mission-root");
    assert!(root.get("mission_id").is_none());
    assert!(root.get("host_agent_id").is_none());
    assert!(root.get("member_ids").is_none());
    assert_eq!(root["node_id"], node_id);
    assert!(root.get("parent_team_id").is_none());
    assert!(root.get("host_member_id").is_none());

    let peer = run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-peer",
            "--name",
            "Peer Team",
            "--description",
            "Receives explicit delegated Work",
            "--mission-id",
            "mission-peer",
            "--host-agent-id",
            "agent-cto",
            "--node-id",
            &node_id,
            "--member",
            "agent-worker",
        ],
    );
    assert_eq!(peer["legacy_mission_id"], "mission-peer");
    assert!(peer.get("mission_id").is_none());
    assert!(peer.get("host_agent_id").is_none());
    assert!(peer.get("parent_team_id").is_none());

    // Legacy Mission provenance does not own Team identity and may be shared.
    let shared_provenance = run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-duplicate-mission",
            "--name",
            "Duplicate Mission",
            "--description",
            "Must fail",
            "--mission-id",
            "mission-root",
            "--host-agent-id",
            "agent-worker",
            "--node-id",
            &node_id,
        ],
    );
    assert_eq!(shared_provenance["legacy_mission_id"], "mission-root");

    // A vNext Team does not require Mission creation or linkage.
    let no_mission = run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-missing-mission",
            "--name",
            "Missing Mission",
            "--description",
            "Independent durable Team",
            "--host-agent-id",
            "agent-worker",
            "--node-id",
            &node_id,
        ],
    );
    assert!(no_mission.get("legacy_mission_id").is_none());

    let err = run_err(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-missing-node",
            "--name",
            "Missing Node",
            "--description",
            "Must fail",
            "--mission-id",
            "mission-node-check",
            "--host-agent-id",
            "agent-worker",
            "--node-id",
            "00000000-0000-4000-8000-000000000099",
        ],
    );
    assert!(
        err.contains("immutable placement Node to exist"),
        "stderr: {err}"
    );

    let err = run_err(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-missing-host",
            "--name",
            "Missing Host",
            "--description",
            "Must fail",
            "--mission-id",
            "mission-host-check",
            "--host-agent-id",
            "agent-missing",
            "--node-id",
            &node_id,
        ],
    );
    assert!(
        err.contains("TeamMembership references a missing AgentMember"),
        "stderr: {err}"
    );

    // Membership lifecycle is durable and cannot remove the sole active Host.
    let left = run_json(
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
    assert_eq!(left["agent_member_id"], "agent-cto");
    assert_eq!(left["role"], "member");
    assert_eq!(left["state"], "inactive");
    let err = run_err(
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
    assert!(err.contains("sole active Host Membership"), "stderr: {err}");
}
