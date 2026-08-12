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
//! - `tools/list` → Mission/Wave authoring plus Agent Team tools.
//! - `tools/call` → `{content:[{type:"text",text:<result JSON>}], isError}`.
//! - unknown method → JSON-RPC -32601. stdin EOF exits.

use std::io::{BufRead, Write};

use harness_core::{
    PendingInteractionStatus, TeamActorKind, TeamActorRef, TeamRunEvent, TeamRunStatus,
    TeamSupervisorLeaseStatus, WaveStatus, Work, WorkCausationRef, WorkClaimMode,
    WorkCommandContext, WorkCondition, WorkPhase, WorkPriority,
};
use harness_store::HarnessStore;
use serde_json::{json, Value};

use crate::{
    add_team_run_member, agentfirm_api, close_mission, close_team_member_value, create_mission,
    create_team_run, current_unix_ms_u64, deactivate_team_run_member,
    delegate_team_run_to_node_daemon, format_work_brief_line, generated_id,
    has_actionable_delivered_manual_ack, host_inbox_for_native_thread, interrupt_team_member_value,
    latest_member_runs_in_append_order, latest_pending_interactions_in_append_order,
    latest_team_messages_in_append_order, latest_team_run, latest_team_runs_in_append_order,
    mutate_team_work_value, now_string, reconcile_team_work_delivery_value, rename_team_run_member,
    reopen_team_member_value, reopened_member_requires_supervisor_start,
    resolve_pending_interaction_value, retired_wave_write_error, revise_mission_context,
    serde_snake_label, steer_team_member_value, team_member_specs_from_definition,
    team_run_board_summary_text, team_run_inbox, team_run_mission_id, team_run_wave_index,
    transition_team_run, visible_member_actions_in_append_order, work_operation_cursors,
    ResolvedStore, TeamMemberSpec,
};

/// MCP protocol revision this server speaks, echoed verbatim in `initialize`
/// (the simple end of "reply with the client's version or the lower one").
const PROTOCOL_VERSION: &str = "2024-11-05";

/// The Vite Dashboard is the human UI. Its development proxy exposes the
/// Harness API at the same origin, so deep links must not point at the
/// API-only `harness serve` root on port 8787.
const DASHBOARD_UI_ORIGIN: &str = "http://127.0.0.1:5173";
const DASHBOARD_SAME_ORIGIN_API_BASE: &str = ".";

fn team_dashboard_url(store: &HarnessStore, resolved: &ResolvedStore, team_run_id: &str) -> String {
    let run = latest_team_run(store, team_run_id).ok();
    let mission_id = run
        .as_ref()
        .and_then(|run| team_run_mission_id(store, run).ok());
    let current_wave_id = mission_id.as_deref().and_then(|mission_id| {
        let mut waves = store.latest_waves().ok()?;
        waves.retain(|wave| wave.mission_id == mission_id);
        waves.sort_by_key(|wave| wave.index);
        waves
            .iter()
            .find(|wave| {
                matches!(
                    wave.status,
                    WaveStatus::Running | WaveStatus::Waiting | WaveStatus::Blocked
                )
            })
            .or_else(|| waves.iter().find(|wave| wave.status == WaveStatus::Planned))
            .or_else(|| waves.last())
            .map(|wave| wave.id.clone())
    });
    let context = match (mission_id.as_deref(), current_wave_id.as_deref()) {
        (Some(mission_id), Some(wave_id)) => format!("&mission={mission_id}&wave={wave_id}"),
        (Some(mission_id), None) => format!("&mission={mission_id}"),
        _ => String::new(),
    };
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
        "mission_create" => tool_mission_create(store, &arguments),
        "mission_update_context" => tool_mission_update_context(store, &arguments),
        "mission_close" => tool_mission_close(store, &arguments),
        "mission_list" => tool_mission_list(store),
        "wave_create" => tool_wave_create(store, &arguments),
        "wave_update" => tool_wave_update(store, &arguments),
        "wave_advance" => tool_wave_advance(store, &arguments),
        "wave_list" => tool_wave_list(store, &arguments),
        "wave_gate" => tool_wave_gate(store, &arguments),
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
        "collaboration_delegation_list" => tool_collaboration_delegation_list(&arguments),
        "collaboration_delegation_show" => tool_collaboration_delegation_show(&arguments),
        "execution_node_list" => tool_execution_node_list(store, &arguments),
        "execution_node_show" => tool_execution_node_show(store, &arguments),
        "remote_fabric_status" => tool_remote_fabric_status(resolved, &arguments),
        "remote_fabric_operation_show" => tool_remote_fabric_operation_show(resolved, &arguments),
        "team_run_rename_member" => tool_team_run_rename_member(store, &arguments),
        "team_run_deactivate_member" => tool_team_run_deactivate_member(store, &arguments),
        "team_run_start" => tool_team_run_start(store, resolved, &arguments),
        "team_run_cancel" => tool_team_run_cancel(store, resolved, &arguments),
        "team_message_acknowledge" => tool_team_message_acknowledge(store, resolved, &arguments),
        "team_run_list" => tool_team_run_list(store, &arguments),
        "team_run_status" => tool_team_run_status(store, resolved, &arguments),
        "team_run_board_summary" => tool_team_run_board_summary(store, &arguments),
        "team_run_host_inbox" => tool_team_run_host_inbox(store, &arguments),
        "team_run_inbox" => tool_team_run_inbox(store, &arguments),
        "team_run_send_message" => tool_team_run_send_message(store, &arguments),
        "team_run_reconcile_delivery" => tool_team_run_reconcile_delivery(store, &arguments),
        "team_run_resolve_interaction" => tool_team_run_resolve_interaction(store, &arguments),
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
    let command = serde_json::from_value::<agentfirm_api::TrustCommand>(
        arguments
            .get("command")
            .cloned()
            .ok_or_else(|| "argument `command` is required".to_string())?,
    )
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
        team_id: None,
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

fn tool_collaboration_delegation_list(arguments: &Value) -> Result<Value, String> {
    const ALLOWED: &[&str] = &[
        "company_id",
        "source_team_id",
        "target_team_id",
        "node_id",
        "state",
        "limit",
    ];
    reject_unknown_arguments(arguments, "collaboration_delegation_list", ALLOWED)?;
    let company_id = required_non_empty_str(arguments, "company_id")?;
    let state = optional_non_empty_str(arguments, "state")?
        .map(|value| serde_json::from_value(json!(value)).map_err(|error| error.to_string()))
        .transpose()?;
    let store = collaboration_store(company_id)?;
    let page = store
        .list_collaboration_delegations(
            company_id,
            &harness_store::CollaborationDelegationFilter {
                source_team_id: optional_non_empty_str(arguments, "source_team_id")?,
                target_team_id: optional_non_empty_str(arguments, "target_team_id")?,
                node_id: optional_non_empty_str(arguments, "node_id")?,
                state,
            },
            None,
            arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(100) as usize,
        )
        .map_err(|error| error.to_string())?;
    serde_json::to_value(page).map_err(|error| error.to_string())
}

fn tool_collaboration_delegation_show(arguments: &Value) -> Result<Value, String> {
    reject_unknown_arguments(
        arguments,
        "collaboration_delegation_show",
        &["company_id", "delegation_id"],
    )?;
    let company_id = required_non_empty_str(arguments, "company_id")?;
    let delegation_id = required_non_empty_str(arguments, "delegation_id")?;
    let store = collaboration_store(company_id)?;
    let delegation = store
        .collaboration_delegation(company_id, delegation_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("collaboration Delegation not found: {delegation_id}"))?;
    Ok(json!({
        "delegation": delegation,
        "cancellation_requests": store
            .collaboration_cancellation_requests(company_id, delegation_id)
            .map_err(|error| error.to_string())?,
        "publications": store
            .collaboration_publications(company_id, delegation_id)
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

fn tool_team_run_resolve_interaction(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let interaction_id = required_str(arguments, "interaction_id")?;
    resolve_pending_interaction_value(store, team_run_id, interaction_id, arguments)
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

fn tool_team_message_acknowledge(
    _store: &HarnessStore,
    _resolved: &ResolvedStore,
    _arguments: &Value,
) -> Result<Value, String> {
    Err("RETIRED_WRITE_AUTHORITY: team_message_acknowledge cannot authenticate the recipient session; acknowledge canonical MessageDelivery through the target NodeDaemon".to_string())
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

fn tool_mission_create(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let mission = create_mission(
        store,
        optional_str(arguments, "id")?,
        required_str(arguments, "title")?,
        required_str(arguments, "objective")?,
        optional_str(arguments, "desired_outcome")?,
        optional_str(arguments, "context")?,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!(mission))
}

fn tool_mission_close(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let mission = close_mission(
        store,
        required_str(arguments, "mission_id")?,
        required_str(arguments, "outcome")?,
        optional_str(arguments, "completed_by")?
            .as_deref()
            .unwrap_or("host"),
    )
    .map_err(|error| error.to_string())?;
    Ok(json!(mission))
}

fn tool_mission_update_context(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    revise_mission_context(
        store,
        required_str(arguments, "mission_id")?,
        required_str(arguments, "context")?,
    )
    .map(|mission| json!(mission))
    .map_err(|error| error.to_string())
}

fn tool_mission_list(store: &HarnessStore) -> Result<Value, String> {
    Ok(json!(store
        .latest_missions()
        .map_err(|error| error.to_string())?))
}

/// Wave write tools retired by the ADR 0051 Mission Log cutover — see
/// `crate::retired_wave_write_error`, the single source of truth this
/// mirrors across CLI, HTTP, and MCP so no surface keeps a live Wave-write
/// path. `wave_list` (below) stays: historical Wave rows remain readable.
fn tool_wave_create(_store: &HarnessStore, _arguments: &Value) -> Result<Value, String> {
    Err(retired_wave_write_error("create").to_string())
}

fn tool_wave_list(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let mission_id = optional_str(arguments, "mission_id")?;
    Ok(json!(store
        .latest_waves()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|wave| mission_id.as_deref().is_none_or(|id| wave.mission_id == id))
        .collect::<Vec<_>>()))
}

fn tool_wave_update(_store: &HarnessStore, _arguments: &Value) -> Result<Value, String> {
    Err(retired_wave_write_error("update").to_string())
}

fn tool_wave_advance(_store: &HarnessStore, _arguments: &Value) -> Result<Value, String> {
    Err(retired_wave_write_error("advance").to_string())
}

fn tool_wave_gate(_store: &HarnessStore, _arguments: &Value) -> Result<Value, String> {
    Err(retired_wave_write_error("gate").to_string())
}

/// `team_run_create` — journal a new run, idle members, and explicit initial Works.
fn tool_team_run_create(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    if arguments.get("wave_index").is_some() {
        return Err(
            "wave_index was retired; supply wave_id and derive order from the native Wave"
                .to_string(),
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
        members = team_member_specs_from_definition(store, &agent_team_id)
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
        "wave_id": null,
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
        optional_str(arguments, "source_plan_ref")?,
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
                let wave_index = team_run_wave_index(store, run).ok().flatten();
                json!({
                    "id": run.id,
                    "objective": run.objective,
                    "status": run.status,
                    "wave_index": wave_index,
                    "member_count": run.member_run_ids.len(),
                    "project_binding_id": run.project_binding_id,
                    "created_at": run.created_at,
                })
            })
            .collect(),
    ))
}

/// `team_run_status` — one run with its members (each carrying the latest
/// MemberAction, if any), the compatibility `unacked_messages` count of
/// actionable delivered `manual_ack` messages, and the dashboard URL. Mirrors
/// the `team-run status --json` projection.
fn tool_team_run_status(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    arguments: &Value,
) -> Result<Value, String> {
    let id = required_str(arguments, "team_run_id")?;
    let run = latest_team_run(store, id).map_err(|error| error.to_string())?;
    let wave_index = team_run_wave_index(store, &run).map_err(|error| error.to_string())?;
    let member_runs: Vec<_> = latest_member_runs_in_append_order(store)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|member| member.team_run_id == id)
        .collect();
    let actions =
        visible_member_actions_in_append_order(store).map_err(|error| error.to_string())?;
    let messages =
        latest_team_messages_in_append_order(store).map_err(|error| error.to_string())?;
    // Only genuinely pending interactions belong in a status snapshot. A
    // persistent run accumulates resolved approvals without bound; measured on
    // team-run-1785417151179 the unfiltered list was 69 resolved records =
    // 60,342 of 68,213 response chars (88% dead payload) growing monotonically
    // with run age. History stays available behind `include_resolved`.
    let include_resolved = arguments
        .get("include_resolved")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut resolved_interactions = 0usize;
    let pending_interactions: Vec<_> = latest_pending_interactions_in_append_order(store)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|interaction| interaction.team_run_id == id)
        .filter(|interaction| {
            // `Unsupported` is terminal but ACTIONABLE: the provider could not
            // render the prompt, so the Host must intervene. Bucketing it as
            // "resolved" would hide the one terminal state that still needs a
            // human/Host decision behind a name that reads as "already handled".
            if matches!(
                interaction.status,
                PendingInteractionStatus::Pending | PendingInteractionStatus::Unsupported
            ) {
                true
            } else {
                resolved_interactions += 1;
                include_resolved
            }
        })
        .collect();
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
    let unacked_messages = messages
        .iter()
        .filter(|message| message.team_run_id == id)
        .filter(|message| has_actionable_delivered_manual_ack(message))
        .count();
    let supervisor = store
        .latest_team_supervisor_lease(id)
        .map_err(|error| error.to_string())?;
    let supervisor_current = supervisor.as_ref().is_some_and(|lease| {
        lease.status == TeamSupervisorLeaseStatus::Active
            && lease.expires_unix_ms > current_unix_ms_u64()
    });
    Ok(json!({
        "team_run": run,
        "wave_index": wave_index,
        "members": members,
        "pending_interactions": pending_interactions,
        "resolved_interactions": resolved_interactions,
        "unacked_messages": unacked_messages,
        "supervisor": {
            "lease": supervisor,
            "current": supervisor_current,
        },
        "dashboard_url": team_dashboard_url(store, resolved, id),
    }))
}

/// The retired MCP tool cannot authenticate a stable sender identity. Keep the
/// name as an explicit hard-rejection surface until the MCP manifest removes
/// it; canonical authorship is an authenticated Role Action or source
/// NodeDaemon RuntimeCommand.
fn tool_team_run_send_message(_store: &HarnessStore, _arguments: &Value) -> Result<Value, String> {
    Err("RETIRED_WRITE_AUTHORITY: team_run_send_message cannot select a sender identity; use an authenticated AgentFirm Role Action or source NodeDaemon RuntimeCommand".to_string())
}

fn tool_team_run_reconcile_delivery(
    _store: &HarnessStore,
    _arguments: &Value,
) -> Result<Value, String> {
    Err("RETIRED_WRITE_AUTHORITY: team_run_reconcile_delivery cannot supply target NodeDaemon authority; use canonical target-NodeDaemon reconciliation".to_string())
}

/// `team_run_inbox` — latest-wins coordination mail addressed to one member.
/// The default projection is actionable queued/delivered mail; `all=true`
/// returns all received messages at their latest stored state.
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

/// `team_run_host_inbox` — aggregate actionable Host mail only for TeamRuns
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
    let after_seq = arguments
        .get("after_seq")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut events: Vec<TeamRunEvent> = store
        .team_run_events()
        .map_err(|error| crate::CliError::Store(error).to_string())?
        .into_iter()
        .filter(|event| event.team_run_id == id && event.seq > after_seq)
        .collect();
    events.sort_by_key(|event| event.seq);
    Ok(json!(events))
}

/// Mission/Wave authoring plus Agent Team tools. Descriptions ARE the interface
/// contract — the host model reads them to decide how to call each tool.
fn tool_definitions() -> Value {
    json!([
        {
            "name": "agentfirm_member_trust_mutate",
            "description": "Execute one canonical Member Execution Trust command through the same application service used by CLI and HTTP. Actor identity comes only from the MCP process transport environment.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "command": {"type": "object", "description": "One tagged TrustCommand payload."},
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
            "name": "mission_create",
            "description": "Create durable Mission intent and optional Markdown context. CLI owns the same operation; this MCP tool is a thin adapter.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Optional stable Mission id; generated when omitted."},
                    "title": {"type": "string"},
                    "objective": {"type": "string"},
                    "desired_outcome": {"type": "string"},
                    "context": {"type": "string", "description": "Durable Markdown Mission brief."}
                },
                "required": ["title", "objective"]
            }
        },
        {
            "name": "mission_update_context",
            "description": "Replace a Mission's durable Markdown context using the shared CLI/store service.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mission_id": {"type": "string"},
                    "context": {"type": "string", "minLength": 1}
                },
                "required": ["mission_id", "context"]
            }
        },
        {
            "name": "mission_close",
            "description": "Complete a Mission with an explicit outcome. Completed Missions are immutable; linked Team lifecycle is unchanged. Wave gate acceptance is no longer required (ADR 0051) — record a closeout_evidence Mission Log entry beforehand by convention.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mission_id": {"type": "string"},
                    "outcome": {"type": "string", "minLength": 1},
                    "completed_by": {"type": "string", "minLength": 1, "description": "Defaults to host."}
                },
                "required": ["mission_id", "outcome"]
            }
        },
        {
            "name": "mission_list",
            "description": "List latest native Mission rows.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "wave_create",
            "description": "Retired by the ADR 0051 Mission Log cutover; always returns an error. Use `mission_log_append`-equivalent CLI (`harness mission log append --mission-id <id> --kind judgment|replan|recovery|closeout_evidence --body <markdown>`) instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "mission_id": {"type": "string"},
                    "index": {"type": "integer", "minimum": 1, "description": "Optional explicit order; next order is selected when omitted."},
                    "title": {"type": "string"},
                    "objective": {"type": "string"},
                    "executor_kind": {"type": "string", "enum": ["agent_team", "dynamic_workflow", "host"]},
                    "exit_criteria": {"type": "string"},
                    "plan_note": {"type": "string"}
                    ,"context": {"type": "string", "description": "Host operational memo in Markdown."}
                    ,"updated_by": {"type": "string", "description": "Defaults to host."}
                },
                "required": ["mission_id", "title", "objective"]
            }
        },
        {
            "name": "wave_update",
            "description": "Retired by the ADR 0051 Mission Log cutover; always returns an error. Use `harness mission log append` instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "wave_id": {"type": "string"},
                    "context": {"type": "string", "minLength": 1},
                    "updated_by": {"type": "string", "description": "Defaults to host."}
                },
                "required": ["wave_id", "context"]
            }
        },
        {
            "name": "wave_advance",
            "description": "Retired by the ADR 0051 Mission Log cutover; always returns an error. Use `harness mission log append` instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "wave_id": {"type": "string"},
                    "outcome": {"type": "string", "minLength": 1},
                    "advanced_by": {"type": "string", "description": "Defaults to host."},
                    "artifact_refs": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["wave_id", "outcome"]
            }
        },
        {
            "name": "wave_list",
            "description": "List latest native Wave rows, optionally limited to one Mission.",
            "inputSchema": {"type": "object", "properties": {"mission_id": {"type": "string"}}}
        },
        {
            "name": "wave_gate",
            "description": "Retired by the ADR 0051 Mission Log cutover; always returns an error. An append-only Mission Log has no gate — use `harness mission log append` instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "wave_id": {"type": "string"},
                    "status": {"type": "string", "enum": ["accepted", "revise", "blocked"]},
                    "run_id": {"type": "string", "description": "Required when status is accepted."},
                    "accepted_by": {"type": "string", "description": "Defaults to host."},
                    "note": {"type": "string"},
                    "outcome": {"type": "string", "description": "Required when status is accepted."},
                    "artifact_refs": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["wave_id", "status"]
            }
        },
        {
            "name": "team_run_create",
            "description": "Create one runtime attempt from a required flat AgentTeam. Mission, ExecutionNode, and Project Binding are derived from the durable Team and selected execution context; members can come from the Team definition.",
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
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"company_id": {"type": "string", "minLength": 1}, "source_team_id": {"type": "string", "minLength": 1}, "target_team_id": {"type": "string", "minLength": 1}, "node_id": {"type": "string", "minLength": 1}, "state": {"type": "string", "enum": ["proposed", "awaiting_target_decision", "provisioning_target_work", "active", "result_available", "cancellation_requested", "terminal"]}, "limit": {"type": "integer", "minimum": 1, "maximum": 100}}, "required": ["company_id"]}
        },
        {
            "name": "collaboration_delegation_show",
            "description": "Read one canonical Company WorkDelegation with its cancellation requests and immutable remote publications.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"company_id": {"type": "string", "minLength": 1}, "delegation_id": {"type": "string", "minLength": 1}}, "required": ["company_id", "delegation_id"]}
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
            "description": "Add one idle member to an active planning/running/waiting TeamRun and optionally create a first Work. source_plan_ref is Host-plan provenance only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "initial_work": {"type": "string", "minLength": 1},
                    "source_plan_ref": {"type": "string", "description": "Optional Host-plan provenance only."},
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
            "name": "team_message_acknowledge",
            "description": "Acknowledge one delivery of a TeamMessageProjection for an explicit member or the reserved host recipient.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": {"type": "string"},
                    "member_id": {"type": "string", "description": "Recipient member-run id or `host`."}
                },
                "required": ["message_id", "member_id"]
            }
        },
        {
            "name": "team_run_list",
            "description": "List team runs in the store (latest projection, append order). One Execution Space store holds every tenant bound to it, so pass project_binding_id to see only one project's runs and status to drop finished ones. wave_index is derived by joining wave_id to the native Wave and is null when unresolved.",
            "inputSchema": {"type": "object", "properties": {
                "project_binding_id": {"type": "string", "description": "Return only runs bound to this project."},
                "status": {"type": "string", "description": "Return only runs in this status, for example running."}
            }}
        },
        {
            "name": "team_run_status",
            "description": "Show one team run: the run row, every member run with its latest MemberAction, provider PendingInteractions that are still pending (resolved history behind include_resolved; resolved_interactions always carries the count), compatibility field unacked_messages (the count of messages with at least one delivered manual_ack delivery awaiting acknowledgement), and the live dashboard URL.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "description": "Run id returned by team_run_create / team_run_list."},
                    "include_resolved": {"type": "boolean", "default": false, "description": "Include resolved PendingInteraction history; the unbounded resolved list is excluded by default."}
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
            "description": "Read Host mail across only those TeamRuns explicitly bound to one exact provider-native Host surface/thread. This is the safe Plugin/App integration path: it never leaks another Host task's inbox. By default returns actionable mail; all=true includes acknowledged history.",
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
            "description": "Read latest-wins Harness coordination mail addressed to one ProviderRuntimeProjection (or the reserved host recipient). By default returns actionable queued/delivered messages; all=true returns every received message at its latest stored state, not raw append revisions. Provider-native transcript and tool history are intentionally excluded.",
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
            "name": "team_run_send_message",
            "description": "Route one conversation message inside a team run and fold it into the run's event log. Durable responsibility lives on Work; pass work_id only to link the discussion. MCP Host calls default to sender_kind=host; external gateways must identify operator/service explicitly and may not impersonate a driven ProviderRuntimeProjection. Omit lineage fields for a fresh conversation correlation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "sender_runtime_id": {"type": "string", "description": "Compatibility sender projection. Use `host` for MCP Host calls; operator/service gateways provide their stable id here."},
                    "sender_kind": {"type": "string", "enum": ["host", "operator", "service", "agent_member"], "description": "Authenticated MCP actor provenance. Unbound MCP cannot author AgentMember messages for driven members; those originate from the bound provider runtime. The declared exception is a non-driven external_interactive member, whose user-driven session may self-author here and is recorded with authn_source=mcp:external_interactive."},
                    "sender_id": {"type": "string", "description": "Stable id of the typed sender; defaults to sender_runtime_id."},
                    "sender_name": {"type": "string"},
                    "recipient_runtime_ids": {"type": "array", "minItems": 1, "uniqueItems": true, "items": {"type": "string", "minLength": 1}, "description": "One or more recipient member run ids, or the reserved host recipient."},
                    "kind": {"type": "string", "enum": ["message", "handoff", "control"], "description": "Use `message` for planning, questions, answers, progress, blockers, review, broadcasts, and peer coordination. Work owns assignment and lifecycle."},
                    "body": {"type": "string"},
                    "work_id": {"type": "string", "description": "Optional Work discussed by this message. It must belong to the same TeamRun."},
                    "correlation_id": {"type": "string", "description": "Optional existing conversation correlation to reuse."},
                    "causation_id": {"type": "string", "description": "Optional earlier TeamMessageProjection id in this team run. When paired with correlation_id, it must carry that same correlation."}
                    ,"source_plan_ref": {"type": "string", "description": "Optional Host-plan provenance only; never a lifecycle boundary."}
                    ,"response_intent": {"type": "string", "enum": ["informational", "response_required"], "description": "Explicit response intent (ADR 0046 §4). Omit for the kind+sender default: handoff/control always require a response round; ordinary message mail from the coordination plane (host/operator/service) requires one too, while peer member-to-member message mail stays informational and never starts a provider round on its own."}
                },
                "required": ["team_run_id", "sender_runtime_id", "recipient_runtime_ids", "kind", "body"]
            }
        },
        {
            "name": "team_run_reconcile_delivery",
            "description": "Resolve one TeamMessageProjection delivery left in claimed state after a Supervisor crash. This never guesses provider consumption: choose provider_accepted=true with an audited provider_receipt_id, or requeue=true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "message_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "claim_id": {"type": "string"},
                    "provider_accepted": {"type": "boolean"},
                    "provider_receipt_id": {"type": "string"},
                    "requeue": {"type": "boolean"},
                    "reason": {"type": "string", "minLength": 1}
                },
                "required": ["team_run_id", "message_id", "member_run_id", "claim_id", "reason"]
            }
        },
        {
            "name": "team_run_resolve_interaction",
            "description": "Resolve a provider-originated interaction by legacy PendingInteraction id or provider_interaction_request TeamMessageProjection id. New responses are strict correlated TeamMessages, atomically ACK the request, and enter the provider only through an Inject delivery. Questions/reviews require host|lead, unknown requests operator|human, and tool/reject-only requests policy.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "interaction_id": {"type": "string"},
                    "option_id": {"type": "string", "description": "Exact option id exposed by the provider interaction."},
                    "response_text": {"type": "string", "description": "Free-form response when the provider contract supports it."},
                    "resolved_by": {"type": "string", "enum": ["host", "lead", "operator", "human", "policy"]}
                },
                "required": ["team_run_id", "interaction_id", "resolved_by"]
            }
        },
        {
            "name": "team_run_steer_member",
            "description": "Inject operator or Lead input into a currently active provider turn. This is capability-gated and currently requires codex_app_server; batch modes must use team_run_send_message for the next round.",
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
