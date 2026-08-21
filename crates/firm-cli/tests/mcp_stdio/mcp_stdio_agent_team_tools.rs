use super::*;

#[test]
fn mcp_stdio_agent_team_tools() {
    let home = TempHome::new("mcp-stdio");
    let project_id = init_project(&home, "mcp-proj");
    let project_root =
        std::fs::canonicalize(home.base().join("mcp-proj")).expect("canonical project root");
    let stable_agent_id =
        seed_canonical_member(&home, &project_root, &project_id, "main", "coordinator");
    let worker_agent_id =
        seed_member_in_active_space(&home, &project_root, "worker-main", "implementer");
    let repair_agent_id = seed_member_in_active_space(&home, &project_root, "repair-main", "fixer");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let mut mcp = McpClient::spawn(
        &home,
        &project_id,
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100"),
        ],
    );

    // 1. initialize → protocol/server handshake, then the initialized
    //    notification (accepted silently).
    let response = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "mcp-stdio-test", "version": "0"},
        }),
    );
    let result = &response["result"];
    assert_eq!(result["protocolVersion"].as_str(), Some("2024-11-05"));
    assert_eq!(result["serverInfo"]["name"].as_str(), Some("harness"));
    assert!(
        result["serverInfo"]["version"].is_string(),
        "serverInfo.version: {result}"
    );
    assert!(
        result["capabilities"]["tools"].is_object(),
        "capabilities.tools: {result}"
    );
    mcp.notify("notifications/initialized");

    // 2. tools/list exposes the current Mission surface. Legacy Wave tools
    // are absent rather than advertised as tempting tombstones.
    let response = mcp.request("tools/list", serde_json::json!({}));
    let tools = response["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        names,
        [
            "agentfirm_member_trust_mutate",
            "remote_fabric_status",
            "remote_fabric_operation_show",
            "mission_create",
            "mission_update_context",
            "mission_close",
            "mission_list",
            "team_run_create",
            "team_run_work_list",
            "team_run_work_show",
            "team_run_work_create",
            "team_run_work_assign",
            "team_run_work_rebind",
            "team_run_work_block",
            "team_run_work_resume",
            "team_run_work_release",
            "team_run_work_request_changes",
            "team_run_work_cancel",
            "team_run_work_reconcile_delivery",
            "collaboration_delegation_list",
            "collaboration_delegation_show",
            "execution_node_list",
            "execution_node_show",
            "team_run_add_member",
            "team_run_rename_member",
            "team_run_deactivate_member",
            "team_run_start",
            "team_run_cancel",
            "team_message_acknowledge",
            "team_run_list",
            "team_run_status",
            "team_run_board_summary",
            "team_run_host_inbox",
            "team_run_inbox",
            "team_run_send_message",
            "team_run_reconcile_delivery",
            "team_inbox_list",
            "team_run_answer_message",
            "team_run_steer_member",
            "team_run_interrupt_member",
            "team_run_close_member",
            "team_run_reopen_member",
            "team_run_events"
        ]
    );
    for tool in tools {
        assert!(tool["description"].is_string(), "tool description: {tool}");
        assert_eq!(tool["inputSchema"]["type"].as_str(), Some("object"));
    }
    for name in [
        "collaboration_delegation_list",
        "collaboration_delegation_show",
    ] {
        let schema = &tools
            .iter()
            .find(|tool| tool["name"].as_str() == Some(name))
            .unwrap()["inputSchema"];
        assert!(
            schema["properties"].get("company_id").is_none(),
            "MCP collaboration reads must resolve Company from the selected Execution Space"
        );
    }
    let collaboration_scope_spoof = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "collaboration_delegation_list",
            "arguments": {"company_id": "caller-selected-company"}
        }),
    );
    assert_eq!(collaboration_scope_spoof["result"]["isError"], true);
    assert!(collaboration_scope_spoof["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unknown arguments"));
    let remote_status = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "remote_fabric_status",
            "arguments": {"company_id": "company-mcp-test"}
        }),
    ));
    assert_eq!(remote_status["read_only"].as_bool(), Some(true));
    assert_eq!(remote_status["company_id"], "company-mcp-test");
    assert!(remote_status["local_node_id"].is_string());
    assert!(remote_status["node_local"].is_null());
    assert!(remote_status["control_plane"].is_null());
    let assign_descriptor = tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some("team_run_work_assign"))
        .expect("team_run_work_assign definition")["description"]
        .as_str()
        .expect("team_run_work_assign description");
    assert!(assign_descriptor.contains("first assignment of open Work"));
    assert!(assign_descriptor.contains("team_run_work_rebind"));
    let rebind_schema = &tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some("team_run_work_rebind"))
        .expect("team_run_work_rebind definition")["inputSchema"];
    assert_eq!(
        rebind_schema["required"],
        serde_json::json!([
            "team_run_id",
            "work_id",
            "member_run_id",
            "expected_version"
        ])
    );
    let reconcile_work_schema = &tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some("team_run_work_reconcile_delivery"))
        .expect("team_run_work_reconcile_delivery definition")["inputSchema"];
    assert_eq!(
        reconcile_work_schema["required"],
        serde_json::json!([
            "team_run_id",
            "delivery_id",
            "supervisor_id",
            "supervisor_generation"
        ])
    );
    let create_schema = tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some("team_run_create"))
        .expect("team_run_create definition");
    assert!(create_schema["inputSchema"]["properties"]
        .get("mission_id")
        .is_none());
    assert!(create_schema["inputSchema"]["properties"]
        .get("wave_id")
        .is_none());
    assert!(create_schema["inputSchema"]["required"]
        .as_array()
        .expect("team_run_create required")
        .iter()
        .any(|field| field == "agent_team_id"));
    assert!(
        create_schema["inputSchema"]["properties"]
            .get("execution_root")
            .is_some(),
        "MCP create accepts execution_root: {create_schema}"
    );
    assert!(
        create_schema["inputSchema"]["properties"]
            .get("host_thread_id")
            .is_some(),
        "MCP create accepts exact native Host binding: {create_schema}"
    );
    assert!(
        create_schema["inputSchema"]["properties"]["members"]["items"]["properties"]
            .get("provider_cwd_hint")
            .is_some(),
        "MCP create accepts member provider_cwd_hint: {create_schema}"
    );
    let start_descriptor = tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some("team_run_start"))
        .expect("team_run_start definition")["description"]
        .as_str()
        .expect("team_run_start description");
    for current_mode in ["codex_app_server", "kimi_acp", "claude_agent_sdk"] {
        assert!(
            start_descriptor.contains(current_mode),
            "descriptor omits executable mode {current_mode}: {start_descriptor}"
        );
    }
    assert!(
        start_descriptor.contains("codex_exec and claude_cli are rejected"),
        "descriptor must make the retired execution-mode boundary explicit: {start_descriptor}"
    );
    assert!(start_descriptor.contains("never store_root"));
    assert!(start_descriptor.contains("provider-native sessions"));

    // 3. Native Mission creation through MCP (the same helper as CLI and
    // HTTP) supplies the outer identity for the TeamRun.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "mission_create",
            "arguments": {"id": "mission-mcp", "title": "MCP mission", "objective": "Exercise authoring"}
        }),
    );
    let mission = call_payload(&response);
    assert_eq!(mission["id"].as_str(), Some("mission-mcp"));
    let team_id = seed_team_for_mission(
        &home,
        &project_root,
        "mission-mcp",
        &stable_agent_id,
        "main",
        &[&worker_agent_id, &repair_agent_id],
    );
    // 4. team_run_create with two members → run id + member run ids. Mission
    // is derived through the required flat AgentTeam.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Ship v0",
                "agent_team_id": team_id,
                "execution_root": project_root,
                "budget_limit_usd": 5.5,
                "host_surface": "codex-app",
                "host_thread_id": "codex-host-mcp",
                "members": [
                    {"name": "lead", "role": "coordinator", "provider": "kimi", "agent_member_id": stable_agent_id, "initial_work": "Coordinate the TeamRun and report evidence."},
                    {"name": "worker-1", "role": "implementer", "provider": "codex", "agent_member_id": worker_agent_id, "model": "gpt-5", "provider_cwd_hint": project_root, "owned_paths": ["crates/a", "docs"], "initial_work": "Implement the requested slice and pass checks."}
                ]
            }
        }),
    );
    let payload = call_payload(&response);
    let team_run_id = payload["team_run_id"]
        .as_str()
        .expect("team_run_id")
        .to_string();
    let expected_dashboard = format!(
        "http://127.0.0.1:5173/?api=.&surface=team&team={team_run_id}&space=mcp-space-main&project={project_id}&mission=mission-mcp"
    );
    assert!(team_run_id.starts_with("team-run-"), "id: {team_run_id}");
    assert_eq!(payload["mission_id"].as_str(), Some("mission-mcp"));
    assert!(payload.get("wave_id").is_none());
    assert_eq!(
        payload["execution_root"].as_str(),
        Some(project_root.to_str().expect("project root"))
    );
    assert_eq!(
        payload["member_runs"][1]["provider_cwd_hint"].as_str(),
        Some(project_root.to_str().expect("project root"))
    );
    let member_ids: Vec<String> = payload["member_run_ids"]
        .as_array()
        .expect("member_run_ids")
        .iter()
        .map(|id| id.as_str().expect("member id").to_string())
        .collect();
    assert_eq!(member_ids.len(), 2, "member ids: {payload}");
    let initial_work = &payload["works"][0];
    let initial_work_id = initial_work["id"]
        .as_str()
        .expect("initial Work id")
        .to_string();
    assert_eq!(
        initial_work["active_member_run_id"].as_str(),
        Some(member_ids[0].as_str())
    );
    assert_eq!(
        payload["dashboard_url"].as_str(),
        Some(expected_dashboard.as_str())
    );
    // A Mission-scoped long-lived TeamRun has no runtime-owned Wave id; the
    // fresh Dashboard URL carries only canonical Team/Mission/Space context.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Mission-scoped cold-link proof",
                "agent_team_id": team_id,
                "members": [
                    {"name": "cold-link", "role": "observer", "provider": "codex", "agent_member_id": stable_agent_id}
                ]
            }
        }),
    );
    let mission_scoped = call_payload(&response);
    let mission_scoped_id = mission_scoped["team_run_id"]
        .as_str()
        .expect("mission-scoped run id");
    assert_eq!(
        mission_scoped["dashboard_url"].as_str(),
        Some(
            format!("http://127.0.0.1:5173/?api=.&surface=team&team={mission_scoped_id}&space=mcp-space-main&project={project_id}&mission=mission-mcp")
                .as_str()
        )
    );

    // 5. The thin MCP adapter can extend the same Mission-scoped run.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_add_member",
            "arguments": {
                "team_run_id": team_run_id,
                "initial_work": "repair the interaction path",
                "member": {
                    "agent_member_id": repair_agent_id,
                    "name": "repair",
                    "role": "fixer",
                    "provider": "kimi",
                    "owned_paths": ["crates/repair"]
                }
            }
        }),
    );
    let added = call_payload(&response);
    assert_eq!(
        added["work"]["active_member_run_id"].as_str(),
        added["member_run"]["id"].as_str()
    );
    assert_eq!(
        added["team_run"]["member_run_ids"].as_array().map(Vec::len),
        Some(3)
    );
    let added_member_id = added["member_run"]["id"]
        .as_str()
        .expect("added member id")
        .to_string();
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_rename_member",
            "arguments": {
                "team_run_id": team_run_id,
                "member_run_id": added_member_id,
                "name": "targeted-repair"
            }
        }),
    );
    assert_eq!(
        call_payload(&response)["name"].as_str(),
        Some("targeted-repair")
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_deactivate_member",
            "arguments": {
                "team_run_id": team_run_id,
                "member_run_id": added_member_id,
                "reason": "review found no defect"
            }
        }),
    );
    assert_eq!(call_payload(&response)["status"].as_str(), Some("stopped"));

    // 6. team_run_status → all members + dashboard URL. Work ownership does
    // not impersonate a manual-ACK message.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_status",
            "arguments": {"team_run_id": team_run_id}
        }),
    );
    let payload = call_payload(&response);
    assert_eq!(
        payload["team_run"]["id"].as_str(),
        Some(team_run_id.as_str())
    );
    let members = payload["members"].as_array().expect("members");
    assert_eq!(members.len(), 3, "members: {payload}");
    for member in members {
        assert!(
            member["member_run"]["id"].is_string(),
            "member_run row: {member}"
        );
        assert!(member.get("latest_action").is_some(), "latest_action key");
    }
    assert_eq!(payload["unacked_messages"].as_u64(), Some(0));
    assert_eq!(
        payload["dashboard_url"].as_str(),
        Some(expected_dashboard.as_str())
    );

    // 8. An unbound MCP connection cannot impersonate a ProviderRuntimeProjection. The same
    // tool remains the Host/operator/service send path and can immediately
    // create an ordinary Work-linked conversation correlation.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "sender_runtime_id": stable_agent_id,
                "sender_kind": "agent_member",
                "recipient_runtime_ids": [member_ids[1]],
                "kind": "message",
                "body": "attempted member impersonation",
                "work_id": initial_work_id.clone()
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .expect("impersonation error")
        .contains("RETIRED_WRITE_AUTHORITY"));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "sender_runtime_id": "host",
                "recipient_runtime_ids": [member_ids[1]],
                "kind": "message",
                "body": "Host coordination for the assigned slice",
                "work_id": initial_work_id.clone()
            }
        }),
    );
    let payload = call_payload(&response);
    let message_id = payload["message_id"]
        .as_str()
        .expect("message_id")
        .to_string();
    let coordination_correlation = payload["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();
    assert!(message_id.starts_with("tmsg-"), "message id: {message_id}");
    assert!(
        !coordination_correlation.is_empty(),
        "fresh conversation correlation: {payload}"
    );

    // An ambiguous crash leaves a claim. MCP reconciliation requires the exact
    // claim id and an explicit operator choice; here the audited choice is to
    // requeue, so the normal inbox remains actionable exactly once.
    let store = HarnessStore::new(home.spaces_dir().join("mcp-space-main"));
    let mut claimed_message = store
        .legacy_team_messages()
        .expect("team messages")
        .into_iter()
        .rev()
        .find(|message| message.id == message_id)
        .expect("Host coordination row");
    claimed_message.deliveries[0].status = TeamDeliveryStatus::Claimed;
    claimed_message.deliveries[0].claim_id = Some("claim-mcp-crash".into());
    claimed_message.deliveries[0].claimed_by_supervisor_id = Some("supervisor-dead".into());
    claimed_message.deliveries[0].claimed_generation = Some(1);
    claimed_message.deliveries[0].claimed_unix_ms = Some(1);
    claimed_message.deliveries[0].claim_expires_unix_ms = Some(2);
    store
        .append_team_message(&claimed_message)
        .expect("persist uncertain claim");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_reconcile_delivery",
            "arguments": {
                "team_run_id": team_run_id,
                "message_id": message_id,
                "member_run_id": member_ids[1],
                "claim_id": "claim-mcp-crash",
                "requeue": true,
                "reason": "fake provider confirms the request was never consumed"
            }
        }),
    );
    let reconciled = call_payload(&response);
    assert_eq!(
        reconciled["deliveries"][0]["status"].as_str(),
        Some("queued")
    );

    // 9. team_run_inbox reads the same latest-wins coordination projection.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_inbox",
            "arguments": {
                "team_run_id": team_run_id,
                "member_run_id": member_ids[1]
            }
        }),
    );
    let payload = call_payload(&response);
    let inbox = payload["messages"].as_array().expect("inbox messages");
    assert!(
        inbox
            .iter()
            .any(|message| message["id"].as_str() == Some(message_id.as_str())),
        "Host coordination must be actionable in MCP inbox: {payload}"
    );

    // A trusted provider runtime persists Member-originated mail with its bound
    // identity. It then appears in the Host-native inbox exposed by MCP.
    let host_message = "tmsg-provider-bound-question".to_string();
    store
        .append_team_message(&TeamMessageProjection {
            id: host_message.clone(),
            team_run_id: team_run_id.clone(),
            work_id: Some(initial_work_id.clone()),
            source_plan_ref: None,
            sender: Some(TeamActorRef {
                kind: TeamActorKind::ProviderRuntimeProjection,
                id: member_ids[0].clone(),
                display_name: Some("Provider-bound member".to_string()),
                authn_source: Some("provider_runtime_test".to_string()),
            }),
            sender_runtime_id: member_ids[0].clone(),
            recipients: vec![TeamRecipientRef {
                kind: TeamRecipientKind::Host,
                id: "host".to_string(),
            }],
            recipient_runtime_ids: vec!["host".to_string()],
            kind: ProviderDispatchIntent::Message,
            body: "QUESTION: choose interface A or B".to_string(),
            correlation_id: coordination_correlation.clone(),
            causation_id: Some(message_id.clone()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "host".to_string(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "2026-07-29T00:00:00Z".to_string(),
            }],
            created_at: "2026-07-29T00:00:00Z".to_string(),
        })
        .expect("persist provider-bound Host question");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_host_inbox",
            "arguments": {
                "host_surface": "codex-app",
                "host_thread_id": "codex-host-mcp"
            }
        }),
    );
    let payload = call_payload(&response);
    assert_eq!(payload["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        payload["runs"][0]["messages"][0]["id"].as_str(),
        Some(host_message.as_str())
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_host_inbox",
            "arguments": {
                "host_surface": "codex-app",
                "host_thread_id": "another-host"
            }
        }),
    );
    assert_eq!(
        call_payload(&response)["runs"].as_array().map(Vec::len),
        Some(0)
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_message_acknowledge",
            "arguments": {"message_id": host_message, "member_id": "host"}
        }),
    );
    assert_eq!(
        call_payload(&response)["message"]["deliveries"][0]["status"].as_str(),
        Some("acknowledged"),
        "Host intake ACK remains separate from the message's semantic answer"
    );

    // 10. team_run_events → strictly increasing seq, and the send above is
    //    journaled as a message/created event. after_seq resumes the tail.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_events",
            "arguments": {"team_run_id": team_run_id}
        }),
    );
    let payload = call_payload(&response);
    //    create journals the run, members, and initial Works; add-member and
    //    conversation events remain part of the same ordered event stream.
    let events = payload.as_array().expect("events array");
    assert!(events.len() >= 9, "events: {}", events.len());
    let seqs: Vec<u64> = events
        .iter()
        .map(|event| event["seq"].as_u64().expect("event seq"))
        .collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "seq not strictly increasing: {seqs:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["entity_type"].as_str() == Some("message")
                && event["entity_id"].as_str() == Some(message_id.as_str())
                && event["operation"].as_str() == Some("created")),
        "message created event missing: {events:?}"
    );
    let last_seq = *seqs.last().expect("last seq");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_events",
            "arguments": {"team_run_id": team_run_id, "after_seq": last_seq}
        }),
    );
    let payload = call_payload(&response);
    assert_eq!(payload.as_array().expect("events array").len(), 0);

    // 11. ACK refuses a message that has not actually been delivered.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_message_acknowledge",
            "arguments": {"message_id": message_id, "member_id": member_ids[1]}
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .expect("ack error")
        .contains("has not been delivered"));

    // Simulate the provider delivery boundary, then prove ACK persists and
    // appears in the run event stream. The provider-specific start tests own
    // actual delivery; this test owns the Host-facing MCP contract.
    let mut delivered_message = store
        .legacy_team_messages()
        .expect("team messages")
        .into_iter()
        .rev()
        .find(|message| message.id == message_id)
        .expect("coordination message row");
    delivered_message.deliveries[0].policy = TeamDeliveryPolicy::ManualAck;
    delivered_message.deliveries[0].status = TeamDeliveryStatus::Delivered;
    store
        .append_team_message(&delivered_message)
        .expect("mark coordination message as delivered manual ACK");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_status",
            "arguments": {"team_run_id": team_run_id}
        }),
    );
    assert_eq!(
        call_payload(&response)["unacked_messages"].as_u64(),
        Some(1),
        "MCP status shares the CLI actionable manual-ACK projection"
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_message_acknowledge",
            "arguments": {"message_id": message_id, "member_id": member_ids[1]}
        }),
    );
    let payload = call_payload(&response);
    assert_eq!(
        payload["message"]["deliveries"][0]["status"].as_str(),
        Some("acknowledged")
    );
    assert_eq!(
        payload["dashboard_url"].as_str(),
        Some(expected_dashboard.as_str())
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_message_acknowledge",
            "arguments": {"message_id": message_id, "member_id": member_ids[1]}
        }),
    );
    assert_eq!(
        call_payload(&response)["message"]["deliveries"][0]["status"].as_str(),
        Some("acknowledged"),
        "repeated MCP ACK remains state-idempotent"
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_status",
            "arguments": {"team_run_id": team_run_id}
        }),
    );
    assert_eq!(
        call_payload(&response)["unacked_messages"].as_u64(),
        Some(0),
        "acknowledged manual ACKs are no longer actionable"
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_events",
            "arguments": {"team_run_id": team_run_id, "after_seq": last_seq}
        }),
    );
    let payload = call_payload(&response);
    let ack_events = payload
        .as_array()
        .expect("events array")
        .iter()
        .filter(|event| {
            event["entity_id"].as_str() == Some(message_id.as_str())
                && event["operation"].as_str() == Some("updated")
                && event["summary"]
                    .as_str()
                    .is_some_and(|summary| summary.contains("acknowledged"))
        })
        .count();
    assert_eq!(
        ack_events, 1,
        "repeated MCP ACK must emit exactly one acknowledgement event"
    );

    // 12. A planning run can be cancelled through MCP using the same guarded
    // transition helper as CLI and HTTP.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_cancel",
            "arguments": {"team_run_id": team_run_id}
        }),
    );
    let payload = call_payload(&response);
    assert_eq!(payload["team_run"]["status"].as_str(), Some("cancelled"));
    assert_eq!(
        payload["dashboard_url"].as_str(),
        Some(expected_dashboard.as_str())
    );

    // 13. MCP start is asynchronous: it immediately returns the reserved
    // running projection and exact URL, then the provider completes one turn
    // in the background while the same Host session remains responsive. Turn
    // completion returns the Member to idle; it does not complete the TeamRun.
    // wave-mcp-start is seeded historical (wave_create MCP retirement is
    // already proven above; no need to repeat the same assertion here).
    seed_historical_wave(&home, &project_id, "wave-mcp-start", "mission-mcp", 3);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Finish through fake Kimi ACP",
                "agent_team_id": team_id,
                "members": [{"name": "async-worker", "role": "implementer", "provider": "kimi", "agent_member_id": stable_agent_id}]
            }
        }),
    );
    let startable = call_payload(&response);
    let startable_id = startable["team_run_id"]
        .as_str()
        .expect("startable team run id")
        .to_string();
    let daemon = run_firm_with_env(
        &home,
        &project_root,
        &["daemon", "start"],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
        ],
    );
    assert!(
        daemon.status.success(),
        "start NodeDaemon failed: {}",
        String::from_utf8_lossy(&daemon.stderr)
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_start",
            "arguments": {"team_run_id": startable_id, "idle_timeout_s": 5}
        }),
    );
    let started = call_payload(&response);
    assert_eq!(started["team_run"]["status"].as_str(), Some("running"));
    assert_eq!(
        started["dashboard_url"].as_str(),
        Some(
            format!("http://127.0.0.1:5173/?api=.&surface=team&team={startable_id}&space=mcp-space-main&project={project_id}&mission=mission-mcp")
                .as_str()
        )
    );
    let mut idle = None;
    for _ in 0..100 {
        let response = mcp.request(
            "tools/call",
            serde_json::json!({
                "name": "team_run_status",
                "arguments": {"team_run_id": startable_id}
            }),
        );
        let status = call_payload(&response);
        let member_is_idle = status["members"].as_array().is_some_and(|members| {
            members
                .iter()
                .any(|member| member["member_run"]["status"].as_str() == Some("idle"))
        });
        if status["team_run"]["status"].as_str() == Some("running") && member_is_idle {
            idle = Some(status);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        idle.is_some(),
        "MCP-started Member did not return to idle while TeamRun stayed running"
    );
    let stopped = run_firm(&home, &project_root, &["daemon", "stop"]);
    assert!(
        stopped.status.success(),
        "stop NodeDaemon failed: {stopped:?}"
    );

    // Mission closeout is a separate Host decision and no Legacy Wave tool is
    // part of the MCP capability surface.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "mission_create",
            "arguments": {"id": "mission-close", "title": "Close me", "objective": "Prove MCP closeout"}
        }),
    );
    call_payload(&response);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "mission_close",
            "arguments": {"mission_id": "mission-close", "outcome": "all intent satisfied", "completed_by": "mcp-host"}
        }),
    );
    let closed = call_payload(&response);
    assert_eq!(closed["status"].as_str(), Some("completed"));
    assert_eq!(closed["completed_by"].as_str(), Some("mcp-host"));
    assert_eq!(
        closed["outcome_summary"].as_str(),
        Some("all intent satisfied")
    );
    assert!(closed.get("wave_ids").is_none());

    // 14. Unknown method → JSON-RPC -32601; unknown tool → -32602; a failing
    //    tool call → isError:true with the reason as text.
    let response = mcp.request("harness/no_such_method", serde_json::json!({}));
    assert_eq!(response["error"]["code"].as_i64(), Some(-32601));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({"name": "no_such_tool", "arguments": {}}),
    );
    assert_eq!(response["error"]["code"].as_i64(), Some(-32602));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_status",
            "arguments": {"team_run_id": "team-run-does-not-exist"}
        }),
    );
    let result = &response["result"];
    assert_eq!(result["isError"].as_bool(), Some(true));
    assert!(
        result["content"][0]["text"]
            .as_str()
            .expect("error text")
            .contains("team run not found"),
        "error payload: {result}"
    );
}
