use super::*;

#[test]
fn removed_mcp_team_run_message_writer_fails_unknown_with_zero_store_delta() {
    let home = TempHome::new("mcp-retired-message-writer");
    let project_id = init_project(&home, "mcp-retired-message-project");
    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let _ = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "retired-writer-test", "version": "0"}
        }),
    );
    let before = directory_snapshot(&home.spaces_dir());
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": "hostile-team-run",
                "sender_runtime_id": "hostile-sender",
                "sender_kind": "agent_member",
                "recipient_runtime_ids": ["hostile-recipient"],
                "kind": "message",
                "body": "must not persist"
            }
        }),
    );
    let error = response["error"]["message"]
        .as_str()
        .expect("removed tool must fail as unknown");
    assert!(error.contains("unknown tool"), "{error}");
    assert_eq!(
        directory_snapshot(&home.spaces_dir()),
        before,
        "retired MCP writer must produce a byte-zero store delta"
    );

    for retired in ["work_delegation_create", "work_delegation_cancel"] {
        let response = mcp.request(
            "tools/call",
            serde_json::json!({
                "name": retired,
                "arguments": {
                    "delegation_id": "hostile-delegation",
                    "idempotency_key": "must-not-write"
                }
            }),
        );
        let error = response["error"]["message"]
            .as_str()
            .expect("removed tool must fail as an unknown JSON-RPC tool")
            .to_string();
        assert!(error.contains("unknown tool"), "{retired}: {error}");
        assert_eq!(
            directory_snapshot(&home.spaces_dir()),
            before,
            "retired local WorkDelegation MCP authority must stay byte-zero"
        );
    }
}
