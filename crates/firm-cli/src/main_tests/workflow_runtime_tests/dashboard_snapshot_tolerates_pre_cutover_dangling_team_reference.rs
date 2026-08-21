use super::*;

/// DOC-108 pre-cutover doctrine: a TeamRun whose `agent_team_id` is absent
/// from the canonical AgentTeam projection but present in the retired
/// legacy `teams.jsonl` ledger is a migration fact — the snapshot renders
/// it as read-only legacy context with an integrity annotation instead of
/// failing closed.
#[test]
fn dashboard_snapshot_tolerates_pre_cutover_dangling_team_reference() {
    let store = temp_store("snapshot-pre-cutover-team");
    append_jsonl_value(
        &store.root().join("node_project_registrations.jsonl"),
        &serde_json::json!({
            "node_id": "node-pre-cutover",
            "execution_space_id": "space-pre-cutover",
            "project_binding_id": "project-pre-cutover",
            "status": "active",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }),
    )
    .expect("append registration");
    append_jsonl_value(
        &store.root().join("team_runs.jsonl"),
        &serde_json::json!({
            "id": "team-run-pre-cutover",
            "agent_team_id": "team-pre-cutover",
            "execution_node_id": "node-pre-cutover",
            "project_binding_id": "project-pre-cutover",
            "host_surface": "test",
            "objective": "pre-cutover run",
            "status": "completed",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }),
    )
    .expect("append dangling TeamRun");
    append_jsonl_value(
        &store.root().join("teams.jsonl"),
        &serde_json::json!({
            "id": "team-pre-cutover",
            "name": "Pre-cutover Team",
            "description": "retired legacy Team row",
            "mission_id": "mission-pre-cutover",
            "host_agent_id": "agent-pre-cutover-host",
            "node_id": "node-pre-cutover",
            "status": "active",
            "member_ids": ["agent-pre-cutover-worker"],
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }),
    )
    .expect("append legacy Team row");

    let snapshot = dashboard_snapshot(&store)
        .expect("pre-cutover dangling Team reference renders as legacy context");
    let teams = snapshot["teams"].as_array().expect("teams array");
    let legacy = teams
        .iter()
        .find(|team| team["id"].as_str() == Some("team-pre-cutover"))
        .expect("legacy Team rendered as context");
    assert_eq!(legacy["legacy_context"].as_bool(), Some(true));
    assert_eq!(legacy["read_only"].as_bool(), Some(true));
    assert!(
        legacy["integrity_annotation"]
            .as_str()
            .is_some_and(|note| note.starts_with("PRE_CUTOVER_DANGLING_AGENT_TEAM_REF")),
        "legacy context must carry the integrity annotation"
    );
    let annotations = snapshot["integrity_annotations"]
        .as_array()
        .expect("integrity_annotations array");
    assert!(
        annotations.iter().any(|annotation| {
            annotation["kind"].as_str() == Some("pre_cutover_dangling_agent_team_ref")
                && annotation["team_run_id"].as_str() == Some("team-run-pre-cutover")
                && annotation["agent_team_id"].as_str() == Some("team-pre-cutover")
        }),
        "snapshot must name the tolerated dangling reference"
    );
}
