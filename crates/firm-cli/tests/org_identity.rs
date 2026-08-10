//! Canonical AgentMember identity at the Organization projection boundary.
//!
//! Company/Organization may read AgentMember ActorRefs from the Execution
//! Space, but it must not recreate a second identity ledger or compatibility
//! convergence/bootstrap mutation path.

mod firm_env;

use firm_env::{create_canonical_agent_member, current_project_id, run_firm, TempHome};

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
    let out = run_firm(home, home.base(), &full);
    assert!(
        out.status.success(),
        "firm {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("command JSON")
}

fn run_err(home: &TempHome, project_id: &str, args: &[&str]) -> String {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm(home, home.base(), &full);
    assert!(!out.status.success(), "firm {args:?} unexpectedly passed");
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn canonical_agent_member_directly_drives_team_host_and_cutover_audit() {
    let home = TempHome::new("org-canonical-identity");
    let project_id = init_project(&home, "identity");
    let lead = create_canonical_agent_member(
        &home,
        home.base(),
        &project_id,
        "agent-lead",
        "Lead",
        "lead",
        "codex",
        &[],
    );
    assert!(lead.status.success(), "canonical Lead failed: {lead:?}");

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
            "Exercise canonical Host identity",
            "--json",
        ],
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id");
    run_json(
        &home,
        &project_id,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            node_id,
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
            "Flat Team with canonical Host",
            "--mission-id",
            "mission-root",
            "--host-agent-id",
            "agent-lead",
            "--node-id",
            node_id,
            "--member",
            "agent-lead",
        ],
    );

    let host = run_json(&home, &project_id, &["org", "host", "--team", "team-root"]);
    assert_eq!(host["host_agent_id"], "agent-lead");
    assert_eq!(host["source"], "agent_team");
    let audit = run_json(&home, &project_id, &["org", "cutover-audit"]);
    assert_eq!(audit["ready"], true);
    assert_eq!(audit["authority"], "host_agent_id");
    assert_eq!(audit["team_count"], 1);
    assert_eq!(audit["agent_member_count"], 1);

    let store = harness_store::HarnessStore::new(home.spaces_dir().join(&project_id));
    let members = store
        .trust_agent_members(&project_id)
        .expect("canonical identities");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].id, "agent-lead");
    for retired in ["provider_launch_profiles.jsonl"] {
        assert!(
            !store.root().join(retired).exists(),
            "retired identity ledger must stay absent: {retired}"
        );
    }
}

#[test]
fn legacy_organization_identity_mutations_are_stably_retired() {
    let home = TempHome::new("org-identity-retired");
    let project_id = init_project(&home, "retired");
    let cases: &[&[&str]] = &[
        &[
            "org",
            "member",
            "create",
            "--id",
            "agent-old",
            "--name",
            "Old",
            "--role",
            "worker",
        ],
        &["org", "member", "converge", "--id", "agent-old"],
        &[
            "org",
            "bootstrap-lead",
            "--team",
            "team-old",
            "--id",
            "agent-old",
            "--name",
            "Old",
            "--role",
            "lead",
        ],
    ];
    for args in cases {
        let error = run_err(&home, &project_id, args);
        assert!(
            error.contains("canonical member-trust mutate")
                && error.contains("legacy")
                && error.contains("removed"),
            "retirement error for {args:?}: {error}"
        );
    }
}
