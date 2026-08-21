use super::*;

#[test]
fn mcp_stdio_external_interactive_member_authorship() {
    let home = TempHome::new("mcp-stdio-external");
    let project_id = init_project(&home, "mcp-proj");
    let project_root =
        std::fs::canonicalize(home.base().join("mcp-proj")).expect("canonical project root");
    let team_id = seed_agent_team(&home, &project_root, "external");
    let external_member_id =
        seed_member_in_active_space(&home, &project_root, "external-reviewer", "reviewer");
    let added = run_firm(
        &home,
        &project_root,
        &[
            "team",
            "add-member",
            "--id",
            &team_id,
            "--member",
            &external_member_id,
        ],
    );
    assert!(added.status.success(), "team add-member failed: {added:?}");
    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let response = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "mcp-stdio-test", "version": "0"},
        }),
    );
    assert!(response["result"]["capabilities"]["tools"].is_object());
    mcp.notify("notifications/initialized");

    // One driven member plus one declared external interactive member.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "External authorship gate",
                "agent_team_id": team_id,
                "execution_root": project_root,
                "members": [
                    {"name": "lead", "role": "coordinator", "provider": "kimi", "agent_member_id": "mcp-host-external"},
                    {"name": "ext-reviewer", "role": "reviewer", "provider": "kimi", "agent_member_id": external_member_id, "execution_mode": "external_interactive", "initial_work": "Review the proposed change and report evidence."}
                ]
            }
        }),
    );
    let payload = call_payload(&response);
    let team_run_id = payload["team_run_id"]
        .as_str()
        .expect("team_run_id")
        .to_string();
    let member_ids: Vec<String> = payload["member_run_ids"]
        .as_array()
        .expect("member_run_ids")
        .iter()
        .map(|id| id.as_str().expect("member id").to_string())
        .collect();
    assert_eq!(member_ids.len(), 2, "member ids: {payload}");
    let work = &payload["works"][0];
    let work_id = work["id"].as_str().expect("Work id").to_string();
    assert_eq!(
        work["active_member_run_id"].as_str(),
        Some(member_ids[1].as_str())
    );

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "sender_runtime_id": "host",
                "recipient_runtime_ids": [member_ids[1]],
                "kind": "message",
                "body": "Please review the linked Work and reply with evidence.",
                "work_id": work_id
            }
        }),
    );
    let host_request = call_payload(&response);
    let request_id = host_request["message_id"]
        .as_str()
        .expect("request id")
        .to_string();
    let conversation_correlation = host_request["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();

    // The external session's own authorship is accepted with explicit
    // provenance and keeps the Work-linked conversation lineage.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "sender_runtime_id": external_member_id,
                "sender_kind": "agent_member",
                "recipient_runtime_ids": ["host"],
                "kind": "message",
                "body": "External review: no defects found",
                "work_id": work_id,
                "correlation_id": conversation_correlation.clone(),
                "causation_id": request_id.clone()
            }
        }),
    );
    let sent = call_payload(&response);
    let reply_id = sent["message_id"].as_str().expect("message_id").to_string();
    assert_eq!(
        sent["correlation_id"].as_str(),
        Some(conversation_correlation.as_str())
    );
    let store = HarnessStore::new(home.spaces_dir().join("mcp-space-external"));
    let reply = store
        .legacy_team_messages()
        .expect("team messages")
        .into_iter()
        .rev()
        .find(|message| message.id == reply_id)
        .expect("external reply row");
    assert_eq!(
        reply
            .sender
            .as_ref()
            .and_then(|sender| sender.authn_source.as_deref()),
        Some("mcp:external_interactive"),
        "external authorship provenance: {reply:?}"
    );

    // A driven member's authorship from the same unbound connection stays
    // rejected.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "sender_runtime_id": "mcp-host-external",
                "sender_kind": "agent_member",
                "recipient_runtime_ids": [member_ids[1]],
                "kind": "message",
                "body": "attempted driven-member impersonation",
                "correlation_id": conversation_correlation.clone()
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .expect("impersonation error")
        .contains("unbound MCP connections may not author"));

    // Inbox read and ack for the external member work over MCP as well: its
    // deliveries never leave queued on their own, and the ack proceeds
    // straight from queued.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_inbox",
            "arguments": {"team_run_id": team_run_id, "member_run_id": member_ids[1]}
        }),
    );
    let inbox = call_payload(&response);
    assert_eq!(
        inbox["messages"].as_array().map(Vec::len),
        Some(1),
        "external inbox: {inbox}"
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_message_acknowledge",
            "arguments": {"message_id": request_id, "member_id": member_ids[1]}
        }),
    );
    call_payload(&response);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_inbox",
            "arguments": {"team_run_id": team_run_id, "member_run_id": member_ids[1]}
        }),
    );
    let inbox = call_payload(&response);
    assert_eq!(
        inbox["messages"].as_array().map(Vec::len),
        Some(0),
        "acked mail leaves the actionable inbox: {inbox}"
    );

    // Close freezes only the Harness coordination binding. The still-running
    // external process cannot author AgentMember mail until explicit Reopen.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_close_member",
            "arguments": {
                "team_run_id": team_run_id,
                "member_run_id": member_ids[1],
                "reason": "external review accepted"
            }
        }),
    );
    let closed = call_payload(&response);
    assert_eq!(closed["runtime_effect"].as_str(), Some("none"));
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "sender_runtime_id": external_member_id,
                "sender_kind": "agent_member",
                "recipient_runtime_ids": ["host"],
                "kind": "message",
                "body": "must not author after coordination close",
                "correlation_id": conversation_correlation
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .expect("closed external error")
        .contains("coordination is closed"));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_reopen_member",
            "arguments": {
                "team_run_id": team_run_id,
                "member_run_id": member_ids[1],
                "reason": "continue external review"
            }
        }),
    );
    let reopened = call_payload(&response);
    assert_eq!(
        reopened["reopen"]["member_run"]["id"].as_str(),
        Some(member_ids[1].as_str())
    );
    assert_eq!(
        reopened["reopen"]["member_run"]["runtime_generation"].as_u64(),
        Some(2)
    );
    assert_eq!(
        reopened["reopen"]["runtime_activation"].as_str(),
        Some("external_user_driven")
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "sender_runtime_id": external_member_id,
                "sender_kind": "agent_member",
                "recipient_runtime_ids": ["host"],
                "kind": "message",
                "body": "authoring resumes after explicit reopen",
                "correlation_id": conversation_correlation
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(false));
}
