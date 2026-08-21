use super::*;

#[test]
fn mcp_answers_canonical_provider_request_with_transport_identity_and_exact_retry() {
    let home = TempHome::new("mcp-provider-interaction-message");
    let project_id = init_project(&home, "mcp-provider-interaction");
    let project_root = home.base().join("mcp-provider-interaction");
    let team_id = seed_agent_team(&home, &project_root, "provider-interaction");
    let worker_id = seed_member_in_active_space_with_provider(
        &home,
        &project_root,
        "provider-interaction-worker",
        "implementer",
        "codex",
    );
    let added = run_firm(
        &home,
        &project_root,
        &[
            "team",
            "add-member",
            "--id",
            &team_id,
            "--member",
            &worker_id,
        ],
    );
    assert!(
        added.status.success(),
        "add canonical worker: {}",
        String::from_utf8_lossy(&added.stderr)
    );
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin-mcp-answer"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let daemon = run_firm_with_env(
        &home,
        &project_root,
        &["daemon", "start"],
        &[("PATH", path.as_str()), ("FAKE_CODEX_ASK", "1")],
    );
    assert!(
        daemon.status.success(),
        "start NodeDaemon: {}",
        String::from_utf8_lossy(&daemon.stderr)
    );

    let host_id = "mcp-host-provider-interaction";
    let mut mcp = McpClient::spawn(
        &home,
        &project_id,
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_ASK", "1"),
            ("AGENTFIRM_MCP_ACTOR_KIND", "agent_member"),
            ("AGENTFIRM_MCP_ACTOR_ID", host_id),
        ],
    );
    let _ = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "provider-bridge-test", "version": "0"}
        }),
    );
    let created = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Exercise canonical MCP provider response bridge",
                "agent_team_id": team_id,
                "members": [{
                    "name": "mcp-question-worker",
                    "role": "implementer",
                    "provider": "codex",
                    "execution_mode": "codex_app_server",
                    "agent_member_id": worker_id,
                    "initial_work": "Ask one deterministic provider question"
                }]
            }
        }),
    ));
    let run_id = created["team_run_id"]
        .as_str()
        .expect("team run id")
        .to_string();
    let started = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_start",
            "arguments": {"team_run_id": run_id, "idle_timeout_s": 5}
        }),
    ));
    assert_eq!(started["team_run"]["status"], "running");

    let store = HarnessStore::new(home.spaces_dir().join("mcp-space-provider-interaction"));
    let execution_space_id = "mcp-space-provider-interaction";
    let mut request_id = None;
    for _ in 0..100 {
        request_id = store
            .fabric_messages(execution_space_id)
            .expect("canonical Message fabric")
            .into_iter()
            .find(|message| {
                serde_json::to_value(message).expect("message JSON")["kind"].as_str()
                    == Some("provider_interaction_request")
            })
            .map(|message| message.id);
        if request_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let request_id = request_id.unwrap_or_else(|| {
        let status = mcp.request(
            "tools/call",
            serde_json::json!({
                "name": "team_run_status",
                "arguments": {"team_run_id": run_id}
            }),
        );
        panic!(
            "NodeDaemon did not author the provider request Message; status={status}; messages={:?}; actions={:?}",
            store.fabric_messages(execution_space_id),
            store.member_actions()
        )
    });
    let mut author_command_settled = false;
    for _ in 0..100 {
        author_command_settled = store
            .runtime_commands(execution_space_id)
            .expect("RuntimeCommand journal")
            .into_iter()
            .any(|command| {
                let command = serde_json::to_value(command).expect("RuntimeCommand JSON");
                command["command"] == "author_message"
                    && command["status"] == "applied"
                    && command["result"]["id"] == request_id
            });
        if author_command_settled {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        author_command_settled,
        "provider request AuthorMessage RuntimeCommand must settle before hostile fixture publication"
    );
    let foreign_execution_space_id = "mcp-space-foreign-same-run-id";
    // The NodeDaemon settles the RuntimeCommand immediately after authoring
    // the request Message. Both canonical mutations atomically replace the
    // complete trust ledger, so an out-of-band hostile fixture append must
    // share the ordinary Store writer lock or the settle rename can discard
    // it. Retain the guard through the read-only MCP projections to prove the
    // foreign row exists while status/inbox ignore it.
    let hostile_fixture_guard = store
        .acquire_exclusive_migration_guard()
        .expect("serialize hostile fixture with canonical Store writers");
    inject_foreign_space_copy_of_message(
        &home.spaces_dir().join("mcp-space-provider-interaction"),
        execution_space_id,
        foreign_execution_space_id,
        &request_id,
        &hostile_fixture_guard,
    );
    assert_eq!(
        store
            .fabric_messages(foreign_execution_space_id)
            .expect("foreign canonical Message fixture")
            .len(),
        1,
        "hostile fixture must create one colliding foreign-space Message"
    );
    // The exact authoring RuntimeCommand is already terminal, so no earlier
    // canonical replacement can now erase the fixture. Release the guard
    // before MCP status takes the Store's strict current-TeamRun resolver lock;
    // the foreign row remains durable until the later answer mutation.
    drop(hostile_fixture_guard);

    let status_before_answer = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_status",
            "arguments": {"team_run_id": run_id}
        }),
    ));
    assert_eq!(
        status_before_answer["message_summary"]["provider_interaction_requests"].as_u64(),
        Some(1)
    );
    assert_eq!(
        status_before_answer["message_summary"]["provider_interaction_responses"].as_u64(),
        Some(0)
    );
    assert_eq!(
        status_before_answer["message_summary"]["awaiting_host_response"].as_u64(),
        Some(1)
    );
    let host_inbox_before_answer = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_inbox",
            "arguments": {"team_run_id": run_id, "member_run_id": "host"}
        }),
    ));
    assert!(
        host_inbox_before_answer["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| message["id"] == request_id)),
        "canonical Host inbox must expose the unanswered request: {host_inbox_before_answer}"
    );
    let mut impostor = McpClient::spawn(
        &home,
        &project_id,
        &[
            ("AGENTFIRM_MCP_ACTOR_KIND", "service"),
            ("AGENTFIRM_MCP_ACTOR_ID", "not-the-team-host"),
        ],
    );
    let unauthorized = impostor.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_answer_message",
            "arguments": {
                "team_run_id": run_id,
                "message_id": request_id,
                "option_id": "implementation::0"
            }
        }),
    );
    assert!(call_error_text(&unauthorized).contains("UNAUTHORIZED_ACTOR"));

    let spoof = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_answer_message",
            "arguments": {
                "team_run_id": run_id,
                "message_id": request_id,
                "option_id": "implementation::0",
                "resolved_by": "host"
            }
        }),
    );
    assert!(call_error_text(&spoof).contains("resolved_by"));

    let invalid = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_answer_message",
            "arguments": {
                "team_run_id": run_id,
                "message_id": request_id,
                "option_id": "not-exposed"
            }
        }),
    );
    assert!(call_error_text(&invalid).contains("does not expose option_id"));
    assert!(store
        .fabric_messages(execution_space_id)
        .expect("messages after rejected answers")
        .iter()
        .all(
            |message| serde_json::to_value(message).expect("message JSON")["kind"]
                != "provider_interaction_response"
        ));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_answer_message",
            "arguments": {
                "team_run_id": run_id,
                "message_id": request_id,
                "option_id": "implementation::0"
            }
        }),
    );
    let payload = call_payload(&response);
    assert_eq!(
        payload["kind"].as_str(),
        Some("provider_interaction_response")
    );
    let retry = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_answer_message",
            "arguments": {
                "team_run_id": run_id,
                "message_id": request_id,
                "option_id": "implementation::0"
            }
        }),
    ));
    assert_eq!(retry["id"], payload["id"]);
    let status_after_retry = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_status",
            "arguments": {"team_run_id": run_id}
        }),
    ));
    assert_eq!(
        status_after_retry["message_summary"]["provider_interaction_requests"].as_u64(),
        Some(1)
    );
    assert_eq!(
        status_after_retry["message_summary"]["provider_interaction_responses"].as_u64(),
        Some(1)
    );
    assert_eq!(
        status_after_retry["message_summary"]["awaiting_host_response"].as_u64(),
        Some(0),
        "exact retry must leave one visible resolved correlation: {status_after_retry}"
    );
    let actionable_host_inbox = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_inbox",
            "arguments": {"team_run_id": run_id, "member_run_id": "host"}
        }),
    ));
    assert!(
        actionable_host_inbox["messages"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "answered request must leave the actionable canonical Host inbox: {actionable_host_inbox}"
    );
    let historical_host_inbox = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_inbox",
            "arguments": {"team_run_id": run_id, "member_run_id": "host", "all": true}
        }),
    ));
    assert!(
        historical_host_inbox["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| message["id"] == request_id)),
        "canonical delivery history must preserve the answered request: {historical_host_inbox}"
    );
    let responses = store
        .fabric_messages(execution_space_id)
        .expect("canonical messages")
        .into_iter()
        .filter(|message| {
            serde_json::to_value(message).expect("message JSON")["kind"]
                == "provider_interaction_response"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        responses.len(),
        1,
        "exact retry must not duplicate response"
    );
    assert_eq!(
        responses[0].causation_id.as_deref(),
        Some(request_id.as_str())
    );
    assert!(
        store
            .legacy_team_messages()
            .expect("legacy message projection")
            .is_empty(),
        "canonical provider question/answer must not revive the retired TeamMessage writer"
    );
    assert!(
        !home
            .spaces_dir()
            .join(execution_space_id)
            .join("team_messages.jsonl")
            .exists(),
        "canonical provider question/answer must not create the retired ledger"
    );
}
