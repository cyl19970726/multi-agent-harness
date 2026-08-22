use super::*;

#[test]
fn mcp_team_run_create_without_legacy_mission_omits_mission_context() {
    let home = TempHome::new("mcp-missionless-team-run");
    let project_id = init_project(&home, "mcp-missionless-project");
    let project_root = home.base().join("mcp-missionless-project");
    let space = run_firm(
        &home,
        &project_root,
        &[
            "space",
            "init",
            "--id",
            "mcp-space-missionless",
            "--project-binding",
            &project_id,
        ],
    );
    assert!(space.status.success(), "space init failed: {space:?}");
    let host_id = seed_member_in_active_space_with_provider(
        &home,
        &project_root,
        "missionless-host",
        "host",
        "codex",
    );
    let team_id = seed_team_without_mission(&home, &project_root, &host_id, "missionless", &[]);

    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Mission-less TeamRun",
                "agent_team_id": team_id,
                "members": [
                    {"name": "lead", "role": "coordinator", "provider": "kimi", "agent_member_id": host_id}
                ]
            }
        }),
    );
    let payload = call_payload(&response);
    let team_run_id = payload["team_run_id"].as_str().expect("team_run_id");
    assert_eq!(payload["host_runtime"]["mode"], "managed");
    assert_eq!(
        payload["host_runtime"]["delivery_guarantee"],
        "daemon_managed"
    );
    assert_eq!(
        payload["host_runtime"]["runtime_residency"],
        "managed_member_run"
    );
    assert!(payload["host_runtime"]["warning"].is_null());
    assert!(
        payload["mission_id"].is_null(),
        "mission-less Team must not report a Mission id: {payload}"
    );
    let dashboard_url = payload["dashboard_url"].as_str().expect("dashboard_url");
    assert_eq!(
        dashboard_url,
        format!(
            "http://127.0.0.1:5173/?api=.&surface=team&team={team_run_id}&space=mcp-space-missionless&project={project_id}"
        ),
        "mission-less dashboard URL must not carry an empty mission selector"
    );
}
