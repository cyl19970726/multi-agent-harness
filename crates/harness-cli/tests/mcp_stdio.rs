//! Integration coverage for `harness mcp`: the binary is spawned as a stdio
//! MCP server against an isolated HOME and driven with line-delimited
//! JSON-RPC 2.0 — initialize handshake, tools/list, the five Agent Team v0
//! tools end to end (create → start/status → send/ACK → events), and the -32601
//! unknown-method error.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use harness_core::TeamDeliveryStatus;
use harness_store::HarnessStore;

mod fake_provider;
mod harness_env;
use harness_env::{current_project_id, run_harness, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// A spawned `harness mcp` child with framed stdin/stdout. Killed on drop.
struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpClient {
    fn spawn(home: &TempHome, project_id: &str, extra_env: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
        cmd.arg("--project")
            .arg(project_id)
            .arg("mcp")
            .current_dir(home.base())
            .envs(home.envs())
            .env_remove("HARNESS_ROOT")
            .env_remove("HARNESS_PROJECT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().expect("spawn harness mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        Self {
            child,
            stdin,
            stdout,
            next_id: 0,
        }
    }

    /// Send one JSON-RPC request and read its one-line response.
    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{request}").expect("write request");
        self.stdin.flush().expect("flush request");
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).expect("read response");
        assert!(
            read > 0,
            "harness mcp closed stdout before answering {method}"
        );
        let response: serde_json::Value = serde_json::from_str(line.trim())
            .unwrap_or_else(|e| panic!("response to {method} not JSON ({e}): {line}"));
        assert_eq!(
            response["id"].as_u64(),
            Some(id),
            "response id mismatch for {method}: {response}"
        );
        response
    }

    /// Send a notification (no id): the server must not answer.
    fn notify(&mut self, method: &str) {
        let notification = serde_json::json!({"jsonrpc": "2.0", "method": method});
        writeln!(self.stdin, "{notification}").expect("write notification");
        self.stdin.flush().expect("flush notification");
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Assert a `tools/call` response is not an error and parse the JSON payload
/// out of its text content block.
fn call_payload(response: &serde_json::Value) -> serde_json::Value {
    let result = &response["result"];
    assert_eq!(
        result["isError"].as_bool(),
        Some(false),
        "tools/call failed: {response}"
    );
    let text = result["content"][0]["text"]
        .as_str()
        .expect("text content block");
    serde_json::from_str(text).unwrap_or_else(|e| panic!("tool payload not JSON ({e}): {text}"))
}

#[test]
fn mcp_stdio_agent_team_tools() {
    let home = TempHome::new("mcp-stdio");
    let project_id = init_project(&home, "mcp-proj");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let mut mcp = McpClient::spawn(
        &home,
        &project_id,
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
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

    // 2. tools/list preserves the original five TeamRun tools and adds the
    // native Mission/Wave authoring surface.
    let response = mcp.request("tools/list", serde_json::json!({}));
    let tools = response["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        names,
        [
            "mission_create",
            "mission_close",
            "mission_list",
            "wave_create",
            "wave_list",
            "wave_gate",
            "team_run_create",
            "team_run_start",
            "team_run_cancel",
            "team_message_acknowledge",
            "team_run_list",
            "team_run_status",
            "team_run_send_message",
            "team_run_resolve_interaction",
            "team_run_steer_member",
            "team_run_interrupt_member",
            "team_run_events"
        ]
    );
    for tool in tools {
        assert!(tool["description"].is_string(), "tool description: {tool}");
        assert_eq!(tool["inputSchema"]["type"].as_str(), Some("object"));
    }
    let create_schema = tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some("team_run_create"))
        .expect("team_run_create definition");
    assert!(
        create_schema["inputSchema"]["properties"]
            .get("mission_id")
            .is_some(),
        "MCP create accepts mission_id: {create_schema}"
    );
    assert!(
        create_schema["inputSchema"]["properties"]
            .get("wave_id")
            .is_some(),
        "MCP create accepts wave_id: {create_schema}"
    );
    assert!(
        create_schema["inputSchema"]["properties"]
            .get("execution_root")
            .is_some(),
        "MCP create accepts execution_root: {create_schema}"
    );
    assert!(
        create_schema["inputSchema"]["properties"]["members"]["items"]["properties"]
            .get("worktree_ref")
            .is_some(),
        "MCP create accepts member worktree_ref: {create_schema}"
    );
    let start_descriptor = tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some("team_run_start"))
        .expect("team_run_start definition")["description"]
        .as_str()
        .expect("team_run_start description");
    for current_mode in ["codex_exec", "codex_app_server", "kimi_acp", "claude_cli"] {
        assert!(
            start_descriptor.contains(current_mode),
            "descriptor omits executable mode {current_mode}: {start_descriptor}"
        );
    }
    assert!(start_descriptor.contains("never store_root"));
    assert!(start_descriptor.contains("provider-native sessions"));

    // 3. Native Mission + Wave creation through MCP (the same helpers as CLI
    // and HTTP) supplies the outer identity for the TeamRun.
    let project_root =
        std::fs::canonicalize(home.base().join("mcp-proj")).expect("canonical project root");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "mission_create",
            "arguments": {"id": "mission-mcp", "title": "MCP mission", "objective": "Exercise authoring"}
        }),
    );
    let mission = call_payload(&response);
    assert_eq!(mission["id"].as_str(), Some("mission-mcp"));
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "wave_create",
            "arguments": {
                "id": "wave-mcp",
                "mission_id": "mission-mcp",
                "index": 2,
                "title": "Team wave",
                "objective": "Run members",
                "executor_kind": "agent_team"
            }
        }),
    );
    let wave = call_payload(&response);
    assert_eq!(wave["mission_id"].as_str(), Some("mission-mcp"));
    assert_eq!(wave["index"].as_u64(), Some(2));

    // 4. team_run_create with two members → run id + member run ids.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Ship v0",
                "mission_id": "mission-mcp",
                "wave_id": "wave-mcp",
                "execution_root": project_root,
                "budget_limit_usd": 5.5,
                "members": [
                    {"name": "lead", "role": "coordinator", "provider": "kimi"},
                    {"name": "worker-1", "role": "implementer", "provider": "codex", "model": "gpt-5", "worktree_ref": project_root, "owned_paths": ["crates/a", "docs"]}
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
        "http://127.0.0.1:5173/?api=.&surface=team&team={team_run_id}&project={project_id}"
    );
    assert!(team_run_id.starts_with("team-run-"), "id: {team_run_id}");
    assert_eq!(payload["mission_id"].as_str(), Some("mission-mcp"));
    assert_eq!(payload["wave_id"].as_str(), Some("wave-mcp"));
    assert_eq!(
        payload["execution_root"].as_str(),
        Some(project_root.to_str().expect("project root"))
    );
    assert_eq!(
        payload["member_runs"][1]["worktree_ref"].as_str(),
        Some(project_root.to_str().expect("project root"))
    );
    let member_ids: Vec<String> = payload["member_run_ids"]
        .as_array()
        .expect("member_run_ids")
        .iter()
        .map(|id| id.as_str().expect("member id").to_string())
        .collect();
    assert_eq!(member_ids.len(), 2, "member ids: {payload}");
    let automatic_assignment = &payload["assignment_messages"][0];
    let assignment_id = automatic_assignment["id"]
        .as_str()
        .expect("automatic assignment id")
        .to_string();
    let assignment_correlation = automatic_assignment["correlation_id"]
        .as_str()
        .expect("automatic assignment correlation")
        .to_string();
    assert_eq!(
        payload["dashboard_url"].as_str(),
        Some(expected_dashboard.as_str())
    );

    // 5. team_run_status → both members + dashboard URL (+ the two queued
    //    assignment messages count as unacked).
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
    assert_eq!(members.len(), 2, "members: {payload}");
    for member in members {
        assert!(
            member["member_run"]["id"].is_string(),
            "member_run row: {member}"
        );
        assert!(member.get("latest_action").is_some(), "latest_action key");
    }
    assert_eq!(
        payload["pending_interactions"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(payload["unacked_messages"].as_u64(), Some(2));
    assert_eq!(
        payload["dashboard_url"].as_str(),
        Some(expected_dashboard.as_str())
    );

    // 6. team_run_send_message can immediately reuse the automatic Assignment
    // returned by team_run_create; the Host never needs a second fake anchor.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "from_member_id": member_ids[0],
                "to_member_ids": [member_ids[1]],
                "kind": "handoff",
                "body": "handing off the slice",
                "correlation_id": assignment_correlation,
                "causation_id": assignment_id
            }
        }),
    );
    let payload = call_payload(&response);
    let message_id = payload["message_id"]
        .as_str()
        .expect("message_id")
        .to_string();
    assert!(message_id.starts_with("tmsg-"), "message id: {message_id}");
    assert!(
        payload["correlation_id"].as_str().expect("correlation_id")
            == automatic_assignment["correlation_id"].as_str().unwrap(),
        "correlation id: {payload}"
    );

    // 7. team_run_events → strictly increasing seq, and the send above is
    //    journaled as a message/created event. after_seq resumes the tail.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_events",
            "arguments": {"team_run_id": team_run_id}
        }),
    );
    let payload = call_payload(&response);
    //    create journals 1 (run) + 2×2 (member + assignment) = 5 events,
    //    the handoff adds one more.
    let events = payload.as_array().expect("events array");
    assert!(events.len() >= 6, "events: {}", events.len());
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

    // 8. ACK refuses a message that has not actually been delivered.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_message_acknowledge",
            "arguments": {"message_id": assignment_id, "member_id": member_ids[0]}
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
    let store = HarnessStore::new(home.projects_dir().join(&project_id));
    let mut delivered_assignment = store
        .team_messages()
        .expect("team messages")
        .into_iter()
        .rev()
        .find(|message| message.id == assignment_id)
        .expect("assignment row");
    delivered_assignment.deliveries[0].status = TeamDeliveryStatus::Delivered;
    store
        .append_team_message(&delivered_assignment)
        .expect("mark assignment delivered");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_message_acknowledge",
            "arguments": {"message_id": assignment_id, "member_id": member_ids[0]}
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
            "name": "team_run_events",
            "arguments": {"team_run_id": team_run_id, "after_seq": last_seq}
        }),
    );
    let payload = call_payload(&response);
    assert!(payload
        .as_array()
        .expect("events array")
        .iter()
        .any(|event| {
            event["entity_id"].as_str() == Some(assignment_id.as_str())
                && event["operation"].as_str() == Some("updated")
                && event["summary"]
                    .as_str()
                    .is_some_and(|summary| summary.contains("acknowledged"))
        }));

    // 9. A planning run can be cancelled through MCP using the same guarded
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

    // 10. MCP start is asynchronous: it immediately returns the reserved
    // running projection and exact URL, then the provider completes in the
    // background while the same Host session remains responsive.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "wave_create",
            "arguments": {
                "id": "wave-mcp-start",
                "mission_id": "mission-mcp",
                "index": 3,
                "title": "Async start",
                "objective": "Prove non-blocking MCP start",
                "executor_kind": "agent_team"
            }
        }),
    );
    call_payload(&response);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Finish through fake Kimi ACP",
                "mission_id": "mission-mcp",
                "wave_id": "wave-mcp-start",
                "members": [{"name": "async-worker", "role": "implementer", "provider": "kimi"}]
            }
        }),
    );
    let startable = call_payload(&response);
    let startable_id = startable["team_run_id"]
        .as_str()
        .expect("startable team run id")
        .to_string();
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
            format!("http://127.0.0.1:5173/?api=.&surface=team&team={startable_id}&project={project_id}")
                .as_str()
        )
    );
    let mut terminal = None;
    for _ in 0..100 {
        let response = mcp.request(
            "tools/call",
            serde_json::json!({
                "name": "team_run_status",
                "arguments": {"team_run_id": startable_id}
            }),
        );
        let status = call_payload(&response);
        if status["team_run"]["status"].as_str() == Some("completed") {
            terminal = Some(status);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(terminal.is_some(), "MCP-started TeamRun did not complete");

    // Mission closeout is a separate Host decision after all Waves are
    // accepted; a host Wave needs no invented executor run.
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
            "name": "wave_create",
            "arguments": {
                "id": "wave-close",
                "mission_id": "mission-close",
                "title": "Host closeout slice",
                "objective": "Produce a direct outcome",
                "executor_kind": "host"
            }
        }),
    );
    call_payload(&response);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "wave_gate",
            "arguments": {"wave_id": "wave-close", "status": "accepted", "outcome": "host slice done"}
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

    // 12. Unknown method → JSON-RPC -32601; unknown tool → -32602; a failing
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
