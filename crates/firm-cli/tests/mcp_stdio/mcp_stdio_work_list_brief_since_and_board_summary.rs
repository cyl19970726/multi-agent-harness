use super::*;

#[test]
fn mcp_stdio_work_list_brief_since_and_board_summary() {
    let home = TempHome::new("mcp-board-reads");
    let project_id = init_project(&home, "mcp-board-reads-proj");
    let project_root = std::fs::canonicalize(home.base().join("mcp-board-reads-proj"))
        .expect("canonical project root");
    let team_id = seed_agent_team(&home, &project_root, "board-reads");

    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Exercise decision-shaped board reads over MCP",
                "agent_team_id": team_id,
                "execution_root": project_root,
                "members": [{
                    "name": "alice",
                    "role": "implementer",
                    "provider": "codex",
                    "agent_member_id": "mcp-host-board-reads",
                    "initial_work": "Ship the assigned slice."
                }]
            }
        }),
    );
    let created = call_payload(&response);
    let team_run_id = created["team_run_id"]
        .as_str()
        .expect("team run id")
        .to_string();
    let alice_id = created["member_run_ids"][0]
        .as_str()
        .expect("alice member run id")
        .to_string();
    let assigned_work_id = created["works"][0]["id"]
        .as_str()
        .expect("assigned Work id")
        .to_string();

    // A second, unassigned Work so board-summary/brief have something on
    // both sides of assigned/unassigned, and the delta cursor has more than
    // one Work's operations to distinguish.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_create",
            "arguments": {
                "team_run_id": team_run_id,
                "title": "Unassigned MCP Work",
                "completion_criteria_markdown": "Claimed and finished by any eligible member."
            }
        }),
    );
    let unassigned = call_payload(&response);
    let unassigned_work_id = unassigned["id"]
        .as_str()
        .expect("unassigned Work id")
        .to_string();

    // Full JSON list stays available and unwrapped when neither brief nor
    // since is passed -- the additive contract from issue #305.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({"name": "team_run_work_list", "arguments": {"team_run_id": team_run_id}}),
    );
    let full = call_payload(&response);
    assert_eq!(
        full["works"].as_array().map(Vec::len),
        Some(2),
        "full list: {full}"
    );

    // brief=true swaps in compact text lines instead of full Work JSON.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_list",
            "arguments": {"team_run_id": team_run_id, "brief": true}
        }),
    );
    let brief = call_payload(&response);
    let lines: Vec<&str> = brief["works_brief"]
        .as_array()
        .expect("works_brief array")
        .iter()
        .map(|line| line.as_str().expect("brief line is a string"))
        .collect();
    assert_eq!(lines.len(), 2, "one brief line per Work: {lines:?}");
    let assigned_line = lines
        .iter()
        .find(|line| line.starts_with(&assigned_work_id))
        .unwrap_or_else(|| panic!("assigned Work brief line: {lines:?}"));
    assert!(
        assigned_line.contains(&alice_id),
        "assigned brief line must carry its owner member-run id: {assigned_line}"
    );
    let unassigned_line = lines
        .iter()
        .find(|line| line.starts_with(&unassigned_work_id))
        .unwrap_or_else(|| panic!("unassigned Work brief line: {lines:?}"));
    assert!(
        unassigned_line.contains("unassigned"),
        "unassigned brief line: {unassigned_line}"
    );

    // since=0 is a delta read from the beginning: every Work comes back, plus
    // a next_since watermark to chain future calls.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_list",
            "arguments": {"team_run_id": team_run_id, "since": 0}
        }),
    );
    let since = call_payload(&response);
    assert_eq!(since["since"].as_u64(), Some(0));
    assert_eq!(
        since["works"].as_array().map(Vec::len),
        Some(2),
        "since=0: {since}"
    );
    let next_since = since["next_since"].as_u64().expect("next_since");
    assert!(next_since >= 2, "next_since: {since}");

    // A second delta read from the fresh cursor sees nothing new.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_list",
            "arguments": {"team_run_id": team_run_id, "since": next_since}
        }),
    );
    let empty = call_payload(&response);
    assert_eq!(
        empty["works"].as_array().map(Vec::len),
        Some(0),
        "no-op delta: {empty}"
    );

    // team_run_board_summary is a single bounded plain-text digest, not the
    // full board. Neither Work has been started, so both are still `open`
    // (is_claim_ready does not care whether an open Work already has an
    // owner -- start_work gates on the same readiness check) and alice's
    // MemberRunStatus never left idle.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({"name": "team_run_board_summary", "arguments": {"team_run_id": team_run_id}}),
    );
    let summary_payload = call_payload(&response);
    let summary = summary_payload["summary"].as_str().expect("summary string");
    assert!(
        summary.chars().count() <= 500,
        "summary must stay <=500 chars: {summary}"
    );
    assert!(summary.contains("open=2"), "summary: {summary}");
    assert!(summary.contains("assigned=1"), "summary: {summary}");
    assert!(summary.contains("unassigned=1"), "summary: {summary}");
    assert!(summary.contains("ready=2"), "summary: {summary}");
    assert!(summary.contains("alice: idle"), "summary: {summary}");

    let execution_space_id = "mcp-space-board-reads";
    let store = HarnessStore::new(home.spaces_dir().join(execution_space_id));
    let run = store
        .team_runs()
        .expect("TeamRuns")
        .into_iter()
        .rev()
        .find(|run| run.id == team_run_id)
        .expect("current TeamRun");
    let mut lifecycle_mcp = McpClient::spawn(
        &home,
        &project_id,
        &[
            ("AGENTFIRM_MCP_ACTOR_KIND", "agent_member"),
            ("AGENTFIRM_MCP_ACTOR_ID", "mcp-host-board-reads"),
        ],
    );
    let before_invalid_resume = directory_snapshot(&home.spaces_dir());
    let invalid_resume = lifecycle_mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "agentfirm_member_trust_mutate",
            "arguments": {
                "command": {"command": "resume_native_session", "member_run_id": alice_id, "updated_at": "unix-ms:invalid-active-idle-resume"},
                "idempotency_key": "invalid-active-idle-resume",
                "expected_version": 1
            }
        }),
    );
    assert!(
        call_error_text(&invalid_resume).contains(
            "Resume native session requires an active, disconnected, failed, or stopped MemberRun"
        ),
        "Active+Idle Resume must fail closed: {invalid_resume}"
    );
    assert_eq!(
        directory_snapshot(&home.spaces_dir()),
        before_invalid_resume,
        "invalid MCP Resume must produce a byte-zero Store delta"
    );
    let dangling_id = "member-run-mcp-dangling";
    let mut partial = run.clone();
    partial.member_run_ids.push(dangling_id.to_string());
    let mut team_run_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root().join("team_runs.jsonl"))
        .expect("open Legacy TeamRun fixture");
    writeln!(
        team_run_file,
        "{}",
        serde_json::to_string(&partial).unwrap()
    )
    .unwrap();
    team_run_file.flush().unwrap();
    let before = directory_snapshot(&home.spaces_dir());
    for tool in [
        "team_run_events",
        "team_run_board_summary",
        "team_run_status",
    ] {
        let response = mcp.request(
            "tools/call",
            serde_json::json!({"name": tool, "arguments": {"team_run_id": team_run_id}}),
        );
        assert!(
            call_error_text(&response).contains(dangling_id),
            "{tool}: {response}"
        );
    }
    let partial_close = lifecycle_mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "agentfirm_member_trust_mutate",
            "arguments": {
                "command": {"command": "close_member_run", "member_run_id": alice_id, "updated_at": "unix-ms:partial-close"},
                "idempotency_key": "partial-close-zero-effect",
                "expected_version": 1
            }
        }),
    );
    assert!(call_error_text(&partial_close).contains(dangling_id));
    assert_eq!(directory_snapshot(&home.spaces_dir()), before);

    let mut legacy = store
        .member_runs()
        .expect("legacy members")
        .into_iter()
        .find(|member| member.id == alice_id)
        .expect("source legacy member");
    legacy.id = dangling_id.to_string();
    let mut legacy_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root().join("member_runs.jsonl"))
        .expect("open Legacy member fixture");
    writeln!(legacy_file, "{}", serde_json::to_string(&legacy).unwrap()).unwrap();
    legacy_file.flush().unwrap();
    let mut canonical = store
        .trust_member_runs(execution_space_id)
        .expect("canonical members")
        .into_iter()
        .find(|member| member.id == alice_id)
        .expect("source canonical member");
    canonical.id = dangling_id.to_string();
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
        id: canonical.agent_member_id.clone(),
    };
    store
        .legacy_import_create_trust_member_run_projection(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: execution_space_id.to_string(),
                authenticated_actor: actor.clone(),
                authority_actor: Some(actor),
                command_name: "test.reconstruct_member_run".into(),
                idempotency_key: "test-reconstruct-mcp-dangling".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            canonical,
        )
        .expect("restore canonical completeness");
    call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({"name": "team_run_board_summary", "arguments": {"team_run_id": team_run_id}}),
    ));
    let foreign_space_id = "mcp-space-board-reads-foreign";
    let foreign = run_firm(
        &home,
        &project_root,
        &[
            "space",
            "init",
            "--id",
            foreign_space_id,
            "--project-binding",
            &project_id,
        ],
    );
    assert!(foreign.status.success(), "foreign space init: {foreign:?}");
    let mut foreign_mcp = McpClient::spawn(
        &home,
        &project_id,
        &[
            ("AGENTFIRM_MCP_ACTOR_KIND", "agent_member"),
            ("AGENTFIRM_MCP_ACTOR_ID", "mcp-host-board-reads"),
        ],
    );
    let before_wrong_space = directory_snapshot(&home.spaces_dir());
    let wrong_space_close = foreign_mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "agentfirm_member_trust_mutate",
            "arguments": {
                "command": {"command": "close_member_run", "member_run_id": alice_id, "updated_at": "unix-ms:wrong-space-close"},
                "idempotency_key": "wrong-space-close-zero-effect",
                "expected_version": 1
            }
        }),
    );
    let wrong_space_error = call_error_text(&wrong_space_close);
    assert!(
        wrong_space_error.contains("UNAUTHORIZED_ACTOR"),
        "wrong-space lifecycle authority must fail closed: {wrong_space_error}"
    );
    assert_eq!(directory_snapshot(&home.spaces_dir()), before_wrong_space);
}
