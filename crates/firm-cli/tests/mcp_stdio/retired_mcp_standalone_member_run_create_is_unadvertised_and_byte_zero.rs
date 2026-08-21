use super::*;

#[test]
fn retired_mcp_standalone_member_run_create_is_unadvertised_and_byte_zero() {
    let home = TempHome::new("mcp-retired-member-run-create");
    let project_id = init_project(&home, "mcp-retired-member-run-project");
    let project_root = home.base().join("mcp-retired-member-run-project");
    let execution_space_id = "mcp-space-retired-member-run-create";
    let space = run_firm(
        &home,
        &project_root,
        &[
            "space",
            "init",
            "--id",
            execution_space_id,
            "--project-binding",
            &project_id,
        ],
    );
    assert!(space.status.success(), "space init failed: {space:?}");
    let mut mcp = McpClient::spawn(
        &home,
        &project_id,
        &[
            ("AGENTFIRM_MCP_ACTOR_KIND", "human"),
            ("AGENTFIRM_MCP_ACTOR_ID", "retired-create-caller"),
        ],
    );
    let _ = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "retired-member-run-create-test", "version": "0"}
        }),
    );
    let listed = mcp.request("tools/list", serde_json::json!({}));
    let member_trust_tool = listed["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .find(|tool| tool["name"] == "agentfirm_member_trust_mutate")
        .expect("Member Trust lifecycle tool");
    assert!(
        !member_trust_tool.to_string().contains("create_member_run"),
        "standalone MemberRun create must not be advertised: {member_trust_tool}"
    );
    let lifecycle_description = member_trust_tool["description"]
        .as_str()
        .expect("Member Trust tool description");
    for contract in [
        "Close requires Active",
        "Reopen requires Closed",
        "ResumeNativeSession requires Active plus a Disconnected, Failed, or Stopped runtime",
        "combined TeamRun authority",
    ] {
        assert!(
            lifecycle_description.contains(contract),
            "MCP lifecycle contract is not honestly advertised ({contract}): {lifecycle_description}"
        );
    }

    let store = HarnessStore::new(home.spaces_dir().join(execution_space_id));
    let authority_counts = || {
        (
            store.team_runs().expect("TeamRun projections").len(),
            store
                .member_runs()
                .expect("legacy runtime projections")
                .len(),
            store
                .canonical_operations()
                .expect("canonical trust operations")
                .len(),
        )
    };
    let before_counts = authority_counts();
    let before_bytes = directory_snapshot(&home.spaces_dir());
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "agentfirm_member_trust_mutate",
            "arguments": {
                "command": {
                    "command": "create_member_run",
                    "run": {
                        "id": "retired-standalone-member-run",
                        "agent_member_id": "retired-standalone-member",
                        "team_run_id": "retired-standalone-team-run",
                        "role_snapshot": "implementer",
                        "coordination_status": "active",
                        "runtime_status": "idle",
                        "runtime_generation": 1,
                        "version": 1,
                        "started_at": "unix-ms:1"
                    }
                },
                "idempotency_key": "retired-member-run-create",
                "expected_version": 0
            }
        }),
    );
    assert!(
        call_error_text(&response).contains("unsupported or retired MCP Member Trust command"),
        "retired standalone create must fail closed: {response}"
    );
    assert_eq!(
        authority_counts(),
        before_counts,
        "retired MCP command must not create a TeamRun, legacy runtime projection, or canonical operation"
    );
    assert_eq!(
        directory_snapshot(&home.spaces_dir()),
        before_bytes,
        "retired MCP command must produce a byte-zero Store delta"
    );
}
