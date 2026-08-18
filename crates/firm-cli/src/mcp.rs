//! `harness mcp` — stdio MCP server exposing Agent Team v0 as MCP tools.
//!
//! The host CLI (Kimi Code / Claude Code / Codex) spawns this process and
//! speaks the MCP stdio transport: line-delimited JSON-RPC 2.0, one request
//! per stdin line, one response per stdout line. stdout carries ONLY protocol
//! frames — every diagnostic goes to stderr (the store resolver's deprecation
//! warnings included), so the wire is never corrupted.
//!
//! Protocol surface (the minimum a host needs):
//! - `initialize` → protocolVersion / capabilities / serverInfo handshake.
//! - `notifications/initialized` (and any other notification) → no response.
//! - `ping` → `{}`.
//! - `tools/list` → Agent Team tools plus the read-only legacy Mission list.
//!   Mission writer tools are removed entirely (DOC-108): callers get the
//!   same unknown-tool tombstone as retired Wave tools, with zero store delta.
//! - `tools/call` → `{content:[{type:"text",text:<result JSON>}], isError}`.
//! - unknown method → JSON-RPC -32601. stdin EOF exits.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};

use harness_core::{
    AgentTeamRun, TeamActorKind, TeamActorRef, TeamRunEvent, TeamRunStatus,
    TeamSupervisorLeaseStatus, Work, WorkCausationRef, WorkClaimMode, WorkCommandContext,
    WorkCondition, WorkPhase, WorkPriority,
};
use harness_store::HarnessStore;
use serde_json::{json, Value};

use crate::{
    add_team_run_member, agentfirm_api, answer_provider_message_value, close_team_member_value,
    create_team_run, current_unix_ms_u64, deactivate_team_run_member,
    delegate_team_run_to_node_daemon, format_work_brief_line, generated_id,
    host_inbox_for_native_thread, interrupt_team_member_value, latest_member_runs_in_append_order,
    latest_team_run, latest_team_runs_in_append_order, mutate_team_work_value, now_string,
    reconcile_team_work_delivery_value, rename_team_run_member, reopen_team_member_value,
    reopened_member_requires_supervisor_start, serde_snake_label, steer_team_member_value,
    team_member_specs_from_definition, team_run_board_summary_text, team_run_inbox,
    team_run_mission_id, transition_team_run, visible_member_actions_in_append_order,
    work_operation_cursors, ResolvedStore, TeamMemberSpec,
};

/// MCP protocol revision this server speaks, echoed verbatim in `initialize`
/// (the simple end of "reply with the client's version or the lower one").
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Closed command inventory for the generic Member Trust MCP adapter.
/// MemberRun creation is intentionally absent: current creation is admitted
/// only by `team_run_create` or `team_run_add_member`, which publish the legacy
/// runtime projection and canonical MemberRun through one Store boundary.
const MCP_MEMBER_TRUST_COMMANDS: &[&str] = &[
    "create_agent_member",
    "pause_agent_member",
    "resume_agent_member",
    "retire_agent_member",
    "close_member_run",
    "reopen_member_run",
    "retire_member_run",
    "resume_native_session",
    "create_work_deliveries",
    "retry_work_delivery",
    "reconcile_work_delivery",
    "provision_workspace",
    "transition_workspace",
    "create_work_report",
    "create_work_finding",
    "create_failure_analysis",
    "bind_work_module",
    "create_gate_requirement",
    "accept_work",
    "evaluate_gate",
    "waive_gate",
    "revoke_gate_waiver",
];

/// The Vite Dashboard is the human UI. Its development proxy exposes the
/// Harness API at the same origin, so deep links must not point at the
/// API-only `harness serve` root on port 8787.
const DASHBOARD_UI_ORIGIN: &str = "http://127.0.0.1:5173";
const DASHBOARD_SAME_ORIGIN_API_BASE: &str = ".";

fn team_dashboard_url(store: &HarnessStore, resolved: &ResolvedStore, team_run_id: &str) -> String {
    let run = latest_team_run(store, team_run_id).ok();
    // Legacy Mission provenance is optional: mission-less Teams produce no
    // `&mission=` selector instead of an empty one.
    let mission_id = run
        .as_ref()
        .and_then(|run| team_run_mission_id(store, run).ok().flatten());
    let context = mission_id
        .as_deref()
        .map(|mission_id| format!("&mission={mission_id}"))
        .unwrap_or_default();
    let mut selectors = String::new();
    if let Some(space) = resolved.execution_space_context.as_ref() {
        selectors.push_str("&space=");
        selectors.push_str(&space.id);
    }
    if let Some(project) = resolved.context.as_ref() {
        selectors.push_str("&project=");
        selectors.push_str(&project.id);
    }
    let base = format!(
        "{DASHBOARD_UI_ORIGIN}/?api={DASHBOARD_SAME_ORIGIN_API_BASE}&surface=team&team={team_run_id}{selectors}"
    );
    base + &context
}

/// Serve the stdio MCP loop until stdin closes.
pub fn run(store: &HarnessStore, resolved: &ResolvedStore) -> crate::CliResult<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = handle_line(store, resolved, trimmed) {
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Handle one JSON-RPC line. Returns `None` for notifications (including
/// `notifications/initialized`): they are accepted and otherwise ignored.
fn handle_line(store: &HarnessStore, resolved: &ResolvedStore, line: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {"code": -32700, "message": format!("parse error: {error}")},
            }));
        }
    };
    // A request without an `id` is a notification: never answered.
    let id = request.get("id")?.clone();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "harness", "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => call_tool(store, resolved, &params),
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
        Err((code, message)) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
        }
    })
}

/// Dispatch one `tools/call`. Unknown tool names and malformed call params
/// are JSON-RPC errors; a tool that runs and fails answers 200-style with
/// `isError: true` so the host model sees the failure text as tool output.
pub(crate) fn call_tool(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    params: &Value,
) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
        (
            -32602,
            "tools/call params.name must be a string".to_string(),
        )
    })?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let outcome = match name {
        "agentfirm_member_trust_mutate" => {
            tool_agentfirm_member_trust_mutate(store, resolved, &arguments)
        }
        // DOC-108: Mission writer tools are REMOVED from the MCP surface
        // (unknown tool, byte-zero store delta), matching the Wave tombstone
        // convention. `mission_list` stays as the read-only legacy read.
        "mission_list" => tool_mission_list(store),
        "team_run_create" => tool_team_run_create(store, resolved, &arguments),
        "team_run_add_member" => tool_team_run_add_member(store, resolved, &arguments),
        "team_run_work_list" => tool_team_run_work_list(store, &arguments),
        "team_run_work_show" => tool_team_run_work_show(store, &arguments),
        "team_run_work_create" => tool_team_run_work_create(store, &arguments),
        "team_run_work_assign" => tool_team_run_work_mutate(store, &arguments, "assign"),
        "team_run_work_rebind" => tool_team_run_work_mutate(store, &arguments, "rebind"),
        "team_run_work_block" => tool_team_run_work_mutate(store, &arguments, "block"),
        "team_run_work_resume" => tool_team_run_work_mutate(store, &arguments, "resume"),
        "team_run_work_release" => tool_team_run_work_mutate(store, &arguments, "release"),
        "team_run_work_request_changes" => {
            tool_team_run_work_mutate(store, &arguments, "request-changes")
        }
        "team_run_work_cancel" => tool_team_run_work_mutate(store, &arguments, "cancel"),
        "team_run_work_reconcile_delivery" => {
            tool_team_run_work_reconcile_delivery(store, &arguments)
        }
        "collaboration_delegation_list" => tool_collaboration_delegation_list(resolved, &arguments),
        "collaboration_delegation_show" => tool_collaboration_delegation_show(resolved, &arguments),
        "execution_node_list" => tool_execution_node_list(store, &arguments),
        "execution_node_show" => tool_execution_node_show(store, &arguments),
        "remote_fabric_status" => tool_remote_fabric_status(resolved, &arguments),
        "remote_fabric_operation_show" => tool_remote_fabric_operation_show(resolved, &arguments),
        "team_run_rename_member" => tool_team_run_rename_member(store, &arguments),
        "team_run_deactivate_member" => tool_team_run_deactivate_member(store, &arguments),
        "team_run_start" => tool_team_run_start(store, resolved, &arguments),
        "team_run_cancel" => tool_team_run_cancel(store, resolved, &arguments),
        "team_run_list" => tool_team_run_list(store, &arguments),
        "team_run_status" => tool_team_run_status(store, resolved, &arguments),
        "team_run_board_summary" => tool_team_run_board_summary(store, &arguments),
        "team_run_host_inbox" => tool_team_run_host_inbox(store, &arguments),
        "team_run_inbox" => tool_team_run_inbox(store, &arguments),
        "team_inbox_list" => tool_team_inbox_list(store, resolved, &arguments),
        "team_run_answer_message" => tool_team_run_answer_message(store, &arguments),
        "team_run_steer_member" => tool_team_run_steer_member(store, &arguments),
        "team_run_interrupt_member" => tool_team_run_interrupt_member(store, &arguments),
        "team_run_close_member" => tool_team_run_close_member(store, &arguments),
        "team_run_reopen_member" => tool_team_run_reopen_member(store, resolved, &arguments),
        "team_run_events" => tool_team_run_events(store, &arguments),
        _ => return Err((-32602, format!("unknown tool: {name}"))),
    };
    let (text, is_error) = match outcome {
        Ok(payload) => (payload.to_string(), false),
        Err(message) => (message, true),
    };
    Ok(json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    }))
}

fn tool_agentfirm_member_trust_mutate(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    reject_unknown_arguments(
        arguments,
        "agentfirm_member_trust_mutate",
        &["command", "idempotency_key", "expected_version"],
    )?;
    let command_value = arguments
        .get("command")
        .cloned()
        .ok_or_else(|| "argument `command` is required".to_string())?;
    let command_name = command_value
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| "argument `command.command` must be a string".to_string())?;
    if !MCP_MEMBER_TRUST_COMMANDS.contains(&command_name) {
        return Err("unsupported or retired MCP Member Trust command".to_string());
    }
    let execution_space_id = resolved
        .execution_space_context
        .as_ref()
        .map(|space| space.id.clone())
        .ok_or_else(|| {
            "member trust MCP mutations require an explicit Execution Space".to_string()
        })?;
    let actor_kind_raw = std::env::var("AGENTFIRM_MCP_ACTOR_KIND")
        .map_err(|_| "MCP transport is missing AGENTFIRM_MCP_ACTOR_KIND".to_string())?;
    let actor_id = std::env::var("AGENTFIRM_MCP_ACTOR_ID")
        .map_err(|_| "MCP transport is missing AGENTFIRM_MCP_ACTOR_ID".to_string())?;
    let actor_kind = agentfirm_api::parse_actor_kind(&actor_kind_raw)
        .ok_or_else(|| "AGENTFIRM_MCP_ACTOR_KIND is invalid".to_string())?;
    let authority_actor = match (
        std::env::var("AGENTFIRM_MCP_AUTHORITY_KIND").ok(),
        std::env::var("AGENTFIRM_MCP_AUTHORITY_ID").ok(),
    ) {
        (None, None) => None,
        (Some(kind), Some(id)) => Some(harness_core::agentfirm_api::ActorRef {
            kind: agentfirm_api::parse_actor_kind(&kind)
                .ok_or_else(|| "AGENTFIRM_MCP_AUTHORITY_KIND is invalid".to_string())?,
            id,
        }),
        _ => return Err("MCP authority kind and id must be configured together".to_string()),
    };
    let command = serde_json::from_value::<agentfirm_api::TrustCommand>(command_value)
        .map_err(|error| format!("invalid TrustCommand: {error}"))?;
    let expected_version = arguments
        .get("expected_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "argument `expected_version` must be an unsigned integer".to_string())?;
    let auth = agentfirm_api::AuthenticatedMutation {
        execution_space_id,
        actor: harness_core::agentfirm_api::ActorRef {
            kind: actor_kind,
            id: actor_id,
        },
        authorized_authority_actors: authority_actor.into_iter().collect(),
        idempotency_key: required_non_empty_str(arguments, "idempotency_key")?.to_string(),
        expected_version,
        request_fingerprint: None,
    };
    agentfirm_api::execute(store, auth, command)
        .map(|result| json!(result))
        .map_err(|error| error.to_string())
}

fn tool_remote_fabric_status(resolved: &ResolvedStore, arguments: &Value) -> Result<Value, String> {
    reject_unknown_arguments(arguments, "remote_fabric_status", &["company_id"])?;
    let company_id = required_non_empty_str(arguments, "company_id")?.to_string();
    let home =
        crate::fabric_runtime::firm_home(resolved, &[]).map_err(|error| error.to_string())?;
    let layout = harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(&home)
        .map_err(|error| error.to_string())?;
    let node_id = crate::read_local_node_id().map_err(|error| error.to_string())?;
    let local_root = layout
        .node_local_root(&company_id, &node_id)
        .map_err(|error| error.to_string())?;
    let local = if local_root.exists() {
        let store = layout
            .open_node_local(&company_id, &node_id)
            .map_err(|error| error.to_string())?;
        let snapshot = store.snapshot().map_err(|error| error.to_string())?;
        let queued_outbox = snapshot
            .outboxes
            .values()
            .filter(|outbox| {
                !matches!(
                    outbox.local_state,
                    harness_fabric::LocalOutboxState::Terminal
                )
            })
            .count();
        let recovery_required = snapshot
            .inboxes
            .values()
            .filter(|inbox| inbox.state == harness_fabric::LocalInboxState::RecoveryRequired)
            .map(|inbox| inbox.operation_id.clone())
            .collect::<Vec<_>>();
        Some(json!({
            "store_revision": snapshot.revision,
            "gateway_session": snapshot.active_session,
            "outbox_depth": queued_outbox,
            "inbox_depth": snapshot.inboxes.len(),
            "recovery_required_operation_ids": recovery_required,
        }))
    } else {
        None
    };
    let control_root = layout
        .control_plane_root(&company_id)
        .map_err(|error| error.to_string())?;
    let control_plane = if control_root.exists() {
        let store = layout
            .open_control_plane(&company_id)
            .map_err(|error| error.to_string())?;
        let snapshot = store.snapshot().map_err(|error| error.to_string())?;
        if snapshot.authority_company_id.as_deref() != Some(company_id.as_str()) {
            return Err("Remote Fabric Store is not bound to the requested Company".into());
        }
        let diagnostics =
            harness_fabric::diagnostics::inspect_fabric(&store, &company_id, current_unix_ms_u64())
                .map_err(|error| error.to_string())?;
        Some(json!({
            "store_revision": snapshot.revision,
            "control_plane_lease": snapshot.control_plane_leases.get(&company_id),
            "nodes": snapshot.nodes.values().collect::<Vec<_>>(),
            "diagnostics": diagnostics,
        }))
    } else {
        None
    };
    Ok(json!({
        "company_id": company_id,
        "local_node_id": node_id,
        "node_local": local,
        "control_plane": control_plane,
        "read_only": true,
    }))
}

fn tool_remote_fabric_operation_show(
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    reject_unknown_arguments(
        arguments,
        "remote_fabric_operation_show",
        &["company_id", "operation_id"],
    )?;
    let company_id = required_non_empty_str(arguments, "company_id")?.to_string();
    let operation_id = required_non_empty_str(arguments, "operation_id")?.to_string();
    let home =
        crate::fabric_runtime::firm_home(resolved, &[]).map_err(|error| error.to_string())?;
    let layout = harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(&home)
        .map_err(|error| error.to_string())?;
    let root = layout
        .control_plane_root(&company_id)
        .map_err(|error| error.to_string())?;
    if !root.exists() {
        return Err("Remote Fabric Control Plane Store is unavailable on this machine".into());
    }
    let store = layout
        .open_control_plane(&company_id)
        .map_err(|error| error.to_string())?;
    let snapshot = store.snapshot().map_err(|error| error.to_string())?;
    if snapshot.authority_company_id.as_deref() != Some(company_id.as_str()) {
        return Err("Remote Fabric Store is not bound to the requested Company".into());
    }
    let operation = snapshot
        .operations
        .get(&operation_id)
        .ok_or_else(|| "Remote Fabric operation does not exist".to_string())?;
    let attempts = snapshot
        .attempts
        .values()
        .filter(|attempt| attempt.operation_id == operation_id)
        .collect::<Vec<_>>();
    let receipts = snapshot
        .receipts
        .values()
        .filter(|receipt| receipt.operation_id == operation_id)
        .collect::<Vec<_>>();
    Ok(json!({
        "operation": operation,
        "attempts": attempts,
        "receipts": receipts,
        "read_only": true,
    }))
}

/// `team_run_work_list` -- mirrors `harness team-run work list`, including
/// its decision-shaped `brief`/`since` projections (issue #305). `since` is a
/// WorkOperation-order cursor (see `work_operation_cursors`); a call that
/// passes it gets back `next_since` so a Host loop can chain delta reads
/// without re-deriving the cursor itself. `brief` returns pre-formatted
/// `works_brief` lines (`format_work_brief_line`) instead of full Work JSON.
fn tool_team_run_work_list(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let brief = arguments
        .get("brief")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let since = match arguments.get("since") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or_else(|| {
            "argument `since` must be a non-negative integer WorkOperation cursor".to_string()
        })?),
    };
    let cursors = match since {
        Some(_) => {
            Some(work_operation_cursors(store, team_run_id).map_err(|error| error.to_string())?)
        }
        None => None,
    };
    let mut works = store
        .latest_works()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|work| work.team_run_id == team_run_id)
        .filter(|work| {
            since.is_none_or(|cursor| {
                cursors
                    .as_ref()
                    .and_then(|cursors| cursors.get(&work.id))
                    .is_some_and(|sequence| *sequence > cursor)
            })
        })
        .collect::<Vec<_>>();
    works.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });
    if brief {
        let works_brief: Vec<String> = works.iter().map(format_work_brief_line).collect();
        return Ok(json!({"works_brief": works_brief}));
    }
    if let Some(since) = since {
        let next_since = cursors
            .as_ref()
            .and_then(|cursors| cursors.values().copied().max())
            .unwrap_or(0)
            .max(since);
        return Ok(json!({"since": since, "next_since": next_since, "works": works}));
    }
    Ok(json!({"works": works}))
}

/// `team_run_board_summary` -- mirrors `harness team-run board-summary`: a
/// single bounded plain-text digest (issue #305), returned under `summary`
/// rather than as the raw MCP text payload so the shape matches every other
/// tool result here.
fn tool_team_run_board_summary(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    require_current_team_run(store, team_run_id)?;
    let summary =
        team_run_board_summary_text(store, team_run_id).map_err(|error| error.to_string())?;
    Ok(json!({"summary": summary}))
}

fn tool_team_run_work_show(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let work_id = required_str(arguments, "work_id")?;
    let work = store
        .latest_works()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|work| work.team_run_id == team_run_id && work.id == work_id)
        .ok_or_else(|| format!("Work not found: {work_id}"))?;
    let events = store
        .work_events()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|event| event.team_run_id == team_run_id && event.work_id == work_id)
        .collect::<Vec<_>>();
    let deliveries = store
        .latest_work_deliveries()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|delivery| delivery.team_run_id == team_run_id && delivery.work_id == work_id)
        .collect::<Vec<_>>();
    Ok(json!({"work": work, "events": events, "deliveries": deliveries}))
}

fn tool_team_run_work_create(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    const ALLOWED: &[&str] = &[
        "team_run_id",
        "id",
        "title",
        "context_markdown",
        "completion_criteria_markdown",
        "owner_member_run_id",
        "claim_mode",
        "eligible_member_ids",
        "parent_work_id",
        "prerequisite_work_ids",
        "priority",
        "caused_by_message_id",
        "idempotency_key",
    ];
    reject_unknown_arguments(arguments, "team_run_work_create", ALLOWED)?;
    let team_run_id = required_non_empty_str(arguments, "team_run_id")?;
    let owner_member_run_id = optional_non_empty_str(arguments, "owner_member_run_id")?;
    let claim_mode = match optional_non_empty_str(arguments, "claim_mode")?.as_deref() {
        None if owner_member_run_id.is_some() => WorkClaimMode::HostAssign,
        None => WorkClaimMode::TeamClaim,
        Some("host_assign") => WorkClaimMode::HostAssign,
        Some("team_claim") => WorkClaimMode::TeamClaim,
        Some(value) => return Err(format!("invalid claim_mode `{value}`")),
    };
    let priority = match optional_non_empty_str(arguments, "priority")?.as_deref() {
        None | Some("normal") => WorkPriority::Normal,
        Some("low") => WorkPriority::Low,
        Some("high") => WorkPriority::High,
        Some("urgent") => WorkPriority::Urgent,
        Some(value) => return Err(format!("invalid priority `{value}`")),
    };
    optional_non_empty_str(arguments, "caused_by_message_id")?;
    optional_non_empty_str(arguments, "idempotency_key")?;
    let context = local_mcp_host_work_context(arguments);
    let work = Work {
        id: optional_non_empty_str(arguments, "id")?.unwrap_or_else(|| generated_id("work")),
        team_run_id: team_run_id.to_string(),
        accountable_team_id: None,
        assignee_membership_id: None,
        created_by_member_id: None,
        parent_work_id: optional_non_empty_str(arguments, "parent_work_id")?,
        title: required_non_empty_str(arguments, "title")?.to_string(),
        context_markdown: optional_str(arguments, "context_markdown")?.unwrap_or_default(),
        completion_criteria_markdown: required_non_empty_str(
            arguments,
            "completion_criteria_markdown",
        )?
        .to_string(),
        phase: WorkPhase::Open,
        condition: WorkCondition::Normal,
        resolution: None,
        owner_member_id: None,
        active_member_run_id: owner_member_run_id,
        claim_mode,
        eligible_member_ids: optional_string_array(arguments, "eligible_member_ids")?,
        prerequisite_work_ids: optional_string_array(arguments, "prerequisite_work_ids")?,
        priority,
        created_by_actor: context.performed_by_actor.clone(),
        result_summary: None,
        blocker_reason: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    };
    store
        .insert_work(work, context)
        .map(|work| json!(work))
        .map_err(|error| error.to_string())
}

/// Record a Work-bound Review through the local MCP Host boundary. Unlike the
/// retired HTTP route, this is a typed tool: caller-controlled identity and
/// authority fields are not part of its schema and unknown arguments fail.
fn reject_unknown_arguments(arguments: &Value, tool: &str, allowed: &[&str]) -> Result<(), String> {
    let object = arguments
        .as_object()
        .ok_or_else(|| format!("{tool} arguments must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!(
            "unknown argument `{key}` for {tool}; actor identity is fixed by the local MCP boundary"
        ));
    }
    Ok(())
}

fn optional_non_empty_str(arguments: &Value, key: &str) -> Result<Option<String>, String> {
    let value = optional_str(arguments, key)?;
    if value
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        Err(format!("argument `{key}` must not be empty"))
    } else {
        Ok(value)
    }
}

fn local_mcp_host_work_context(arguments: &Value) -> WorkCommandContext {
    WorkCommandContext {
        event_id: generated_id("work-event"),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Service,
            id: "service:mcp".to_string(),
            display_name: Some("Harness MCP".to_string()),
            authn_source: Some("local_mcp_stdio".to_string()),
        },
        authority_actor: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: "host".to_string(),
            display_name: None,
            authn_source: Some("local_mcp_host_authority".to_string()),
        }),
        causation_ref: arguments
            .get("caused_by_message_id")
            .and_then(Value::as_str)
            .map(|id| WorkCausationRef {
                kind: "team_message".to_string(),
                id: id.to_string(),
            }),
        idempotency_key: arguments
            .get("idempotency_key")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| generated_id("work-command")),
        created_at: now_string(),
        duplicate_ok: false,
    }
}

fn required_non_empty_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    let value = required_str(arguments, key)?;
    if value.trim().is_empty() {
        Err(format!("argument `{key}` must not be empty"))
    } else {
        Ok(value)
    }
}

fn optional_string_array(arguments: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = arguments.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("argument `{key}` must be an array of non-empty strings"))?;
    values
        .iter()
        .map(|value| {
            let text = value
                .as_str()
                .ok_or_else(|| format!("argument `{key}` must contain only strings"))?;
            if text.trim().is_empty() {
                Err(format!("argument `{key}` must not contain empty strings"))
            } else {
                Ok(text.to_string())
            }
        })
        .collect()
}

fn tool_team_run_work_mutate(
    store: &HarnessStore,
    arguments: &Value,
    operation: &str,
) -> Result<Value, String> {
    mutate_team_work_value(
        store,
        required_str(arguments, "team_run_id")?,
        required_str(arguments, "work_id")?,
        operation,
        arguments,
    )
    .map_err(|error| error.to_string())
}

fn tool_team_run_work_reconcile_delivery(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    reconcile_team_work_delivery_value(store, required_str(arguments, "team_run_id")?, arguments)
        .map_err(|error| error.to_string())
}

fn collaboration_store(company_id: &str) -> Result<HarnessStore, String> {
    let firm_home = crate::execution_space::firm_home().map_err(|error| error.to_string())?;
    let layout = harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(&firm_home)
        .map_err(|error| error.to_string())?;
    let root = layout
        .collaboration_root(company_id)
        .map_err(|error| error.to_string())?;
    Ok(HarnessStore::new(root))
}

fn resolved_mcp_collaboration_scope(
    resolved: &ResolvedStore,
) -> Result<(String, crate::AgentFirmHttpCredential), String> {
    let company_id = resolved
        .execution_space_context
        .as_ref()
        .and_then(|space| space.company_id.clone())
        .ok_or_else(|| {
            "collaboration MCP reads require a selected Execution Space bound to one Company"
                .to_string()
        })?;
    let token = std::env::var("AGENTFIRM_MCP_CREDENTIAL_TOKEN").map_err(|_| {
        "collaboration MCP reads require server-configured AGENTFIRM_MCP_CREDENTIAL_TOKEN"
            .to_string()
    })?;
    let credential = crate::resolve_agentfirm_http_credential(Some(&token))?;
    Ok((company_id, credential))
}

fn mcp_actor_can_read_delegation(
    store: &HarnessStore,
    company_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    delegation: &harness_core::collaboration::WorkDelegationV1,
) -> Result<bool, String> {
    let source_host = store
        .collaboration_source_work_attestation(company_id, &delegation.source_work_attestation_id)
        .map_err(|error| error.to_string())?
        .map(|attestation| attestation.source_host_ref);
    Ok(actor == &delegation.source_owner_ref
        || source_host.as_ref() == Some(actor)
        || actor == &delegation.target_host_ref)
}

fn tool_collaboration_delegation_list(
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    const ALLOWED: &[&str] = &[
        "source_team_id",
        "target_team_id",
        "node_id",
        "state",
        "limit",
        "cursor",
    ];
    reject_unknown_arguments(arguments, "collaboration_delegation_list", ALLOWED)?;
    let (company_id, credential) = resolved_mcp_collaboration_scope(resolved)?;
    let state = optional_non_empty_str(arguments, "state")?
        .map(|value| serde_json::from_value(json!(value)).map_err(|error| error.to_string()))
        .transpose()?;
    let store = collaboration_store(&company_id)?;
    let filter = harness_store::CollaborationDelegationFilter {
        source_team_id: optional_non_empty_str(arguments, "source_team_id")?,
        target_team_id: optional_non_empty_str(arguments, "target_team_id")?,
        node_id: optional_non_empty_str(arguments, "node_id")?,
        state,
    };
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(100) as usize;
    let secret = std::env::var("AGENTFIRM_MCP_CREDENTIAL_TOKEN")
        .map_err(|_| "collaboration cursor requires server credential".to_string())?;
    let cursor = optional_non_empty_str(arguments, "cursor")?
        .map(|value| {
            super::fabric_runtime::decode_collaboration_cursor(&value, &secret)
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    let page = store
        .list_collaboration_delegations_for_actor(
            &company_id,
            &credential.actor,
            &filter,
            cursor,
            limit,
        )
        .map_err(|error| error.to_string())?;
    let next_cursor = page
        .next_cursor
        .as_ref()
        .map(|value| {
            super::fabric_runtime::encode_collaboration_cursor(value, &secret)
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    Ok(json!({
        "items": page.items,
        "as_of_store_sequence": page.as_of_store_sequence,
        "next_cursor": next_cursor,
    }))
}

fn tool_collaboration_delegation_show(
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    reject_unknown_arguments(
        arguments,
        "collaboration_delegation_show",
        &["delegation_id"],
    )?;
    let (company_id, credential) = resolved_mcp_collaboration_scope(resolved)?;
    let delegation_id = required_non_empty_str(arguments, "delegation_id")?;
    let store = collaboration_store(&company_id)?;
    let delegation = store
        .collaboration_delegation(&company_id, delegation_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("collaboration Delegation not found: {delegation_id}"))?;
    if !mcp_actor_can_read_delegation(&store, &company_id, &credential.actor, &delegation)? {
        return Err(
            "UNAUTHORIZED_ACTOR: Delegation is outside the authenticated MCP actor scope".into(),
        );
    }
    Ok(json!({
        "delegation": delegation,
        "cancellation_requests": store
            .collaboration_cancellation_requests(&company_id, delegation_id)
            .map_err(|error| error.to_string())?,
        "publications": store
            .collaboration_publications(&company_id, delegation_id)
            .map_err(|error| error.to_string())?,
    }))
}

fn tool_execution_node_list(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    reject_unknown_arguments(arguments, "execution_node_list", &[])?;
    Ok(json!({
        "nodes": store.latest_execution_nodes().map_err(|error| error.to_string())?,
        "registrations": store.latest_node_project_registrations().map_err(|error| error.to_string())?,
        "daemon_leases": store.latest_node_daemon_leases().map_err(|error| error.to_string())?,
    }))
}

fn tool_execution_node_show(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    reject_unknown_arguments(arguments, "execution_node_show", &["node_id"])?;
    let node_id = required_str(arguments, "node_id")?;
    let node = store
        .latest_execution_nodes()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| format!("ExecutionNode not found: {node_id}"))?;
    let registrations = store
        .latest_node_project_registrations()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|registration| registration.node_id == node_id)
        .collect::<Vec<_>>();
    let daemon_lease = store
        .latest_node_daemon_lease(node_id)
        .map_err(|error| error.to_string())?;
    Ok(json!({"node": node, "registrations": registrations, "daemon_lease": daemon_lease}))
}

fn tool_team_run_steer_member(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let member_run_id = required_str(arguments, "member_run_id")?;
    steer_team_member_value(store, team_run_id, member_run_id, arguments)
        .map_err(|error| error.to_string())
}

fn tool_team_run_interrupt_member(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let member_run_id = required_str(arguments, "member_run_id")?;
    interrupt_team_member_value(store, team_run_id, member_run_id, arguments)
        .map_err(|error| error.to_string())
}

fn tool_team_run_close_member(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let member_run_id = required_str(arguments, "member_run_id")?;
    close_team_member_value(store, team_run_id, member_run_id, arguments)
        .map_err(|error| error.to_string())
}

fn tool_team_run_reopen_member(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let member_run_id = required_str(arguments, "member_run_id")?;
    let reopened = reopen_team_member_value(store, team_run_id, member_run_id, arguments)
        .map_err(|error| error.to_string())?;
    if reopened_member_requires_supervisor_start(store, team_run_id, member_run_id)
        .map_err(|error| error.to_string())?
    {
        let mut start_arguments = arguments.clone();
        start_arguments["team_run_id"] = Value::String(team_run_id.to_string());
        let start = tool_team_run_start(store, resolved, &start_arguments)?;
        return Ok(json!({
            "reopen": reopened,
            "runtime_start": start,
        }));
    }
    Ok(json!({"reopen": reopened}))
}

fn tool_team_run_answer_message(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    reject_unknown_arguments(
        arguments,
        "team_run_answer_message",
        &["team_run_id", "message_id", "option_id", "response_text"],
    )?;
    let team_run_id = required_str(arguments, "team_run_id")?;
    let message_id = required_str(arguments, "message_id")?;
    let actor_kind_raw = std::env::var("AGENTFIRM_MCP_ACTOR_KIND")
        .map_err(|_| "MCP transport is missing AGENTFIRM_MCP_ACTOR_KIND".to_string())?;
    let actor_id = std::env::var("AGENTFIRM_MCP_ACTOR_ID")
        .map_err(|_| "MCP transport is missing AGENTFIRM_MCP_ACTOR_ID".to_string())?;
    let actor_kind = agentfirm_api::parse_actor_kind(&actor_kind_raw)
        .ok_or_else(|| "AGENTFIRM_MCP_ACTOR_KIND is invalid".to_string())?;
    let body = serde_json::json!({
        "option_id": arguments.get("option_id").cloned().unwrap_or(Value::Null),
        "response_text": arguments.get("response_text").cloned().unwrap_or(Value::Null),
    });
    answer_provider_message_value(
        store,
        team_run_id,
        message_id,
        &body,
        &harness_core::agentfirm_api::ActorRef {
            kind: actor_kind,
            id: actor_id,
        },
        "mcp_transport",
    )
    .map_err(|error| error.to_string())
}

fn tool_team_run_start(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    let id = required_str(arguments, "team_run_id")?;
    let max_concurrency = arguments
        .get("max_concurrency")
        .and_then(Value::as_u64)
        .unwrap_or(4) as usize;
    if max_concurrency == 0 {
        return Err("max_concurrency must be positive".into());
    }
    let node_daemon = delegate_team_run_to_node_daemon(store, resolved, id, max_concurrency)
        .map_err(|error| error.to_string())?;
    let run = latest_team_run(store, id).map_err(|error| error.to_string())?;
    Ok(
        json!({"team_run": run, "node_daemon": node_daemon, "dashboard_url": team_dashboard_url(store, resolved, id)}),
    )
}

fn tool_team_run_cancel(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    let id = required_str(arguments, "team_run_id")?;
    let run = transition_team_run(store, id, TeamRunStatus::Cancelled)
        .map_err(|error| error.to_string())?;
    Ok(json!({"team_run": run, "dashboard_url": team_dashboard_url(store, resolved, id)}))
}

/// Read a required string argument, or the tool-error message.
fn required_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string argument `{key}`"))
}

fn optional_str(arguments: &Value, key: &str) -> Result<Option<String>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| format!("argument `{key}` must be a string or null")),
    }
}

fn tool_mission_list(store: &HarnessStore) -> Result<Value, String> {
    Ok(json!(store
        .latest_missions()
        .map_err(|error| error.to_string())?))
}

/// `team_run_create` — journal a new run, idle members, and explicit initial Works.
fn tool_team_run_create(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    if arguments.get("wave_index").is_some() || arguments.get("wave_id").is_some() {
        return Err(
            "wave_id and wave_index are Legacy-only and cannot create a current TeamRun; supply agent_team_id and derive Mission through AgentTeam".to_string(),
        );
    }
    let objective = required_str(arguments, "objective")?;
    let budget_limit_usd = match arguments.get("budget_limit_usd") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_f64()
                .ok_or_else(|| "budget_limit_usd must be a number or null".to_string())?,
        ),
    };
    let member_values = arguments
        .get("members")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut members = Vec::new();
    for (index, member) in member_values.iter().enumerate() {
        let member_str = |key: &str| {
            member
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("members[{index}].{key} must be a string"))
        };
        let owned_paths = match member.get("owned_paths") {
            None => Vec::new(),
            Some(Value::Array(paths)) => paths
                .iter()
                .enumerate()
                .map(|(path_index, path)| {
                    path.as_str().map(str::to_string).ok_or_else(|| {
                        format!("members[{index}].owned_paths[{path_index}] must be a string")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => {
                return Err(format!("members[{index}].owned_paths must be an array"));
            }
        };
        members.push(TeamMemberSpec {
            agent_member_id: member_str("agent_member_id")?.to_string(),
            name: member_str("name")?.to_string(),
            role: member_str("role")?.to_string(),
            provider: member_str("provider")?.to_string(),
            execution_mode: optional_str(member, "execution_mode")?,
            model: optional_str(member, "model")?,
            effort: optional_str(member, "effort")?,
            service_tier: optional_str(member, "service_tier")?,
            provider_cwd_hint: optional_str(member, "provider_cwd_hint")?,
            owned_paths,
            resume_native_session_id: optional_str(member, "resume_native_session_id")?,
            initial_work: optional_str(member, "initial_work")?,
        });
    }
    let agent_team_id = required_non_empty_str(arguments, "agent_team_id")?.to_string();
    if members.is_empty() {
        let execution_space_id = resolved
            .execution_space_context
            .as_ref()
            .map(|space| space.id.as_str())
            .ok_or_else(|| {
                "team run creation requires an explicitly selected execution space".to_string()
            })?;
        members = team_member_specs_from_definition(store, execution_space_id, &agent_team_id)
            .map_err(|error| error.to_string())?;
    }
    let created = create_team_run(
        store,
        resolved.context.as_ref(),
        resolved
            .execution_space_context
            .as_ref()
            .map(|space| space.id.as_str()),
        optional_str(arguments, "execution_root")?,
        objective,
        budget_limit_usd,
        optional_str(arguments, "host_surface")?
            .as_deref()
            .unwrap_or("mcp"),
        optional_str(arguments, "host_thread_id")?,
        optional_str(arguments, "previous_run_id")?,
        Some(agent_team_id),
        None,
        None,
        &members,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "team_run_id": created.team_run.id,
        "member_run_ids": created.team_run.member_run_ids,
        "mission_id": team_run_mission_id(store, &created.team_run).map_err(|error| error.to_string())?,
        "execution_root": created.team_run.execution_root,
        "member_runs": created.member_runs,
        "works": created.works,
        "dashboard_url": team_dashboard_url(store, resolved, &created.team_run.id),
    }))
}

/// `team_run_add_member` — extend an active long-lived run and create the new
/// member's optional initial Work.
fn tool_team_run_add_member(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let initial_work = optional_str(arguments, "initial_work")?;
    let member = arguments
        .get("member")
        .and_then(Value::as_object)
        .ok_or_else(|| "member must be an object".to_string())?;
    let member_str = |key: &str| {
        member
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("member.{key} must be a string"))
    };
    let owned_paths = match member.get("owned_paths") {
        None => Vec::new(),
        Some(Value::Array(paths)) => paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                path.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("member.owned_paths[{index}] must be a string"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("member.owned_paths must be an array".to_string()),
    };
    let member = TeamMemberSpec {
        agent_member_id: member_str("agent_member_id")?.to_string(),
        name: member_str("name")?.to_string(),
        role: member_str("role")?.to_string(),
        provider: member_str("provider")?.to_string(),
        execution_mode: optional_str(&Value::Object(member.clone()), "execution_mode")?,
        model: optional_str(&Value::Object(member.clone()), "model")?,
        effort: optional_str(&Value::Object(member.clone()), "effort")?,
        service_tier: optional_str(&Value::Object(member.clone()), "service_tier")?,
        provider_cwd_hint: optional_str(&Value::Object(member.clone()), "provider_cwd_hint")?,
        owned_paths,
        resume_native_session_id: optional_str(
            &Value::Object(member.clone()),
            "resume_native_session_id",
        )?,
        initial_work: None,
    };
    let (run, member_run, work) = add_team_run_member(
        store,
        resolved.context.as_ref(),
        team_run_id,
        &member,
        initial_work.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "team_run": run,
        "member_run": member_run,
        "work": work,
        "dashboard_url": team_dashboard_url(store, resolved, team_run_id),
    }))
}

fn tool_team_run_rename_member(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    rename_team_run_member(
        store,
        required_str(arguments, "team_run_id")?,
        required_str(arguments, "member_run_id")?,
        required_str(arguments, "name")?,
    )
    .map(|member| json!(member))
    .map_err(|error| error.to_string())
}

fn tool_team_run_deactivate_member(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    deactivate_team_run_member(
        store,
        required_str(arguments, "team_run_id")?,
        required_str(arguments, "member_run_id")?,
        required_str(arguments, "reason")?,
    )
    .map(|member| json!(member))
    .map_err(|error| error.to_string())
}

/// `team_run_list` — the latest projection of every run, trimmed to the
/// fields a host needs to pick one.
fn tool_team_run_list(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    // One coordination store holds every tenant that shares an Execution Space
    // (which is the decided architecture -- ADR 0042 makes the Project Binding
    // store a compatibility root that does not own new execution rows). What is
    // missing is a way to ask for one tenant: an unscoped list measured 64 KB
    // with another project's rows listed first, and every caller pays for every
    // other tenant's history on each call.
    let project_binding_id = arguments
        .get("project_binding_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let status_filter = arguments.get("status").and_then(Value::as_str);
    let runs = latest_team_runs_in_append_order(store).map_err(|error| error.to_string())?;
    let runs: Vec<_> = runs
        .into_iter()
        .filter(|run| match project_binding_id.as_deref() {
            Some(wanted) => run.project_binding_id == wanted,
            None => true,
        })
        .filter(|run| match status_filter {
            Some(wanted) => serde_snake_label(&run.status) == wanted,
            None => true,
        })
        .collect();
    Ok(Value::Array(
        runs.iter()
            .map(|run| {
                json!({
                    "id": run.id,
                    "objective": run.objective,
                    "status": run.status,
                    "member_count": run.member_run_ids.len(),
                    "project_binding_id": run.project_binding_id,
                    "created_at": run.created_at,
                })
            })
            .collect(),
    ))
}

/// `team_run_status` — one run with its members (each carrying the latest
/// MemberAction, if any), a current canonical Message-fabric summary, and the
/// dashboard URL. Historical `team_messages.jsonl` rows never participate.
fn tool_team_run_status(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    let id = required_str(arguments, "team_run_id")?;
    let run = require_current_team_run(store, id)?;
    let execution_space_id = mcp_team_run_execution_space_id(store, resolved, &run)?;
    let member_runs: Vec<_> = latest_member_runs_in_append_order(store)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|member| member.team_run_id == id)
        .collect();
    let actions =
        visible_member_actions_in_append_order(store).map_err(|error| error.to_string())?;
    let members: Vec<Value> = member_runs
        .iter()
        .map(|member| {
            let latest_action = actions
                .iter()
                .filter(|action| action.team_run_id == id && action.member_run_id == member.id)
                .max_by_key(|action| action.seq);
            json!({
                "member_run": member,
                "latest_action": latest_action,
            })
        })
        .collect();
    let message_summary = canonical_message_summary_for_run(store, id, &execution_space_id)?;
    let supervisor = store
        .latest_team_supervisor_lease(id)
        .map_err(|error| error.to_string())?;
    let supervisor_current = supervisor.as_ref().is_some_and(|lease| {
        lease.status == TeamSupervisorLeaseStatus::Active
            && lease.expires_unix_ms > current_unix_ms_u64()
    });
    Ok(json!({
        "team_run": run,
        "members": members,
        "message_summary": message_summary,
        "supervisor": {
            "lease": supervisor,
            "current": supervisor_current,
        },
        "dashboard_url": team_dashboard_url(store, resolved, id),
    }))
}

fn canonical_message_summary_for_run(
    store: &HarnessStore,
    team_run_id: &str,
    execution_space_id: &str,
) -> Result<Value, String> {
    let mut messages = store
        .fabric_messages(execution_space_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|message| message.team_run_id.as_deref() == Some(team_run_id))
        .collect::<Vec<_>>();
    messages.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let message_ids = messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<BTreeSet<_>>();
    let deliveries = store
        .fabric_message_deliveries(execution_space_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|delivery| message_ids.contains(delivery.message_id.as_str()))
        .collect::<Vec<_>>();
    let answered_request_ids = messages
        .iter()
        .filter(|message| {
            message.kind == harness_core::agentfirm_api::MessageKind::ProviderInteractionResponse
        })
        .filter_map(|message| message.causation_id.as_deref())
        .collect::<BTreeSet<_>>();
    let provider_interaction_requests = messages
        .iter()
        .filter(|message| {
            message.kind == harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest
        })
        .count();
    let provider_interaction_responses = messages
        .iter()
        .filter(|message| {
            message.kind == harness_core::agentfirm_api::MessageKind::ProviderInteractionResponse
        })
        .count();
    let awaiting_host_response = messages
        .iter()
        .filter(|message| {
            message.kind == harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest
                && !answered_request_ids.contains(message.id.as_str())
        })
        .count();
    let actionable_deliveries = deliveries
        .iter()
        .filter(|delivery| {
            matches!(
                delivery.status,
                harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued
                    | harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Routed
                    | harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Claimed
                    | harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::ProviderReceived
            )
        })
        .count();
    Ok(json!({
        "total": messages.len(),
        "provider_interaction_requests": provider_interaction_requests,
        "provider_interaction_responses": provider_interaction_responses,
        "awaiting_host_response": awaiting_host_response,
        "actionable_deliveries": actionable_deliveries,
    }))
}

/// Resolve the strict current Execution Space of one TeamRun before any MCP
/// Message projection reads. The Store validates the complete declared member
/// set under its writer lock; MCP only enforces agreement with its selected
/// Execution Space and never performs an independent physical-space scan.
fn mcp_team_run_execution_space_id(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    run: &AgentTeamRun,
) -> Result<String, String> {
    let execution_space_id = store
        .current_team_run_execution_space(run)
        .map_err(|error| error.to_string())?;
    if let Some(selected) = resolved.execution_space_context.as_ref() {
        if selected.id != execution_space_id {
            return Err(format!(
                "EXECUTION_SPACE_SCOPE_MISMATCH: TeamRun {} belongs to {}, not selected Execution Space {}",
                run.id, execution_space_id, selected.id
            ));
        }
    }
    Ok(execution_space_id)
}

/// `team_run_inbox` — canonical Message/MessageDelivery projection addressed
/// to one member. `all=true` includes terminal delivery history; the retired
/// `team_messages.jsonl` ledger is never consulted.
fn tool_team_run_inbox(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let member_run_id = required_str(arguments, "member_run_id")?;
    let include_all = arguments
        .get("all")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let messages = team_run_inbox(store, team_run_id, member_run_id, include_all)
        .map_err(|error| error.to_string())?;
    Ok(json!({"messages": messages}))
}

/// `team_inbox_list` — read the shared Team Inbox projection (DOC-106):
/// Team-subject canonical deliveries joined with their immutable Messages.
/// Read-only; claiming stays on the canonical store mutation path.
fn tool_team_inbox_list(
    store: &HarnessStore,
    resolved: &crate::ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_id = required_str(arguments, "team_id")?;
    let include_all = arguments
        .get("all")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let execution_space_id = resolved
        .execution_space_context
        .as_ref()
        .map(|space| space.id.clone())
        .ok_or_else(|| "team_inbox_list requires an explicit Execution Space".to_string())?;
    crate::team_inbox_projection(store, &execution_space_id, team_id, include_all)
        .map_err(|error| error.to_string())
}

/// `team_run_host_inbox` — aggregate canonical Host mail only for TeamRuns
/// bound to the exact provider-native Host thread.
fn tool_team_run_host_inbox(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let host_surface = required_str(arguments, "host_surface")?;
    let host_thread_id = required_str(arguments, "host_thread_id")?;
    let include_all = arguments
        .get("all")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let runs = host_inbox_for_native_thread(store, host_surface, host_thread_id, include_all)
        .map_err(|error| error.to_string())?;
    Ok(json!({"runs": runs}))
}

/// `team_run_events` — the run's folded event log, seq-ordered, optionally
/// resumed after a seen seq (pass the last seq you have as `after_seq`).
fn tool_team_run_events(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let id = required_str(arguments, "team_run_id")?;
    require_current_team_run(store, id)?;
    let after_seq = arguments
        .get("after_seq")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut events: Vec<TeamRunEvent> = store
        .current_team_run_events(id)
        .map_err(|error| crate::CliError::Store(error).to_string())?
        .into_iter()
        .filter(|event| event.seq > after_seq)
        .collect();
    events.sort_by_key(|event| event.seq);
    Ok(json!(events))
}

fn require_current_team_run(store: &HarnessStore, id: &str) -> Result<AgentTeamRun, String> {
    let run = latest_team_run(store, id).map_err(|error| error.to_string())?;
    store
        .current_team_run_execution_space(&run)
        .map_err(|error| error.to_string())?;
    Ok(run)
}

/// Mission / Mission Log authoring plus Agent Team tools. Descriptions ARE the interface
/// contract — the host model reads them to decide how to call each tool.
fn tool_definitions() -> Value {
    json!([
        {
            "name": "agentfirm_member_trust_mutate",
            "description": "Execute one advertised Member Execution Trust lifecycle or Work command. MemberRun creation is available only through team_run_create or team_run_add_member. Close requires Active; Reopen requires Closed; ResumeNativeSession requires Active plus a Disconnected, Failed, or Stopped runtime. Lifecycle changes use the combined TeamRun authority and never mutate only one projection. Actor identity comes only from the MCP process transport environment.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "command": {
                        "type": "object",
                        "description": "One tagged command from the closed MCP Member Trust inventory.",
                        "properties": {
                            "command": {"type": "string", "enum": MCP_MEMBER_TRUST_COMMANDS}
                        },
                        "required": ["command"]
                    },
                    "idempotency_key": {"type": "string", "minLength": 1},
                    "expected_version": {"type": "integer", "minimum": 0}
                },
                "required": ["command", "idempotency_key", "expected_version"]
            }
        },
        {
            "name": "remote_fabric_status",
            "description": "Read current Node-local Remote Fabric queue/session truth and, when this machine hosts it, Company Control Plane Node/lease diagnostics. This tool is read-only and never reconstructs route truth from Message or runtime ledgers.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "company_id": {"type": "string", "minLength": 1}
                },
                "required": ["company_id"]
            }
        },
        {
            "name": "remote_fabric_operation_show",
            "description": "Read one RoutedOperation with its transport Attempts and generation-fenced Receipts from the local Company Control Plane FabricStore. It is unavailable away from the Control Plane and performs no replay or mutation.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "company_id": {"type": "string", "minLength": 1},
                    "operation_id": {"type": "string", "minLength": 1}
                },
                "required": ["company_id", "operation_id"]
            }
        },
        {
            "name": "mission_list",
            "description": "Read-only legacy read of historical Mission rows (DOC-108). Not current authority; no writer surface remains.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "team_run_create",
            "description": "Create one runtime attempt from a required flat AgentTeam. ExecutionNode and Project Binding are derived from the durable Team and selected execution context; members can come from the Team definition. Legacy Mission provenance is optional and never required.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "objective": {"type": "string", "minLength": 1, "description": "Durable TeamRun context. It never silently assigns the same responsibility to every member."},
                    "budget_limit_usd": {"type": "number", "minimum": 0, "description": "Optional budget cap in USD, recorded on the run."},
                    "previous_run_id": {"type": "string", "description": "Optional previous attempt id; it must belong to the same durable AgentTeam."},
                    "agent_team_id": {"type": "string", "minLength": 1, "description": "Required durable flat AgentTeam identity."},
                    "execution_root": {"type": "string", "minLength": 1, "description": "Optional TeamRun execution root. Must be the selected project_root or a Git worktree sharing its git common directory; defaults to project_root."},
                    "host_surface": {"type": "string", "minLength": 1, "description": "Exact provider-native Host surface, for example codex-app. Defaults to mcp when the calling Host does not bind itself."},
                    "host_thread_id": {"type": "string", "minLength": 1, "description": "Exact native Host task/session id. Required for Plugin safe-boundary delivery to this Host."},
                    "members": {
                        "type": "array",
                        "description": "One entry per team member.",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string", "minLength": 1, "description": "Member display name, unique within the run."},
                                "role": {"type": "string", "minLength": 1, "description": "e.g. coordinator / implementer / reviewer."},
                                "provider": {"type": "string", "minLength": 1, "description": "Provider label. Harness-driven modes require a registered codex, kimi, or claude adapter; external_interactive accepts any non-empty label because Harness does not execute it."},
                                "execution_mode": {"type": "string", "enum": ["codex_app_server", "kimi_acp", "claude_agent_sdk", "external_interactive"], "description": "Optional provider-specific Agent Team mode. Codex only accepts codex_app_server and Claude only accepts claude_agent_sdk; codex_exec and claude_cli are workflow-only. external_interactive declares the user's own already-open session: Harness spawns no provider process, does not constrain its provider label, and the member polls its own inbox."},
                                "model": {"type": "string", "minLength": 1, "description": "Optional provider model override."},
                                "effort": {"type": "string", "minLength": 1, "description": "Optional provider-neutral reasoning-effort request. The adapter must record the provider-confirmed effective value or an unsupported/review_required status."},
                                "service_tier": {"type": "string", "minLength": 1, "description": "Optional provider-neutral latency/service profile request, such as priority. This is not a universal fast boolean."},
                                "provider_cwd_hint": {"type": "string", "minLength": 1, "description": "Optional member workspace override. Must be the selected project_root or a Git worktree sharing its git common directory, including external Codex worktrees."},
                                "owned_paths": {"type": "array", "items": {"type": "string", "minLength": 1}, "description": "Paths this member exclusively owns."},
                                "initial_work": {"type": "string", "minLength": 1, "description": "Optional completion criteria for one initial Host-assigned Work. Omit to create the member idle."},
                                "resume_native_session_id": {"type": "string", "minLength": 1, "description": "Explicit provider-owned session to resume. Never inferred from recent local history."}
                            },
                            "required": ["name", "role", "provider"]
                        }
                    }
                },
                "required": ["objective", "agent_team_id"]
            }
        },
        {
            "name": "team_run_work_list",
            "description": "List the authoritative shared Works board for one TeamRun. brief=true returns compact works_brief text lines (id, status, owner, version, title<=60 chars) instead of full Work JSON. since=<cursor> is a delta read: only Works whose latest WorkOperation postdates the cursor (a WorkOperation-order sequence, not a Work version), returned alongside a next_since watermark to chain the next call; combine with brief for the smallest board read.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "minLength": 1},
                    "brief": {"type": "boolean", "default": false, "description": "Return works_brief (one compact text line per Work) instead of works (full Work JSON)."},
                    "since": {"type": "integer", "minimum": 0, "description": "WorkOperation-order cursor. Only Works that changed after this point are returned; response adds since/next_since."}
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_work_show",
            "description": "Show one Work with its append-only WorkEvents and latest WorkDeliveries.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}}, "required": ["team_run_id", "work_id"]}
        },
        {
            "name": "team_run_work_create",
            "description": "Create durable team responsibility. Host may assign it immediately, expose it for self-claim, or leave it unassigned.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "team_run_id": {"type": "string", "minLength": 1},
                    "id": {"type": "string", "minLength": 1, "description": "Optional caller-stable Work id."},
                    "title": {"type": "string", "minLength": 1},
                    "context_markdown": {"type": "string"},
                    "completion_criteria_markdown": {"type": "string", "minLength": 1},
                    "owner_member_run_id": {"type": "string", "minLength": 1, "description": "Optional concrete ProviderRuntimeProjection to receive the first ProviderWorkDispatch; stable AgentMember ownership is derived by the store."},
                    "claim_mode": {"type": "string", "enum": ["host_assign", "team_claim"]},
                    "eligible_member_ids": {"type": "array", "items": {"type": "string", "minLength": 1}},
                    "parent_work_id": {"type": "string", "minLength": 1},
                    "prerequisite_work_ids": {"type": "array", "items": {"type": "string", "minLength": 1}},
                    "priority": {"type": "string", "enum": ["low", "normal", "high", "urgent"]},
                    "caused_by_message_id": {"type": "string", "minLength": 1},
                    "idempotency_key": {"type": "string", "minLength": 1}
                },
                "required": ["team_run_id", "title", "completion_criteria_markdown"]
            }
        },
        {
            "name": "team_run_work_assign",
            "description": "Host performs the first assignment of open Work using optimistic versioning. This does not move an existing stable owner to another runtime; use team_run_work_rebind for that.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "member_run_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "member_run_id", "expected_version"]}
        },
        {
            "name": "team_run_work_rebind",
            "description": "Host preserves the Work's stable AgentMember owner while moving its active runtime binding to another active ProviderRuntimeProjection for that same identity, for example after a runtime replacement or crash recovery.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "member_run_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "member_run_id", "expected_version"]}
        },
        {
            "name": "team_run_work_block",
            "description": "Host pauses owned in-progress Work with a durable blocker reason. Use ordinary Work-linked messages only for the surrounding discussion.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "reason": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "reason"]}
        },
        {
            "name": "team_run_work_resume",
            "description": "Host resumes blocked Work after recording how the blocker was resolved; the latest owner is woken through ProviderWorkDispatch.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "resolution": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "resolution"]}
        },
        {
            "name": "team_run_work_release",
            "description": "Host releases open owned Work back to the shared Ready Pool when it has not been claimed or delivered to a provider.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version"]}
        },
        {
            "name": "team_run_work_request_changes",
            "description": "Host returns submitted Work with specific feedback; a new delivery wakes the current owner.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "reason": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "reason"]}
        },
        {
            "name": "team_run_work_cancel",
            "description": "Host cancels unfinished Work without closing the member or TeamRun.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "reason": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "reason"]}
        },
        {
            "name": "team_run_work_reconcile_delivery",
            "description": "A successor Supervisor explicitly requeues one stale claimed ProviderWorkDispatch after a crash. The caller must name the successor Supervisor id and generation; this never guesses provider consumption or changes Work responsibility.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "delivery_id": {"type": "string"},
                    "supervisor_id": {"type": "string"},
                    "supervisor_generation": {"type": "integer", "minimum": 1}
                },
                "required": ["team_run_id", "delivery_id", "supervisor_id", "supervisor_generation"]
            }
        },
        {
            "name": "collaboration_delegation_list",
            "description": "Read the Company Control Plane's canonical cross-Team Delegations. This tool never folds an Execution Space's retired local WorkDelegation ledger.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"source_team_id": {"type": "string", "minLength": 1}, "target_team_id": {"type": "string", "minLength": 1}, "node_id": {"type": "string", "minLength": 1}, "state": {"type": "string", "enum": ["proposed", "awaiting_target_decision", "provisioning_target_work", "active", "result_available", "cancellation_requested", "terminal"]}, "limit": {"type": "integer", "minimum": 1, "maximum": 100}, "cursor": {"type": "string", "minLength": 1}}}
        },
        {
            "name": "collaboration_delegation_show",
            "description": "Read one canonical Company WorkDelegation with its cancellation requests and immutable remote publications.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"delegation_id": {"type": "string", "minLength": 1}}, "required": ["delegation_id"]}
        },
        {
            "name": "execution_node_list",
            "description": "List stable ExecutionNodes with project registrations and current NodeDaemon lease generations.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {}}
        },
        {
            "name": "execution_node_show",
            "description": "Show one ExecutionNode with its registrations and current NodeDaemon lease.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"node_id": {"type": "string", "minLength": 1}}, "required": ["node_id"]}
        },
        {
            "name": "team_run_add_member",
            "description": "Add one idle member to an active planning/running/waiting TeamRun and optionally create a first Work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "initial_work": {"type": "string", "minLength": 1},
                    "member": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "minLength": 1},
                            "role": {"type": "string", "minLength": 1},
                            "provider": {"type": "string", "minLength": 1},
                            "execution_mode": {"type": "string", "enum": ["codex_app_server", "kimi_acp", "claude_agent_sdk", "external_interactive"]},
                            "model": {"type": "string", "minLength": 1},
                            "effort": {"type": "string", "minLength": 1},
                            "service_tier": {"type": "string", "minLength": 1},
                            "provider_cwd_hint": {"type": "string", "minLength": 1},
                            "owned_paths": {"type": "array", "items": {"type": "string", "minLength": 1}},
                            "resume_native_session_id": {"type": "string", "minLength": 1}
                        },
                        "required": ["name", "role", "provider"]
                    }
                },
                "required": ["team_run_id", "member"]
            }
        },
        {
            "name": "team_run_rename_member",
            "description": "Rename one ProviderRuntimeProjection for future coordination and Dashboard display without replacing its provider-native session or historical id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "name": {"type": "string", "minLength": 1}
                },
                "required": ["team_run_id", "member_run_id", "name"]
            }
        },
        {
            "name": "team_run_deactivate_member",
            "description": "Deactivate an idle, queued, waiting, reviewing, or blocked ProviderRuntimeProjection while preserving its history. An active provider turn must be interrupted first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string", "minLength": 1}
                },
                "required": ["team_run_id", "member_run_id", "reason"]
            }
        },
        {
            "name": "team_run_start",
            "description": "Reserve and start a planning AgentTeamRun asynchronously, returning its running projection and exact Workspace-scoped UI URL immediately. Agent Team modes are Codex app-server (codex_app_server), Kimi ACP (kimi_acp), and Claude Agent SDK streaming (claude_agent_sdk); declared external_interactive members are user-driven and skipped by the supervisor. Bounded codex_exec and claude_cli are workflow-only and never Team fallbacks. Provider cwd is the member worktree or selected Workspace project_root, never store_root. Provider transcripts and thinking remain in provider-native sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "max_concurrency": {"type": "integer", "minimum": 1, "default": 4},
                    "idle_timeout_s": {"type": "integer", "minimum": 1, "default": 120}
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_cancel",
            "description": "Cancel a planning, waiting, or reviewing TeamRun. Running cancellation is rejected until cooperative provider interruption exists.",
            "inputSchema": {
                "type": "object",
                "properties": {"team_run_id": {"type": "string"}},
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_list",
            "description": "List team runs in the store (latest projection, append order). One Execution Space store holds every tenant bound to it, so pass project_binding_id to see only one project's runs and status to drop finished ones. Mission is derived through AgentTeam; Legacy Wave rows never participate.",
            "inputSchema": {"type": "object", "properties": {
                "project_binding_id": {"type": "string", "description": "Return only runs bound to this project."},
                "status": {"type": "string", "description": "Return only runs in this status, for example running."}
            }}
        },
        {
            "name": "team_run_status",
            "description": "Show one team run: the run row, every member run with its latest MemberAction, a canonical Message-fabric summary, and the live dashboard URL. Historical team_messages.jsonl rows are excluded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "description": "Run id returned by team_run_create / team_run_list."},
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_board_summary",
            "description": "Decision-shaped Works board digest for one TeamRun (issue #305): a single `summary` string, always under 500 chars, with counts by status (open/in_progress/blocked/review/done/cancelled), assigned vs unassigned, the claim-ready count, and one `member: idle|working|awaiting-review` line per active member. Use this instead of team_run_work_list when the question is 'what should I do next', not 'show me every Work'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "description": "Run id returned by team_run_create / team_run_list."}
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_host_inbox",
            "description": "Read canonical Host mail across only those TeamRuns explicitly bound to one exact provider-native Host surface/thread. This is the safe Plugin/App integration path: it never leaks another Host task's inbox. By default returns actionable mail; all=true includes terminal canonical delivery history.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host_surface": {"type": "string", "description": "Provider-native Host surface, for example codex-app."},
                    "host_thread_id": {"type": "string", "description": "Exact native Host task/session id stored on AgentTeamRun."},
                    "all": {"type": "boolean", "default": false}
                },
                "required": ["host_surface", "host_thread_id"]
            }
        },
        {
            "name": "team_run_inbox",
            "description": "Read the canonical Message/MessageDelivery projection addressed to one ProviderRuntimeProjection (or the reserved host recipient). By default returns actionable mail; all=true includes terminal delivery history. Historical team_messages.jsonl rows and provider-native transcript/tool history are excluded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string", "description": "ProviderRuntimeProjection id, or `host` for the Lead inbox."},
                    "all": {"type": "boolean", "default": false}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_inbox_list",
            "description": "Read one AgentTeam's shared Team Inbox (DOC-106): Team-subject canonical MessageDeliveries joined with their immutable Messages, including delivery status, claim binding, correlation, and author/source-Team provenance. Team-addressed peer Messages land here without waking every Member until one exact TeamMembership generation claims the delivery. Read-only; by default returns unclaimed queued items, all=true includes the full delivery history.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_id": {"type": "string", "description": "Exact AgentTeam id."},
                    "all": {"type": "boolean", "default": false}
                },
                "required": ["team_id"]
            }
        },
        {
            "name": "team_run_answer_message",
            "description": "Answer a provider-originated correlated question or plan-review Message as the authenticated AgentTeam Host. The exact response Message is durably published before the request delivery is ACKed, so an exact retry can recover a crash between those writes without duplicating the answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "message_id": {"type": "string"},
                    "option_id": {"type": "string", "description": "Exact option id exposed by the provider message."},
                    "response_text": {"type": "string", "description": "Free-form response only when the provider request exposes no exact options."}
                },
                "required": ["team_run_id", "message_id"]
            }
        },
        {
            "name": "team_run_steer_member",
            "description": "Inject operator or Lead input into a currently active provider turn. This is capability-gated and currently requires codex_app_server; when no turn is active, publish a canonical Message through the normal AgentTeam message surface.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "content": {"type": "string", "minLength": 1},
                    "requested_by": {"type": "string", "default": "host"}
                },
                "required": ["team_run_id", "member_run_id", "content"]
            }
        },
        {
            "name": "team_run_interrupt_member",
            "description": "Cooperatively interrupt one active provider turn when its execution mode advertises supports_cancel. Codex app-server uses turn/interrupt, Kimi ACP uses session/cancel, and Claude Agent SDK uses query.interrupt while preserving its native session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string"},
                    "requested_by": {"type": "string", "default": "host"}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_run_close_member",
            "description": "Explicitly close one Member runtime while preserving the same ProviderRuntimeProjection, native-session binding, and frozen mailbox for a later reopen. Managed adapters release their Harness-owned process; external_interactive only closes Harness coordination because its process is user-owned. The live request must be sent through the same Host server process that started the TeamRun.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string"},
                    "requested_by": {"type": "string", "default": "host"}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_run_reopen_member",
            "description": "Reopen a closed ProviderRuntimeProjection in place. Managed adapters increment runtime_generation, start a new adapter process, and resume the exact provider-native session; Harness never reconstructs a transcript or silently starts fresh. external_interactive reopens only the coordination binding because its process and conversation are user-owned. Retired members cannot reopen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string"},
                    "reopened_by": {"type": "string", "default": "host"},
                    "max_concurrency": {"type": "integer", "minimum": 1, "default": 4},
                    "idle_timeout_s": {"type": "integer", "minimum": 1, "default": 120}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_run_events",
            "description": "Read a team run's folded event log, ordered by seq. Pass `after_seq` (the last seq you already saw) to resume incrementally; events cover team_run/member_run/message/member_action lifecycle rows with host or member source kind.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "after_seq": {"type": "integer", "description": "Only return events with seq greater than this (default 0 = all)."}
                },
                "required": ["team_run_id"]
            }
        }
    ])
}
