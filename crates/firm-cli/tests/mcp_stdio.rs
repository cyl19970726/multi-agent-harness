//! Integration coverage for `harness mcp`: the binary is spawned as a stdio
//! MCP server against an isolated HOME and driven with line-delimited
//! JSON-RPC 2.0 — initialize handshake, tools/list, the Agent Team control
//! surface end to end (create → start/status → route/reconcile → send/ACK →
//! events), and the -32601 unknown-method error.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use harness_core::{
    MemberCoordinationStatus, MemberRunStatus, NativeSessionAvailability, NativeSessionRef,
    ProviderInteractionMessageOption, ProviderInteractionRequestBody, ProviderInteractionType,
    TeamActorKind, TeamActorRef, TeamDeliveryPolicy, TeamDeliveryStatus, TeamMessage,
    TeamMessageDelivery, TeamMessageKind, TeamMessageResponseIntent, TeamRecipientKind,
    TeamRecipientRef, WorkCommandContext, WorkDeliveryStatus,
};
use harness_store::{HarnessStore, WorkDeliveryClaimResult};

mod fake_provider;
mod firm_env;
use firm_env::{current_project_id, run_firm, run_firm_with_env, TempHome};

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

    let host = run_firm(
        home,
        project_root,
        &[
            "agent",
            "create",
            "--name",
            &format!("mcp-host-{suffix}"),
            "--role",
            "host",
            "--provider",
            "codex",
        ],
    );
    assert!(host.status.success(), "host create failed: {host:?}");
    let host: serde_json::Value = serde_json::from_slice(&host.stdout).expect("host JSON");
    let host_id = host["id"].as_str().expect("host id");

    seed_team_for_mission(home, project_root, &mission_id, host_id, suffix)
}

fn seed_team_for_mission(
    home: &TempHome,
    project_root: &std::path::Path,
    mission_id: &str,
    host_id: &str,
    suffix: &str,
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
    team["id"].as_str().expect("team id").to_string()
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
fn mcp_stdio_host_only_typed_work_review_boundary() {
    let home = TempHome::new("mcp-host-work-review");
    let project_id = init_project(&home, "mcp-host-work-review-proj");
    let project_root = home.base().join("mcp-host-work-review-proj");
    let team_id = seed_agent_team(&home, &project_root, "host-review");
    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let response = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "mcp-review-test", "version": "0"}
        }),
    );
    assert!(response["result"]["capabilities"]["tools"].is_object());
    mcp.notify("notifications/initialized");

    let listed = mcp.request("tools/list", serde_json::json!({}));
    let review_tool = listed["result"]["tools"]
        .as_array()
        .expect("tools list")
        .iter()
        .find(|tool| tool["name"] == "team_run_work_review")
        .expect("typed Work review tool");
    let schema = &review_tool["inputSchema"];
    assert_eq!(schema["additionalProperties"].as_bool(), Some(false));
    for forbidden in [
        "reviewer_agent_id",
        "review_strategy",
        "actor_id",
        "performed_by_actor",
        "authority_actor",
    ] {
        assert!(
            schema["properties"].get(forbidden).is_none(),
            "MCP review schema exposes spoofable field {forbidden}: {schema}"
        );
    }
    assert_eq!(schema["properties"]["expected_version"]["minimum"], 1);
    assert!(schema["required"]
        .as_array()
        .expect("review required fields")
        .iter()
        .any(|field| field == "review_id"));
    assert!(schema["properties"].get("residual_risk").is_some());
    let create_tool = listed["result"]["tools"]
        .as_array()
        .expect("tools list")
        .iter()
        .find(|tool| tool["name"] == "team_run_work_create")
        .expect("typed Work create tool");
    assert_eq!(
        create_tool["inputSchema"]["additionalProperties"].as_bool(),
        Some(false)
    );
    assert!(create_tool["inputSchema"]["properties"]
        .get("gates")
        .is_some());
    let accept_tool = listed["result"]["tools"]
        .as_array()
        .expect("tools list")
        .iter()
        .find(|tool| tool["name"] == "team_run_work_accept")
        .expect("typed Work accept tool");
    assert_eq!(
        accept_tool["inputSchema"]["additionalProperties"].as_bool(),
        Some(false)
    );
    assert!(accept_tool["inputSchema"]["properties"]
        .get("summary")
        .is_none());

    let created = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Exercise typed host-only MCP review",
                "agent_team_id": team_id,
                "members": [{
                    "name": "owner",
                    "role": "implementer",
                    "provider": "manual",
                    "execution_mode": "external_interactive"
                }]
            }
        }),
    ));
    let team_run_id = created["team_run_id"]
        .as_str()
        .expect("TeamRun id")
        .to_string();
    let member_run_id = created["member_run_ids"][0]
        .as_str()
        .expect("MemberRun id")
        .to_string();
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let mut sequence = 0u64;

    let create_spoof = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_create",
            "arguments": {
                "team_run_id": team_run_id,
                "title": "Spoofed creator",
                "completion_criteria_markdown": "must never be persisted",
                "actor_id": "operator:spoof"
            }
        }),
    ));
    assert!(
        create_spoof.contains("unknown argument `actor_id`"),
        "{create_spoof}"
    );
    let invalid_gate = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_create",
            "arguments": {
                "team_run_id": team_run_id,
                "title": "Invalid typed gate",
                "completion_criteria_markdown": "must never be persisted",
                "gates": [{"plugin": "code-review", "config": {"strategy": "host", "unexpected": true}}]
            }
        }),
    ));
    assert!(invalid_gate.contains("unknown field"), "{invalid_gate}");

    let mut seed_work = |mcp: &mut McpClient, strategy: &str, suffix: &str, submit: bool| {
        sequence += 1;
        let config = if strategy == "peer" {
            serde_json::json!({"strategy": strategy, "reviewer": "not-the-owner"})
        } else {
            serde_json::json!({"strategy": strategy})
        };
        let value = call_payload(&mcp.request(
            "tools/call",
            serde_json::json!({
                "name": "team_run_work_create",
                "arguments": {
                    "team_run_id": team_run_id,
                    "id": format!("mcp-review-{suffix}"),
                    "title": format!("Review candidate {suffix}"),
                    "completion_criteria_markdown": "Exercise review boundary",
                    "owner_member_run_id": member_run_id,
                    "claim_mode": "host_assign",
                    "gates": [{"plugin": "code-review", "config": config}]
                }
            }),
        ));
        let work: harness_core::Work = serde_json::from_value(value).expect("decode created Work");
        if !submit {
            return work;
        }
        let context = |label: &str| WorkCommandContext {
            event_id: format!("event-{suffix}-{label}-{sequence}"),
            performed_by_actor: TeamActorRef {
                kind: TeamActorKind::MemberRun,
                id: member_run_id.clone(),
                display_name: None,
                authn_source: Some("mcp-review-test".to_string()),
            },
            authority_actor: None,
            causation_ref: None,
            idempotency_key: format!("command-{suffix}-{label}-{sequence}"),
            created_at: format!("unix-ms:{}", 100 + sequence),
            duplicate_ok: false,
        };
        let claimed = store
            .start_work(&work.id, work.version, &member_run_id, context("start"))
            .expect("start review candidate");
        store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member_run_id,
                "candidate ready for review",
                Vec::new(),
                Vec::new(),
                context("submit"),
            )
            .expect("submit review candidate")
    };

    let peer = seed_work(&mut mcp, "peer", "peer", true);
    let peer_error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {"team_run_id": team_run_id, "work_id": peer.id, "expected_version": peer.version, "review_id": "review-peer", "verdict": "pass", "summary": "must reject peer"}
        }),
    ));
    assert!(
        peer_error.contains("REQUIRES_HOST_STRATEGY"),
        "{peer_error}"
    );

    let self_review = seed_work(&mut mcp, "self", "self", true);
    let self_error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {"team_run_id": team_run_id, "work_id": self_review.id, "expected_version": self_review.version, "review_id": "review-self", "verdict": "pass", "summary": "must reject self review"}
        }),
    ));
    assert!(
        self_error.contains("REQUIRES_HOST_STRATEGY"),
        "{self_error}"
    );

    let wrong_state = seed_work(&mut mcp, "host", "wrong-state", false);
    let state_error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {"team_run_id": team_run_id, "work_id": wrong_state.id, "expected_version": wrong_state.version, "review_id": "review-wrong-state", "verdict": "pass", "summary": "not submitted"}
        }),
    ));
    assert!(state_error.contains("not awaiting review"), "{state_error}");

    let host = seed_work(&mut mcp, "host", "host", true);
    assert_eq!(host.created_by_actor.id, "service:mcp");
    assert_eq!(
        host.created_by_actor.authn_source.as_deref(),
        Some("local_mcp_stdio")
    );
    let version_error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {"team_run_id": team_run_id, "work_id": host.id, "expected_version": host.version + 1, "review_id": "review-wrong-version", "verdict": "pass", "summary": "wrong version"}
        }),
    ));
    assert!(
        version_error.contains("VERSION_CONFLICT"),
        "{version_error}"
    );

    let reviews_before_spoof = store.reviews().expect("reviews before spoof");
    let spoof_error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {"team_run_id": team_run_id, "work_id": host.id, "expected_version": host.version, "review_id": "review-spoof", "verdict": "pass", "summary": "spoof", "authority_actor": {"kind": "member_run", "id": member_run_id}}
        }),
    ));
    assert!(
        spoof_error.contains("unknown argument `authority_actor`"),
        "{spoof_error}"
    );
    assert_eq!(
        store.reviews().expect("reviews after spoof"),
        reviews_before_spoof
    );
    let actor_spoof_error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {"team_run_id": team_run_id, "work_id": host.id, "expected_version": host.version, "review_id": "review-actor-spoof", "verdict": "pass", "summary": "spoof", "actor_id": "operator:spoof"}
        }),
    ));
    assert!(
        actor_spoof_error.contains("unknown argument `actor_id`"),
        "{actor_spoof_error}"
    );

    let review = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": host.id,
                "expected_version": host.version,
                "review_id": "review-host-stable",
                "verdict": "pass",
                "summary": "host-approved through local MCP",
                "blockers": [],
                "residual_risk": "manual acceptance remains explicit",
                "missing_validation": [],
                "evidence_ids": ["evidence:mcp-review"]
            }
        }),
    ));
    assert_eq!(review["reviewer_agent_id"].as_str(), Some("host"));
    assert_eq!(review["review_strategy"].as_str(), Some("host"));
    assert_eq!(
        review["performed_by_actor"]["kind"].as_str(),
        Some("service")
    );
    assert_eq!(
        review["performed_by_actor"]["id"].as_str(),
        Some("service:mcp")
    );
    assert_eq!(review["authority_actor"]["kind"].as_str(), Some("host"));
    assert_eq!(review["authority_actor"]["id"].as_str(), Some("host"));
    assert_eq!(
        review["command_idempotency_key"].as_str(),
        Some("mcp-work-review:review-host-stable")
    );
    assert_eq!(
        review["residual_risk"].as_str(),
        Some("manual acceptance remains explicit")
    );

    let review_count = store.reviews().expect("reviews after first call").len();
    let retry = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": host.id,
                "expected_version": host.version,
                "review_id": "review-host-stable",
                "verdict": "pass",
                "summary": "host-approved through local MCP",
                "blockers": [],
                "residual_risk": "manual acceptance remains explicit",
                "missing_validation": [],
                "evidence_ids": ["evidence:mcp-review"]
            }
        }),
    ));
    assert_eq!(retry, review);
    assert_eq!(
        store.reviews().expect("reviews after retry").len(),
        review_count
    );

    let conflict = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_review",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": host.id,
                "expected_version": host.version,
                "review_id": "review-host-stable",
                "verdict": "fail",
                "summary": "different payload"
            }
        }),
    ));
    assert!(conflict.contains("IDEMPOTENCY_CONFLICT"), "{conflict}");

    let accept_spoof = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_accept",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": host.id,
                "expected_version": host.version,
                "actor_id": "operator:spoof"
            }
        }),
    ));
    assert!(
        accept_spoof.contains("unknown argument `actor_id`"),
        "{accept_spoof}"
    );

    let accepted = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_accept",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": host.id,
                "expected_version": host.version,
                "idempotency_key": "accept-host-reviewed"
            }
        }),
    ));
    assert_eq!(accepted["phase"].as_str(), Some("closed"));
    assert_eq!(accepted["condition"].as_str(), Some("normal"));
    assert_eq!(accepted["resolution"].as_str(), Some("accepted"));
    let accepted_operation = store
        .work_operations()
        .expect("Work operations")
        .into_iter()
        .find(|operation| operation.event.idempotency_key == "accept-host-reviewed")
        .expect("MCP accept operation");
    assert_eq!(
        accepted_operation.event.performed_by_actor.id,
        "service:mcp"
    );
    assert_eq!(
        accepted_operation
            .event
            .performed_by_actor
            .authn_source
            .as_deref(),
        Some("local_mcp_stdio")
    );
    let accepted_retry = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_work_accept",
            "arguments": {
                "team_run_id": team_run_id,
                "work_id": host.id,
                "expected_version": host.version,
                "idempotency_key": "accept-host-reviewed"
            }
        }),
    ));
    assert_eq!(accepted_retry, accepted);
    assert_eq!(
        store
            .work_operations()
            .expect("Work operations after accept retry")
            .into_iter()
            .filter(|operation| operation.event.idempotency_key == "accept-host-reviewed")
            .count(),
        1
    );
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

#[test]
fn mcp_resolves_provider_request_messages_and_keeps_legacy_ledger_empty() {
    let home = TempHome::new("mcp-provider-interaction-message");
    let project_id = init_project(&home, "mcp-provider-interaction");
    let project_root = home.base().join("mcp-provider-interaction");
    let team_id = seed_agent_team(&home, &project_root, "provider-interaction");
    let created = run_firm(
        &home,
        &project_root,
        &[
            "team-run",
            "create",
            "--agent-team-id",
            &team_id,
            "--objective",
            "Exercise MCP provider response bridge",
            "--member",
            "worker:implementer:codex#Wait for MCP answer",
        ],
    );
    assert!(
        created.status.success(),
        "create TeamRun: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let run_id = String::from_utf8_lossy(&created.stdout).trim().to_string();
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let initial = store
        .member_runs()
        .expect("member runs")
        .into_iter()
        .rev()
        .find(|member| member.team_run_id == run_id)
        .expect("created member");
    let mut member = initial.clone();
    member.status = MemberRunStatus::Running;
    member.native_session = Some(NativeSessionRef {
        provider: member.provider.clone(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "mcp-native-session".into(),
        native_locator_kind: "codex_thread".into(),
        provider_version: None,
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: None,
        parent_native_session_id: None,
    });
    store
        .compare_and_append_member_run(&initial, &member)
        .expect("seed native member");
    let request_body = ProviderInteractionRequestBody {
        interaction_type: ProviderInteractionType::Question,
        prompt: "Choose one".into(),
        options: vec![ProviderInteractionMessageOption {
            id: "choice-a".into(),
            label: "Choice A".into(),
            intent: Some("answer".into()),
        }],
        provider: member.provider.clone(),
        provider_request_id: "mcp-reverse-1".into(),
        method: "item/tool/requestUserInput".into(),
        session: "mcp-native-session".into(),
        member: member.id.clone(),
        generation: member.runtime_generation,
    };
    let created_at = "unix-ms:mcp-provider-request".to_string();
    let request = TeamMessage {
        id: "tmsg-mcp-provider-request".into(),
        team_run_id: run_id.clone(),
        work_id: None,
        origin_wave_id: None,
        sender: Some(TeamActorRef {
            kind: TeamActorKind::MemberRun,
            id: member.id.clone(),
            display_name: None,
            authn_source: Some("provider_reverse_request_test".into()),
        }),
        from_member_id: member.id.clone(),
        recipients: vec![TeamRecipientRef {
            kind: TeamRecipientKind::Host,
            id: "host".into(),
        }],
        to_member_ids: vec!["host".into()],
        kind: TeamMessageKind::ProviderInteractionRequest,
        body: request_body.to_canonical_json().expect("canonical request"),
        correlation_id: request_body.correlation_id(),
        causation_id: None,
        response_intent: Some(TeamMessageResponseIntent::ResponseRequired),
        evidence_refs: Vec::new(),
        deliveries: vec![TeamMessageDelivery {
            member_id: "host".into(),
            policy: TeamDeliveryPolicy::ManualAck,
            status: TeamDeliveryStatus::Delivered,
            attempt: 1,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: Some("mcp-reverse-request-receipt".into()),
            failure_reason: None,
            updated_at: created_at.clone(),
        }],
        created_at,
    };
    store
        .append_team_message_checked(&request)
        .expect("append provider request");

    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let _ = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "provider-bridge-test", "version": "0"}
        }),
    );
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_resolve_interaction",
            "arguments": {
                "team_run_id": run_id,
                "interaction_id": request.id,
                "option_id": "choice-a",
                "resolved_by": "host"
            }
        }),
    );
    let payload = call_payload(&response);
    assert_eq!(
        payload["kind"].as_str(),
        Some("provider_interaction_response")
    );
    assert!(store
        .pending_interactions()
        .expect("legacy pending interactions")
        .is_empty());
    let messages = store.team_messages().expect("team messages");
    assert!(messages.iter().any(|message| {
        message.kind == TeamMessageKind::ProviderInteractionResponse
            && message.causation_id.as_deref() == Some("tmsg-mcp-provider-request")
    }));
}

/// Seed one historical Wave row directly, bypassing the retired `wave_create`
/// MCP tool (ADR 0051), so tests can prove `origin_wave_id` navigation still
/// resolves a pre-cutover Wave row without exercising a live write.
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
            "objective": "Seeded pre-cutover row for origin_wave_id coverage",
            "executor_kind": "agent_team",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
        })
    )
    .expect("append historical wave");
}

#[test]
fn mcp_stdio_agent_team_tools() {
    let home = TempHome::new("mcp-stdio");
    let project_id = init_project(&home, "mcp-proj");
    let project_root =
        std::fs::canonicalize(home.base().join("mcp-proj")).expect("canonical project root");
    let stable_agent = run_firm(
        &home,
        &project_root,
        &[
            "agent",
            "create",
            "--name",
            "stable-lead",
            "--role",
            "coordinator",
            "--provider",
            "kimi",
        ],
    );
    assert!(
        stable_agent.status.success(),
        "create stable Agent failed: {}",
        String::from_utf8_lossy(&stable_agent.stderr)
    );
    let stable_agent: serde_json::Value =
        serde_json::from_slice(&stable_agent.stdout).expect("stable Agent JSON");
    let stable_agent_id = stable_agent["id"]
        .as_str()
        .expect("stable Agent id")
        .to_string();
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
            "mission_update_context",
            "mission_close",
            "mission_list",
            "wave_create",
            "wave_update",
            "wave_advance",
            "wave_list",
            "wave_gate",
            "team_run_create",
            "team_run_work_list",
            "team_run_work_show",
            "team_run_work_create",
            "team_run_work_review",
            "team_run_work_assign",
            "team_run_work_rebind",
            "team_run_work_block",
            "team_run_work_resume",
            "team_run_work_release",
            "team_run_work_request_changes",
            "team_run_work_accept",
            "team_run_work_cancel",
            "team_run_work_reconcile_delivery",
            "work_delegation_create",
            "work_delegation_list",
            "work_delegation_show",
            "work_delegation_cancel",
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
            "agent_route_inbox",
            "team_run_resolve_interaction",
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
    // HTTP) supplies the outer identity for the TeamRun. Wave creation is
    // retired (ADR 0051): the MCP tool now answers isError:true instead of
    // authoring a row; a historical Wave is seeded directly (never through
    // `wave_create`) purely so the origin_wave_id navigation check below has
    // a real pre-cutover Wave id to cite.
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
    );
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
    let wave_create_error = call_error_text(&response);
    assert!(
        wave_create_error.contains("retired") && wave_create_error.contains("mission log append"),
        "wave_create error: {wave_create_error}"
    );
    seed_historical_wave(&home, &project_id, "wave-mcp", "mission-mcp", 2);

    // 4. team_run_create with two members → run id + member run ids. Mission
    // and navigation Wave are derived through the required flat AgentTeam.
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
                    {"name": "worker-1", "role": "implementer", "provider": "codex", "model": "gpt-5", "worktree_ref": project_root, "owned_paths": ["crates/a", "docs"], "initial_work": "Implement the requested slice and pass checks."}
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
        "http://127.0.0.1:5173/?api=.&surface=team&team={team_run_id}&space={project_id}&project={project_id}&mission=mission-mcp&wave=wave-mcp"
    );
    assert!(team_run_id.starts_with("team-run-"), "id: {team_run_id}");
    assert_eq!(payload["mission_id"].as_str(), Some("mission-mcp"));
    assert!(payload["wave_id"].is_null());
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
    // A Mission-scoped long-lived TeamRun has no runtime-owned Wave id, but
    // its fresh Dashboard URL still carries the Host's current Wave as
    // navigation context.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_create",
            "arguments": {
                "objective": "Mission-scoped cold-link proof",
                "agent_team_id": team_id,
                "members": [
                    {"name": "cold-link", "role": "observer", "provider": "codex"}
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
            format!("http://127.0.0.1:5173/?api=.&surface=team&team={mission_scoped_id}&space={project_id}&project={project_id}&mission=mission-mcp&wave=wave-mcp")
                .as_str()
        )
    );

    // 5. The thin MCP adapter can extend the same run and records the
    // origin Wave only as Host-plan provenance.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_add_member",
            "arguments": {
                "team_run_id": team_run_id,
                "origin_wave_id": "wave-mcp",
                "initial_work": "repair the interaction path",
                "member": {
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
    assert_eq!(
        payload["pending_interactions"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(payload["unacked_messages"].as_u64(), Some(0));
    assert_eq!(
        payload["dashboard_url"].as_str(),
        Some(expected_dashboard.as_str())
    );

    // 7. Stable Agent Inbox mail is routed atomically into its one eligible
    // MemberRun without changing the source identity or inventing a second
    // runtime.
    let stable_message = run_firm(
        &home,
        &project_root,
        &[
            "agent",
            "send",
            "--to",
            &stable_agent_id,
            "--from",
            "external-reviewer",
            "--content",
            "Please include the native receipt in the review.",
        ],
    );
    assert!(
        stable_message.status.success(),
        "queue stable Agent mail failed: {}",
        String::from_utf8_lossy(&stable_message.stderr)
    );
    let stable_message: serde_json::Value =
        serde_json::from_slice(&stable_message.stdout).expect("stable message JSON");
    let stable_message_id = stable_message["id"].as_str().expect("stable message id");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "agent_route_inbox",
            "arguments": {
                "agent_member_id": stable_agent_id,
                "message_id": stable_message_id
            }
        }),
    );
    let routed = call_payload(&response);
    assert_eq!(routed["routes"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        routed["routes"][0]["member_run_id"].as_str(),
        Some(member_ids[0].as_str())
    );
    assert_eq!(
        routed["routes"][0]["agent_message_id"].as_str(),
        Some(stable_message_id)
    );

    // 8. An unbound MCP connection cannot impersonate a MemberRun. The same
    // tool remains the Host/operator/service send path and can immediately
    // create an ordinary Work-linked conversation correlation.
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "from_member_id": member_ids[0],
                "sender_kind": "member_run",
                "to_member_ids": [member_ids[1]],
                "kind": "handoff",
                "body": "attempted member impersonation",
                "work_id": initial_work_id.clone()
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(true));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .expect("impersonation error")
        .contains("unbound MCP connections may not author"));

    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "team_run_send_message",
            "arguments": {
                "team_run_id": team_run_id,
                "from_member_id": "host",
                "to_member_ids": [member_ids[1]],
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
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let mut claimed_message = store
        .team_messages()
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
        .append_team_message(&TeamMessage {
            id: host_message.clone(),
            team_run_id: team_run_id.clone(),
            work_id: Some(initial_work_id.clone()),
            origin_wave_id: None,
            sender: Some(TeamActorRef {
                kind: TeamActorKind::MemberRun,
                id: member_ids[0].clone(),
                display_name: Some("Provider-bound member".to_string()),
                authn_source: Some("provider_runtime_test".to_string()),
            }),
            from_member_id: member_ids[0].clone(),
            recipients: vec![TeamRecipientRef {
                kind: TeamRecipientKind::Host,
                id: "host".to_string(),
            }],
            to_member_ids: vec!["host".to_string()],
            kind: TeamMessageKind::Message,
            body: "QUESTION: choose interface A or B".to_string(),
            correlation_id: coordination_correlation.clone(),
            causation_id: Some(message_id.clone()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
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
        .team_messages()
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
                "members": [{"name": "async-worker", "role": "implementer", "provider": "kimi"}]
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
            format!("http://127.0.0.1:5173/?api=.&surface=team&team={startable_id}&space={project_id}&project={project_id}&mission=mission-mcp&wave=wave-mcp")
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

    // Mission closeout is a separate Host decision; it no longer requires
    // any Wave gate (ADR 0051) -- close succeeds directly on its own
    // outcome, and wave_create/wave_gate both answer isError:true.
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
    let wave_create_error = call_error_text(&response);
    assert!(wave_create_error.contains("retired"), "{wave_create_error}");
    let response = mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "wave_gate",
            "arguments": {"wave_id": "wave-close", "status": "accepted", "outcome": "host slice done"}
        }),
    );
    let wave_gate_error = call_error_text(&response);
    assert!(wave_gate_error.contains("retired"), "{wave_gate_error}");
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
    assert_eq!(closed["wave_ids"], serde_json::json!([]));

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
/// its own MemberRun mail, recorded with explicit provenance. Driven members
/// stay rejected from the same unbound connection.
#[test]
fn mcp_stdio_external_interactive_member_authorship() {
    let home = TempHome::new("mcp-stdio-external");
    let project_id = init_project(&home, "mcp-proj");
    let project_root =
        std::fs::canonicalize(home.base().join("mcp-proj")).expect("canonical project root");
    let team_id = seed_agent_team(&home, &project_root, "external");
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
                    {"name": "lead", "role": "coordinator", "provider": "kimi"},
                    {"name": "ext-reviewer", "role": "reviewer", "provider": "kimi", "execution_mode": "external_interactive", "initial_work": "Review the proposed change and report evidence."}
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
                "from_member_id": "host",
                "to_member_ids": [member_ids[1]],
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
                "from_member_id": member_ids[1],
                "sender_kind": "member_run",
                "to_member_ids": ["host"],
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
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let reply = store
        .team_messages()
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
                "from_member_id": member_ids[0],
                "sender_kind": "member_run",
                "to_member_ids": [member_ids[1]],
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
    // external process cannot author MemberRun mail until explicit Reopen.
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
                "from_member_id": member_ids[1],
                "sender_kind": "member_run",
                "to_member_ids": ["host"],
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
                "from_member_id": member_ids[1],
                "sender_kind": "member_run",
                "to_member_ids": ["host"],
                "kind": "message",
                "body": "authoring resumes after explicit reopen",
                "correlation_id": conversation_correlation
            }
        }),
    );
    assert_eq!(response["result"]["isError"].as_bool(), Some(false));
}

#[test]
fn mcp_stdio_work_rebind_and_successor_delivery_reconcile() {
    let home = TempHome::new("mcp-work-rebind-reconcile");
    let project_id = init_project(&home, "mcp-work-control-proj");
    let project_root = std::fs::canonicalize(home.base().join("mcp-work-control-proj"))
        .expect("canonical project root");
    let stable_agent = run_firm(
        &home,
        &project_root,
        &[
            "agent",
            "create",
            "--name",
            "stable-worker",
            "--role",
            "implementer",
            "--provider",
            "kimi",
        ],
    );
    assert!(
        stable_agent.status.success(),
        "create stable Agent failed: {}",
        String::from_utf8_lossy(&stable_agent.stderr)
    );
    let stable_agent: serde_json::Value =
        serde_json::from_slice(&stable_agent.stdout).expect("stable Agent JSON");
    let stable_agent_id = stable_agent["id"]
        .as_str()
        .expect("stable Agent id")
        .to_string();
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
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
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
        .expect("atomically admit replacement MemberRun");

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
        .expect("replacement WorkDelivery");
    assert_eq!(delivery.status, WorkDeliveryStatus::Queued);
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
    assert_eq!(claimed.status, WorkDeliveryStatus::Claimed);
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
