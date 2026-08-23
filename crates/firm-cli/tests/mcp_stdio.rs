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
/// one durable Host AgentMember and the local ExecutionNode. No Mission is
/// created — post-DEV-35 Teams never require one and DOC-108 retired the
/// Mission writers.
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
            "workspace_policy": "trusted-development-explicit-cwd",
            "permission_ceiling": "full_access",
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

    seed_team_without_mission(home, project_root, &host_id, suffix, &[])
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
            "workspace_policy": "trusted-development-explicit-cwd",
            "permission_ceiling": "full_access",
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

/// Historical helper for the source-only `mcp_stdio_agent_team_tools`
/// migration-evidence test: seeds a Team WITH legacy Mission provenance.
/// Live tests use `seed_team_without_mission`; DOC-108 retired the Mission
/// writers this flow depended on.
#[cfg(any())]
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

/// Seed a durable flat AgentTeam with NO Mission relation (the post-DEV-35
/// default): local ExecutionNode, durable Host AgentMember, no
/// `--mission-id`/`--legacy-mission-id` on `team create`.
fn seed_team_without_mission(
    home: &TempHome,
    project_root: &std::path::Path,
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
            &format!("MCP Missionless Team {suffix}"),
            "--description",
            "Flat test AgentTeam without Mission provenance",
            "--host-agent-id",
            host_id,
            "--node-id",
            node_id,
            "--member",
            host_id,
        ],
    );
    assert!(
        team.status.success(),
        "mission-less team create failed: {team:?}"
    );
    let team: serde_json::Value = serde_json::from_slice(&team.stdout).expect("team JSON");
    assert!(
        team["legacy_mission_id"].is_null(),
        "mission-less team must not gain Mission provenance: {team}"
    );
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

/// Simulate a hostile recovery/import artifact: the physical Store contains a
/// second Execution Space with a Message carrying the same TeamRun id. Current
/// MCP projections must resolve the TeamRun's frozen canonical scope first and
/// must never fold this foreign row merely because the logical id collides.
fn inject_foreign_space_copy_of_message(
    store_root: &std::path::Path,
    source_execution_space_id: &str,
    foreign_execution_space_id: &str,
    message_id: &str,
    _exclusive_store_guard: &harness_store::StoreExclusiveMigrationGuard,
) {
    let ledger = store_root.join("agentfirm_trust_operations.jsonl");
    let source = std::fs::read_to_string(&ledger).expect("read canonical operation ledger");
    let mut envelope = source
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|row| {
            row["execution_space_id"] == source_execution_space_id
                && row["operation"]["event"]["aggregate_kind"] == "message"
                && row["operation"]["event"]["aggregate_id"] == message_id
        })
        .expect("source Message operation");
    envelope["execution_space_id"] = serde_json::json!(foreign_execution_space_id);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .expect("open canonical operation ledger for hostile fixture");
    writeln!(file, "{envelope}").expect("append hostile foreign-space fixture");
    file.flush().expect("flush hostile foreign-space fixture");
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

#[path = "mcp_stdio/mcp_answers_canonical_provider_request_with_transport_identity_and_exact_retry.rs"]
mod mcp_answers_canonical_provider_request_with_transport_identity_and_exact_retry;
#[path = "mcp_stdio/mcp_current_surface_is_team_tools_with_retired_mission_and_wave_tombstones.rs"]
mod mcp_current_surface_is_team_tools_with_retired_mission_and_wave_tombstones;
#[path = "mcp_stdio/mcp_stdio_work_list_brief_since_and_board_summary.rs"]
mod mcp_stdio_work_list_brief_since_and_board_summary;
#[path = "mcp_stdio/mcp_team_run_create_without_legacy_mission_omits_mission_context.rs"]
mod mcp_team_run_create_without_legacy_mission_omits_mission_context;
#[path = "mcp_stdio/remote_fabric_mcp_surface_is_read_only_and_server_resolves_local_node.rs"]
mod remote_fabric_mcp_surface_is_read_only_and_server_resolves_local_node;
#[path = "mcp_stdio/removed_mcp_team_run_message_writer_fails_unknown_with_zero_store_delta.rs"]
mod removed_mcp_team_run_message_writer_fails_unknown_with_zero_store_delta;
#[path = "mcp_stdio/retired_mcp_standalone_member_run_create_is_unadvertised_and_byte_zero.rs"]
mod retired_mcp_standalone_member_run_create_is_unadvertised_and_byte_zero;

// Historical all-in-one MCP AgentTeam exercise retained as source-only migration evidence.
#[cfg(any())]
#[path = "mcp_stdio/mcp_stdio_agent_team_tools.rs"]
mod mcp_stdio_agent_team_tools;

// Retired unbound-MCP sender selection contract retained as source-only migration evidence.
#[cfg(any())]
#[path = "mcp_stdio/mcp_stdio_external_interactive_member_authorship.rs"]
mod mcp_stdio_external_interactive_member_authorship;
