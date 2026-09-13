//! `team-run work migrate-responsibility` writes ordinary `Updated`
//! WorkOperations, so it needs the same authority every other Host Work verb
//! needs: the exact Host actor stored on the Work's TeamRun.
//!
//! Before this gate the CLI synthesized a Host actor from `--actor`, defaulting
//! to the literal `migration-host`, and the Store checked only that the actor
//! kind was `Host`. Any local caller could therefore rewrite every Work's
//! responsibility fields. These assertions pin the CLI half: the actor is
//! resolved from the store, and `--actor` is only an assertion about it.

mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, current_space_id, run_firm, TempHome,
};

const TEAM_ID: &str = "team-migrate-fixture";
const HOST_ID: &str = "agent-migrate-host";

fn migrate_fixture(tag: &str) -> (TempHome, String, String) {
    let home = TempHome::new(tag);
    let root = home.base().join("alpha");
    std::fs::create_dir_all(&root).expect("create project root");

    let out = run_firm(&home, &root, &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
    let project_id = current_project_id(&home);

    let node = run_firm(&home, &root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id").to_string();
    let registration = run_firm(
        &home,
        &root,
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
    assert!(
        registration.status.success(),
        "node project register failed: {registration:?}"
    );
    firm_env::seed_historical_mission(
        &home,
        &project_id,
        "mission-migrate-fixture",
        "Responsibility migration mission",
    );
    let host = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        HOST_ID,
        "migrate-host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "host member create failed: {host:?}");
    let team = run_firm(
        &home,
        &root,
        &[
            "team",
            "create",
            "--id",
            TEAM_ID,
            "--name",
            "Responsibility migration team",
            "--description",
            "Flat migration test team",
            "--mission-id",
            "mission-migrate-fixture",
            "--host-agent-id",
            HOST_ID,
            "--node-id",
            &node_id,
            "--member",
            HOST_ID,
        ],
    );
    assert!(team.status.success(), "team create failed: {team:?}");
    let created = run_firm(
        &home,
        &root,
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            TEAM_ID,
            "--objective",
            "Prove the migration is bound to the exact Host",
            "--host-runtime-mode",
            "external_interactive",
            "--member",
            &format!("{HOST_ID}:host:codex/external_interactive"),
        ],
    );
    assert!(
        created.status.success(),
        "team-run create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let run_id = String::from_utf8_lossy(&created.stdout).trim().to_string();
    let work = run_firm(
        &home,
        &root,
        &[
            "--project",
            &project_id,
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--title",
            "Migration fixture Work",
            "--completion-criteria",
            "the migration reports this Work",
        ],
    );
    assert!(
        work.status.success(),
        "team-run work create failed: {}",
        String::from_utf8_lossy(&work.stderr)
    );
    let space_id = current_space_id(&home);
    (home, project_id, space_id)
}

#[test]
fn migrate_responsibility_binds_to_the_stored_team_run_host() {
    let (home, project_id, space_id) = migrate_fixture("migrate-host-gate");

    let impostor = run_firm(
        &home,
        home.base(),
        &[
            "--space",
            &space_id,
            "--project",
            &project_id,
            "team-run",
            "work",
            "migrate-responsibility",
            "--actor",
            "migration-host",
        ],
    );
    assert!(
        !impostor.status.success(),
        "a caller-named actor must not authorize the migration: {}",
        String::from_utf8_lossy(&impostor.stdout)
    );
    let refusal = String::from_utf8_lossy(&impostor.stderr).to_string();
    assert!(
        refusal.contains("MIGRATION_HOST_MISMATCH"),
        "unexpected refusal: {refusal}"
    );

    let resolved = run_firm(
        &home,
        home.base(),
        &[
            "--space",
            &space_id,
            "--project",
            &project_id,
            "team-run",
            "work",
            "migrate-responsibility",
        ],
    );
    assert!(
        resolved.status.success(),
        "the stored TeamRun Host must be resolved without --actor: {}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&resolved.stdout).expect("migration report JSON");
    assert_eq!(report["execution_space_id"], serde_json::json!(space_id));

    let asserted = run_firm(
        &home,
        home.base(),
        &[
            "--space",
            &space_id,
            "--project",
            &project_id,
            "team-run",
            "work",
            "migrate-responsibility",
            "--actor",
            HOST_ID,
        ],
    );
    assert!(
        asserted.status.success(),
        "naming the exact Host is an allowed assertion: {}",
        String::from_utf8_lossy(&asserted.stderr)
    );
}
