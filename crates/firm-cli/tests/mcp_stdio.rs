//! Integration coverage for `harness mcp`: the binary is spawned as a stdio
//! MCP server against an isolated HOME and driven with line-delimited
//! JSON-RPC 2.0 — initialize handshake, tools/list, the Agent Team control
//! surface end to end (create → start/status → canonical question/reply →
//! events), and the -32601 unknown-method error.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

mod fake_provider;
mod firm_env;
use firm_env::{current_project_id, run_firm, run_firm_with_env, TempHome};
use harness_store::HarnessStore;

/// `harness init` a project rooted at `<base>/<name>` and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Seed the mandatory flat AgentTeam relation used by every AgentTeamRun:
/// one Mission, one durable Host Agent, and the local ExecutionNode.
fn seed_agent_team(home: &TempHome, project_root: &std::path::Path, suffix: &str) -> String {
    let project_id = current_project_id(home);
    let space_id = format!("mcp-space-{suffix}");
    let space = run_firm(
        home,
        project_root,
        &[
            "space",
            "init",
            "--id",
            &space_id,
            "--project-binding",
            &project_id,
        ],
    );
    assert!(space.status.success(), "space init failed: {space:?}");

    let host_id = format!("mcp-host-{suffix}");
    let create_host = serde_json::json!({
        "command": "create_agent_member",
        "member": {
            "id": host_id,
            "name": format!("MCP Host {suffix}"),
            "description": "durable host identity",
            "role": "host",
            "capabilities": ["coordination"],
            "skill_refs": [],
            "provider_profile_ref": "codex",
            "model_preference": null,
            "workspace_policy": "managed-worktree",
            "permission_ceiling": "workspace_write",
            "organization_status": "active",
            "version": 1,
            "created_by": {"kind": "human", "id": "test-operator"},
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }
    })
    .to_string();
    let host = run_firm(
        home,
        project_root,
        &[
            "member-trust",
            "mutate",
            "--actor-kind",
            "human",
            "--actor-id",
            "test-operator",
            "--idempotency-key",
            &format!("create-host-{suffix}"),
            "--expected-version",
            "0",
            "--json",
            &create_host,
        ],
    );
    assert!(host.status.success(), "host create failed: {host:?}");

    let mission = run_firm(
        home,
        project_root,
        &[
            "mission",
            "create",
            "--title",
            &format!("MCP Mission {suffix}"),
            "--objective",
            "Exercise the MCP AgentTeam contract",
        ],
    );
    assert!(
        mission.status.success(),
        "mission create failed: {mission:?}"
    );
    let mission_id = String::from_utf8_lossy(&mission.stdout).trim().to_string();

    seed_team_for_mission(home, project_root, &mission_id, &host_id, suffix, &[])
}

#[cfg(any())]
fn seed_canonical_member(
    home: &TempHome,
    project_root: &std::path::Path,
    project_id: &str,
    suffix: &str,
    role: &str,
) -> String {
    let space_id = format!("mcp-space-{suffix}");
    let space = run_firm(
        home,
        project_root,
        &[
            "space",
            "init",
            "--id",
            &space_id,
            "--project-binding",
            project_id,
        ],
    );
    assert!(space.status.success(), "space init failed: {space:?}");
    seed_member_in_active_space(home, project_root, suffix, role)
}

#[cfg(any())]
fn seed_member_in_active_space(
    home: &TempHome,
    project_root: &std::path::Path,
    suffix: &str,
    role: &str,
) -> String {
    seed_member_in_active_space_with_provider(home, project_root, suffix, role, "kimi")
}

fn seed_member_in_active_space_with_provider(
    home: &TempHome,
    project_root: &std::path::Path,
    suffix: &str,
    role: &str,
    provider: &str,
) -> String {
    let member_id = format!("mcp-member-{suffix}");
    let command = serde_json::json!({
        "command": "create_agent_member",
        "member": {
            "id": member_id,
            "name": format!("MCP AgentMember {suffix}"),
            "description": "canonical member identity for MCP integration coverage",
            "role": role,
            "capabilities": [role],
            "skill_refs": [],
            "provider_profile_ref": provider,
            "model_preference": null,
            "workspace_policy": "managed-worktree",
            "permission_ceiling": "workspace_write",
            "organization_status": "active",
            "version": 1,
            "created_by": {"kind": "human", "id": "test-operator"},
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }
    })
    .to_string();
    let created = run_firm(
        home,
        project_root,
        &[
            "member-trust",
            "mutate",
            "--actor-kind",
            "human",
            "--actor-id",
            "test-operator",
            "--idempotency-key",
            &format!("create-member-{suffix}"),
            "--expected-version",
            "0",
            "--json",
            &command,
        ],
    );
    assert!(
        created.status.success(),
        "member create failed: {created:?}"
    );
    member_id
}

fn seed_team_for_mission(
    home: &TempHome,
    project_root: &std::path::Path,
    mission_id: &str,
    host_id: &str,
    suffix: &str,
    additional_member_ids: &[&str],
) -> String {
    let node = run_firm(home, project_root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node init JSON");
    let node_id = node["id"].as_str().expect("node id");
    let project_id = current_project_id(home);
    let registration = run_firm(
        home,
        project_root,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    assert!(
        registration.status.success(),
        "node project register failed: {registration:?}"
    );

    let team = run_firm(
        home,
        project_root,
        &[
            "team",
            "create",
            "--name",
            &format!("MCP Team {suffix}"),
            "--description",
            "Flat test AgentTeam",
            "--mission-id",
            mission_id,
            "--host-agent-id",
            host_id,
            "--node-id",
            node_id,
            "--member",
            host_id,
        ],
    );
    assert!(team.status.success(), "team create failed: {team:?}");
    let team: serde_json::Value = serde_json::from_slice(&team.stdout).expect("team JSON");
    let team_id = team["id"].as_str().expect("team id").to_string();
    for member_id in additional_member_ids {
        let added = run_firm(
            home,
            project_root,
            &[
                "team",
                "add-member",
                "--id",
                &team_id,
                "--member",
                member_id,
            ],
        );
        assert!(added.status.success(), "team add-member failed: {added:?}");
    }
    team_id
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
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_firm"));
        cmd.arg("--project")
            .arg(project_id)
            .arg("mcp")
            .current_dir(home.base())
            .envs(home.envs())
            .env_remove("FIRM_ROOT")
            .env_remove("FIRM_PROJECT")
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
    #[cfg(any())]
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

#[test]
fn mcp_current_surface_is_mission_only_and_rejects_legacy_wave_tools() {
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
    for current in [
        "mission_create",
        "mission_update_context",
        "mission_close",
        "mission_list",
        "team_run_create",
    ] {
        assert!(
            names.contains(current),
            "missing current MCP tool {current}"
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

/// Assert a `tools/call` response IS an error (ADR 0051 retired Wave-write
/// tools answer 200-style with `isError: true`, not a JSON-RPC error) and
/// return its text content.
fn call_error_text(response: &serde_json::Value) -> String {
    let result = &response["result"];
    assert_eq!(
        result["isError"].as_bool(),
        Some(true),
        "tools/call unexpectedly succeeded: {response}"
    );
    result["content"][0]["text"]
        .as_str()
        .expect("text content block")
        .to_string()
}

fn directory_snapshot(root: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn visit(
        base: &std::path::Path,
        path: &std::path::Path,
        rows: &mut std::collections::BTreeMap<String, Vec<u8>>,
    ) {
        let mut entries = std::fs::read_dir(path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| panic!("enumerate {}: {error}", path.display()));
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let entry_path = entry.path();
            let file_type = entry
                .file_type()
                .unwrap_or_else(|error| panic!("stat {}: {error}", entry_path.display()));
            assert!(!file_type.is_symlink(), "test store contains a symlink");
            if file_type.is_dir() {
                visit(base, &entry_path, rows);
            } else if file_type.is_file() {
                rows.insert(
                    entry_path
                        .strip_prefix(base)
                        .expect("snapshot path under base")
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(&entry_path)
                        .unwrap_or_else(|error| panic!("read {}: {error}", entry_path.display())),
                );
            }
        }
    }

    let mut rows = std::collections::BTreeMap::new();
    visit(root, root, &mut rows);
    rows
}

#[test]
fn remote_fabric_mcp_surface_is_read_only_and_server_resolves_local_node() {
    let home = TempHome::new("mcp-remote-fabric-read");
    let project_id = init_project(&home, "mcp-remote-fabric-project");
    let project_root = home.base().join("mcp-remote-fabric-project");
    let node = run_firm(&home, &project_root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let listed = mcp.request("tools/list", serde_json::json!({}));
    let tools = listed["result"]["tools"].as_array().expect("tools array");
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "remote_fabric_status"));
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "remote_fabric_operation_show"));

    let status = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "remote_fabric_status",
            "arguments": {"company_id": "company-mcp-test"}
        }),
    ));
    assert_eq!(status["read_only"].as_bool(), Some(true));
    assert_eq!(status["company_id"], "company-mcp-test");
    assert!(status["local_node_id"].is_string());
    assert!(status["node_local"].is_null());
    assert!(status["control_plane"].is_null());

    let error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "remote_fabric_operation_show",
            "arguments": {"company_id": "company-mcp-test", "operation_id": "operation-1"}
        }),
    ));
    assert!(error.contains("Control Plane Store is unavailable"));
}

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

/// Seed one historical Wave row directly, bypassing the retired `wave_create`
/// MCP tool (ADR 0051), so tests can prove `source_plan_ref` navigation still
/// resolves a pre-cutover Wave row without exercising a live write.
#[cfg(any())]
fn seed_historical_wave(home: &TempHome, project_id: &str, id: &str, mission_id: &str, index: u64) {
    use std::io::Write as _;

    let path = home.spaces_dir().join(project_id).join("waves.jsonl");
    let mut ledger = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open wave ledger");
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": id,
            "mission_id": mission_id,
            "index": index,
            "title": "Historical Wave",
            "objective": "Seeded pre-cutover row for source_plan_ref coverage",
            "executor_kind": "agent_team",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
        })
    )
    .expect("append historical wave");
}

// Historical all-in-one MCP AgentTeam exercise. It includes the retired
// TeamRun message/reconcile authority and is retained as source-only migration
// evidence; current MCP reads and fail-closed retired writes have focused
// executable coverage below.
#[cfg(any())]
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
        start_descriptor.contains("codex_exec and claude_cli are workflow-only"),
        "descriptor must make the Team/Workflow boundary explicit: {start_descriptor}"
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

/// Declared `external_interactive` members are the one exception to the
/// unbound-MCP impersonation invariant: their user-driven session may author
/// its own ProviderRuntimeProjection mail, recorded with explicit provenance. Driven members
/// stay rejected from the same unbound connection.
// Retired unbound-MCP sender selection contract. External interactive
// sessions now author through their authenticated AgentSession/NodeDaemon.
#[cfg(any())]
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

// Historical Wave4A WorkDelivery claim/reconcile seam. Canonical
// WorkExecutionBinding + WorkDelivery owns the current runtime path.
#[cfg(any())]
#[test]
fn mcp_stdio_work_rebind_and_successor_delivery_reconcile() {
    let home = TempHome::new("mcp-work-rebind-reconcile");
    let project_id = init_project(&home, "mcp-work-control-proj");
    let project_root = std::fs::canonicalize(home.base().join("mcp-work-control-proj"))
        .expect("canonical project root");
    let stable_agent_id =
        seed_canonical_member(&home, &project_root, &project_id, "rebind", "implementer");
    let mission = run_firm(
        &home,
        &project_root,
        &[
            "mission",
            "create",
            "--title",
            "Rebind",
            "--objective",
            "Exercise recovery",
        ],
    );
    assert!(
        mission.status.success(),
        "mission create failed: {mission:?}"
    );
    let mission_id = String::from_utf8_lossy(&mission.stdout).trim().to_string();
    let team_id = seed_team_for_mission(
        &home,
        &project_root,
        &mission_id,
        &stable_agent_id,
        "rebind",
        &[],
    );

    // This is the successful rebind path, so pin the provider probe to the
    // reviewed fake Kimi version instead of inheriting a developer machine's
    // potentially review_required installation.
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let mut mcp = McpClient::spawn(&home, &project_id, &[("KIMI_CODE_BIN", fake_kimi.as_str())]);
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Exercise MCP Work lifecycle recovery",
                "agent_team_id": team_id,
                "execution_root": project_root,
                "members": [{
                    "name": "stable-worker",
                    "role": "implementer",
                    "provider": "kimi",
                    "agent_member_id": stable_agent_id,
                    "initial_work": "Preserve durable ownership across runtime replacement."
                }]
            }
        }),
    );
    let created = call_payload(&response);
    let team_run_id = created["team_run_id"]
        .as_str()
        .expect("team run id")
        .to_string();
    let old_member_id = created["member_run_ids"][0]
        .as_str()
        .expect("old member id")
        .to_string();
    let work_id = created["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    let initial_version = created["works"][0]["version"]
        .as_u64()
        .expect("initial Work version");

    // Assign is deliberately not a reassignment primitive. An already-owned
    // Work must move to another runtime only through rebind.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_assign",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": work_id,
                "member_run_id": old_member_id,
                "expected_version": initial_version
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .expect("assign conflict")
        .contains("must be open to assign"));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_deactivate_member",
            "arguments": {
                "team_run_id": team_run_id,
                "member_run_id": old_member_id,
                "reason": "replace crashed runtime generation"
            }
        }),
    );
    call_payload(&response);

    // Runtime replacement is normally produced by the lifecycle controller.
    // The focused MCP test materializes that prerequisite directly, then proves
    // the public rebind tool preserves the stable AgentMember owner.
    let store = HarnessStore::new(home.spaces_dir().join("mcp-space-rebind"));
    let old_member = store
        .member_runs()
        .expect("member runs")
        .into_iter()
        .rev()
        .find(|member| member.id == old_member_id)
        .expect("deactivated member");
    let mut replacement = old_member.clone();
    replacement.id = "member-mcp-stable-worker-generation-2".to_string();
    replacement.coordination_status = MemberCoordinationStatus::Active;
    replacement.runtime_generation += 1;
    replacement.status = MemberRunStatus::Idle;
    replacement.native_session = None;
    replacement.started_at = "unix-ms:mcp-replacement".to_string();
    replacement.last_event_at = None;
    replacement.finished_at = None;
    let run = store
        .team_runs()
        .expect("team runs")
        .into_iter()
        .rev()
        .find(|run| run.id == team_run_id)
        .expect("TeamRun");
    let mut next_run = run.clone();
    next_run.member_run_ids.push(replacement.id.clone());
    store
        .admit_member_run(&run, &next_run, &replacement)
        .expect("atomically admit replacement ProviderRuntimeProjection");

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_rebind",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": work_id,
                "member_run_id": replacement.id,
                "expected_version": initial_version
            }
        }),
    );
    let rebound = call_payload(&response);
    assert_eq!(
        rebound["owner_member_id"].as_str(),
        created["works"][0]["owner_member_id"].as_str()
    );
    assert_eq!(
        rebound["active_member_run_id"].as_str(),
        Some(replacement.id.as_str())
    );
    let rebound_version = rebound["version"].as_u64().expect("rebound version");
    assert_eq!(rebound_version, initial_version + 1);

    let delivery = store
        .latest_work_deliveries()
        .expect("latest WorkDeliveries")
        .into_iter()
        .find(|delivery| {
            delivery.work_id == work_id
                && delivery.work_version == rebound_version
                && delivery.recipient_member_run_id == replacement.id
        })
        .expect("replacement ProviderWorkDispatch");
    assert_eq!(delivery.status, ProviderWorkDispatchStatus::Queued);
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64;
    let node_daemon = store
        .acquire_node_daemon_lease(
            &run.execution_node_id,
            "daemon-mcp",
            "instance-mcp",
            now_unix_ms,
            60_000,
        )
        .expect("parent NodeDaemon lease");
    let first = store
        .acquire_team_supervisor_under_node_lease(
            &team_run_id,
            &run.execution_node_id,
            &node_daemon.daemon_id,
            node_daemon.generation,
            &project_id,
            &run.project_binding_id,
            "supervisor-mcp-generation-1",
            11,
            "mcp:test:first",
            now_unix_ms,
            10,
        )
        .expect("first Supervisor lease");
    let claimed = match store
        .claim_work_delivery(
            &team_run_id,
            &delivery.id,
            &replacement.id,
            &first.supervisor_id,
            first.generation,
            "claim-mcp-work-generation-1",
            now_unix_ms + 1,
            "unix-ms:mcp-claim",
        )
        .expect("claim replacement delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("replacement delivery must be queued"),
    };
    assert_eq!(claimed.status, ProviderWorkDispatchStatus::Claimed);
    let successor = store
        .acquire_team_supervisor_under_node_lease(
            &team_run_id,
            &run.execution_node_id,
            &node_daemon.daemon_id,
            node_daemon.generation,
            &project_id,
            &run.project_binding_id,
            "supervisor-mcp-generation-2",
            22,
            "mcp:test:successor",
            now_unix_ms + 11,
            60_000,
        )
        .expect("successor Supervisor lease");
    assert_eq!(successor.generation, first.generation + 1);

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_reconcile_delivery",
            "arguments": {
                "team_run_id": team_run_id,
                "delivery_id": delivery.id,
                "supervisor_id": successor.supervisor_id,
                "supervisor_generation": successor.generation
            }
        }),
    );
    let reconciled = call_payload(&response);
    assert_eq!(reconciled["status"].as_str(), Some("queued"));
    assert!(reconciled["claim_id"].is_null());
    assert!(reconciled["claimed_by_supervisor_id"].is_null());
    assert!(reconciled["claimed_generation"].is_null());
}

/// Decision-shaped board reads over MCP (issue #305): `team_run_work_list`'s
/// `brief`/`since` params and the new `team_run_board_summary` tool mirror
/// the CLI behavior exercised in depth by `tests/team_run_api.rs`; this test
/// only proves the MCP wiring itself -- argument parsing, dispatch, and
/// response shape -- for each of the three projections.
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
}
