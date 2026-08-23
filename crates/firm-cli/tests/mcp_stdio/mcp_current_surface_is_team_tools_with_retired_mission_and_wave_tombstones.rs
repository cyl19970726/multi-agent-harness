use super::*;

#[test]
fn mcp_current_surface_is_team_tools_with_retired_mission_and_wave_tombstones() {
    let home = TempHome::new("mcp-mission-only-surface");
    let project_id = init_project(&home, "mcp-mission-only-project");
    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let _ = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "mission-only-surface-test", "version": "0"}
        }),
    );

    let listed = mcp.request("tools/list", serde_json::json!({}));
    let tools = listed["result"]["tools"].as_array().expect("tools array");
    let names = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for current in ["mission_list", "team_run_create"] {
        assert!(
            names.contains(current),
            "missing current MCP tool {current}"
        );
    }
    // DOC-108: Mission writers are removed from the MCP surface entirely —
    // the same unknown-tool tombstone as retired Wave tools.
    for retired in ["mission_create", "mission_update_context", "mission_close"] {
        assert!(
            !names.contains(retired),
            "retired Mission writer must not be advertised: {retired}"
        );
    }
    assert!(
        names.iter().all(|name| !name.starts_with("wave_")),
        "Legacy Wave tools must not be advertised: {names:?}"
    );
    for removed in [
        "team_run_send_message",
        "team_message_acknowledge",
        "team_run_reconcile_delivery",
        "team_run_work_reconcile_delivery",
    ] {
        assert!(
            !names.contains(removed),
            "retired TeamMessageProjection tombstone must not be advertised: {removed}"
        );
    }

    let team_run_create = tools
        .iter()
        .find(|tool| tool["name"] == "team_run_create")
        .expect("team_run_create definition");
    let schema = &team_run_create["inputSchema"];
    assert!(schema["properties"].get("mission_id").is_none());
    assert!(schema["properties"].get("wave_id").is_none());
    assert!(schema["required"]
        .as_array()
        .expect("required array")
        .iter()
        .any(|field| field == "agent_team_id"));

    let before = directory_snapshot(&home.spaces_dir());
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "wave_create",
            "arguments": {
                "id": "must-not-exist",
                "mission_id": "must-not-exist",
                "title": "must not write",
                "objective": "must not write"
            }
        }),
    );
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown tool")),
        "removed Legacy Wave tool must fail as unknown: {response}"
    );
    assert_eq!(
        directory_snapshot(&home.spaces_dir()),
        before,
        "removed Legacy Wave MCP tool must have a byte-zero store delta"
    );
    for removed in [
        "team_run_send_message",
        "team_message_acknowledge",
        "team_run_reconcile_delivery",
        "team_run_work_reconcile_delivery",
        "mission_create",
        "mission_update_context",
        "mission_close",
    ] {
        let response = mcp.request(
            "tools/call",
            serde_json::json!({
                "name": removed,
                "arguments": {
                    "team_run_id": "must-not-exist",
                    "message_id": "must-not-exist",
                    "member_id": "must-not-exist"
                }
            }),
        );
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("unknown tool")),
            "removed MCP tombstone must fail as unknown: {removed}: {response}"
        );
        assert_eq!(
            directory_snapshot(&home.spaces_dir()),
            before,
            "removed MCP tombstone must have a byte-zero store delta: {removed}"
        );
    }
}
