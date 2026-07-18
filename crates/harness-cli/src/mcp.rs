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
//! - `tools/list` → the five Agent Team v0 tools.
//! - `tools/call` → `{content:[{type:"text",text:<result JSON>}], isError}`.
//! - unknown method → JSON-RPC -32601. stdin EOF exits.

use std::io::{BufRead, Write};

use harness_core::{TeamDeliveryStatus, TeamRunEvent};
use harness_store::HarnessStore;
use serde_json::{json, Value};

use crate::{
    create_team_run, latest_member_actions_in_append_order, latest_member_runs_in_append_order,
    latest_team_messages_in_append_order, latest_team_run, latest_team_runs_in_append_order,
    parse_team_message_kind, send_team_message, TeamMemberSpec,
};

/// MCP protocol revision this server speaks, echoed verbatim in `initialize`
/// (the simple end of "reply with the client's version or the lower one").
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Where the default `harness serve` surface renders the Agent Team console;
/// the tools return it so the host can point a human at the live view.
const DASHBOARD_URL: &str = "http://127.0.0.1:8787/team-console";

/// Serve the stdio MCP loop until stdin closes.
pub fn run(store: &HarnessStore) -> crate::CliResult<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = handle_line(store, trimmed) {
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Handle one JSON-RPC line. Returns `None` for notifications (including
/// `notifications/initialized`): they are accepted and otherwise ignored.
fn handle_line(store: &HarnessStore, line: &str) -> Option<Value> {
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
        "tools/call" => call_tool(store, &params),
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
fn call_tool(store: &HarnessStore, params: &Value) -> Result<Value, (i64, String)> {
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
        "team_run_create" => tool_team_run_create(store, &arguments),
        "team_run_list" => tool_team_run_list(store),
        "team_run_status" => tool_team_run_status(store, &arguments),
        "team_run_send_message" => tool_team_run_send_message(store, &arguments),
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

/// Read a required string argument, or the tool-error message.
fn required_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string argument `{key}`"))
}

/// `team_run_create` — journal a new run + member runs + assignment messages.
fn tool_team_run_create(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let objective = required_str(arguments, "objective")?;
    let wave_index = arguments
        .get("wave_index")
        .and_then(Value::as_u64)
        .unwrap_or(1) as u32;
    let budget_limit_usd = arguments.get("budget_limit_usd").and_then(Value::as_f64);
    let member_values = arguments
        .get("members")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing required array argument `members`".to_string())?;
    let mut members = Vec::new();
    for (index, member) in member_values.iter().enumerate() {
        let member_str = |key: &str| {
            member
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("members[{index}].{key} must be a string"))
        };
        members.push(TeamMemberSpec {
            name: member_str("name")?.to_string(),
            role: member_str("role")?.to_string(),
            provider: member_str("provider")?.to_string(),
            model: member
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            owned_paths: member
                .get("owned_paths")
                .and_then(Value::as_array)
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    let created = create_team_run(
        store,
        objective,
        wave_index,
        budget_limit_usd,
        "mcp",
        None,
        arguments
            .get("previous_run_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        &members,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "team_run_id": created.team_run.id,
        "member_run_ids": created.team_run.member_run_ids,
        "dashboard_url": DASHBOARD_URL,
    }))
}

/// `team_run_list` — the latest projection of every run, trimmed to the
/// fields a host needs to pick one.
fn tool_team_run_list(store: &HarnessStore) -> Result<Value, String> {
    let runs = latest_team_runs_in_append_order(store).map_err(|error| error.to_string())?;
    Ok(Value::Array(
        runs.iter()
            .map(|run| {
                json!({
                    "id": run.id,
                    "objective": run.objective,
                    "status": run.status,
                    "wave_index": run.wave_index,
                    "member_count": run.member_run_ids.len(),
                    "created_at": run.created_at,
                })
            })
            .collect(),
    ))
}

/// `team_run_status` — one run with its members (each carrying the latest
/// MemberAction, if any), the count of not-yet-acknowledged messages, and the
/// dashboard URL. Mirrors the `team-run status --json` projection.
fn tool_team_run_status(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let id = required_str(arguments, "team_run_id")?;
    let run = latest_team_run(store, id).map_err(|error| error.to_string())?;
    let member_runs: Vec<_> = latest_member_runs_in_append_order(store)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|member| member.team_run_id == id)
        .collect();
    let actions =
        latest_member_actions_in_append_order(store).map_err(|error| error.to_string())?;
    let messages =
        latest_team_messages_in_append_order(store).map_err(|error| error.to_string())?;
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
        .filter(|message| {
            message
                .deliveries
                .iter()
                .any(|delivery| delivery.status != TeamDeliveryStatus::Acknowledged)
        })
        .count();
    Ok(json!({
        "team_run": run,
        "members": members,
        "unacked_messages": unacked_messages,
        "dashboard_url": DASHBOARD_URL,
    }))
}

/// `team_run_send_message` — route one message inside a run and fold it into
/// the event log. `from_member_id` may be the reserved sender `host`.
fn tool_team_run_send_message(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let from_member_id = required_str(arguments, "from_member_id")?;
    let to_member_ids: Vec<String> = arguments
        .get("to_member_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing required array argument `to_member_ids`".to_string())?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    if to_member_ids.is_empty() {
        return Err("`to_member_ids` must name at least one member id".to_string());
    }
    let kind = parse_team_message_kind(required_str(arguments, "kind")?)
        .map_err(|error| error.to_string())?;
    let body = required_str(arguments, "body")?;
    let task_id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let message = send_team_message(
        store,
        team_run_id,
        from_member_id,
        to_member_ids,
        kind,
        body,
        task_id,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "message_id": message.id,
        "correlation_id": message.correlation_id,
    }))
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

/// The five Agent Team v0 tools. Descriptions ARE the interface contract —
/// the host model reads them to decide how to call each tool.
fn tool_definitions() -> Value {
    json!([
        {
            "name": "team_run_create",
            "description": "Create an Agent Team v0 run: journals the team run (status `planning`), one idle member run per member, one queued assignment message per member from the reserved `host` sender, and folded team-run events. Returns the run id, member run ids, and the live dashboard URL. Drive the run afterwards with `harness team-run start` (the kimi-over-ACP adapter is the v0 member runtime).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "objective": {"type": "string", "description": "What the team should accomplish; also seeds each member's assignment message body."},
                    "wave_index": {"type": "integer", "description": "Wave number of this run (default 1)."},
                    "budget_limit_usd": {"type": "number", "description": "Optional budget cap in USD, recorded on the run."},
                    "previous_run_id": {"type": "string", "description": "Optional id of the previous wave's run; chains this run onto that lineage (re-plan of an earlier wave)."},
                    "members": {
                        "type": "array",
                        "description": "One entry per team member.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string", "description": "Member display name, unique within the run."},
                                "role": {"type": "string", "description": "e.g. coordinator / implementer / reviewer."},
                                "provider": {"type": "string", "description": "Provider id (kimi is the v0 adapter)."},
                                "model": {"type": "string", "description": "Optional provider model override."},
                                "owned_paths": {"type": "array", "items": {"type": "string"}, "description": "Paths this member exclusively owns."}
                            },
                            "required": ["name", "role", "provider"]
                        }
                    }
                },
                "required": ["objective", "members"]
            }
        },
        {
            "name": "team_run_list",
            "description": "List every team run in the store (latest projection, append order), trimmed to id/objective/status/wave_index/member_count/created_at. Use it to find a run id for the other tools.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "team_run_status",
            "description": "Show one team run: the run row, every member run with its latest MemberAction (null when the member has not acted yet), the count of messages with at least one unacknowledged delivery, and the live dashboard URL.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "description": "Run id returned by team_run_create / team_run_list."}
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_send_message",
            "description": "Route one message inside a team run and fold it into the run's event log. `from_member_id` may be a member run id or the reserved sender `host`. Returns the new message id and its correlation id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "from_member_id": {"type": "string", "description": "Sender: a member run id, or `host`."},
                    "to_member_ids": {"type": "array", "items": {"type": "string"}, "description": "One or more recipient member run ids."},
                    "kind": {"type": "string", "enum": ["assignment", "question", "answer", "progress", "blocker", "handoff", "review_request", "review_result", "control", "broadcast"]},
                    "body": {"type": "string"},
                    "task_id": {"type": "string", "description": "Optional task this message refers to."}
                },
                "required": ["team_run_id", "from_member_id", "to_member_ids", "kind", "body"]
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
