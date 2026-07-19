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

use harness_core::{TeamDeliveryStatus, TeamRunEvent};
use harness_store::HarnessStore;
use serde_json::{json, Value};

use crate::{
    create_mission, create_team_run, create_wave, gate_wave, latest_member_runs_in_append_order,
    latest_team_messages_in_append_order, latest_team_run, latest_team_runs_in_append_order,
    parse_team_message_kind, parse_wave_executor_kind, send_team_message,
    visible_member_actions_in_append_order, TeamMemberSpec,
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
        "mission_create" => tool_mission_create(store, &arguments),
        "mission_list" => tool_mission_list(store),
        "wave_create" => tool_wave_create(store, &arguments),
        "wave_list" => tool_wave_list(store, &arguments),
        "wave_gate" => tool_wave_gate(store, &arguments),
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

fn optional_str(arguments: &Value, key: &str) -> Result<Option<String>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| format!("argument `{key}` must be a string or null")),
    }
}

fn optional_string_array(arguments: &Value, key: &str) -> Result<Vec<String>, String> {
    match arguments.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("argument `{key}[{index}]` must be a string"))
            })
            .collect(),
        Some(_) => Err(format!("argument `{key}` must be an array")),
    }
}

fn tool_mission_create(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let mission = create_mission(
        store,
        optional_str(arguments, "id")?,
        required_str(arguments, "title")?,
        required_str(arguments, "objective")?,
        optional_str(arguments, "desired_outcome")?,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!(mission))
}

fn tool_mission_list(store: &HarnessStore) -> Result<Value, String> {
    Ok(json!(store
        .mission_projections()
        .map_err(|error| error.to_string())?))
}

fn tool_wave_create(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let index = match arguments.get("index") {
        None => None,
        Some(value) => {
            let index = value
                .as_u64()
                .ok_or_else(|| "index must be a positive integer".to_string())?;
            Some(u32::try_from(index).map_err(|_| "index must fit a positive u32".to_string())?)
        }
    };
    let wave = create_wave(
        store,
        optional_str(arguments, "id")?,
        required_str(arguments, "mission_id")?,
        index,
        required_str(arguments, "title")?,
        required_str(arguments, "objective")?,
        parse_wave_executor_kind(required_str(arguments, "executor_kind")?)
            .map_err(|error| error.to_string())?,
        optional_str(arguments, "exit_criteria")?,
        optional_str(arguments, "plan_note")?,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!(wave))
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

fn tool_wave_gate(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let artifacts = optional_string_array(arguments, "artifact_refs")?;
    let wave = gate_wave(
        store,
        required_str(arguments, "wave_id")?,
        required_str(arguments, "status")?,
        optional_str(arguments, "run_id")?,
        optional_str(arguments, "accepted_by")?
            .as_deref()
            .unwrap_or("host"),
        optional_str(arguments, "note")?,
        optional_str(arguments, "outcome")?,
        artifacts,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!(wave))
}

/// `team_run_create` — journal a new run + member runs + assignment messages.
fn tool_team_run_create(store: &HarnessStore, arguments: &Value) -> Result<Value, String> {
    let objective = required_str(arguments, "objective")?;
    let wave_index = match arguments.get("wave_index") {
        None => 1,
        Some(value) => {
            let raw = value
                .as_u64()
                .ok_or_else(|| "wave_index must be a positive integer".to_string())?;
            u32::try_from(raw).map_err(|_| "wave_index must fit a positive u32".to_string())?
        }
    };
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
        .ok_or_else(|| "missing required array argument `members`".to_string())?;
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
            name: member_str("name")?.to_string(),
            role: member_str("role")?.to_string(),
            provider: member_str("provider")?.to_string(),
            model: optional_str(member, "model")?,
            owned_paths,
        });
    }
    let created = create_team_run(
        store,
        objective,
        wave_index,
        budget_limit_usd,
        "mcp",
        None,
        optional_str(arguments, "previous_run_id")?,
        optional_str(arguments, "mission_id")?,
        optional_str(arguments, "wave_id")?,
        &members,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "team_run_id": created.team_run.id,
        "member_run_ids": created.team_run.member_run_ids,
        "mission_id": created.team_run.mission_id,
        "wave_id": created.team_run.wave_id,
        "assignment_messages": created.assignment_messages,
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
        visible_member_actions_in_append_order(store).map_err(|error| error.to_string())?;
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
    let correlation_id = arguments
        .get("correlation_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let causation_id = arguments
        .get("causation_id")
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
        correlation_id,
        causation_id,
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

/// Mission/Wave authoring plus Agent Team tools. Descriptions ARE the interface
/// contract — the host model reads them to decide how to call each tool.
fn tool_definitions() -> Value {
    json!([
        {
            "name": "mission_create",
            "description": "Create a native durable Mission. Goal compatibility projections are read-only; use this for new Mission/Wave work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Optional stable Mission id; generated when omitted."},
                    "title": {"type": "string"},
                    "objective": {"type": "string"},
                    "desired_outcome": {"type": "string"}
                },
                "required": ["title", "objective"]
            }
        },
        {
            "name": "mission_list",
            "description": "List Mission projections with provenance. Native Mission rows and read-only Goal compatibility projections are both returned explicitly.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "wave_create",
            "description": "Create a lightweight ordered Wave for a native Mission. The Mission is updated with the Wave id; no Task Graph is created.",
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
                },
                "required": ["mission_id", "title", "objective", "executor_kind"]
            }
        },
        {
            "name": "wave_list",
            "description": "List latest native Wave rows, optionally limited to one Mission.",
            "inputSchema": {"type": "object", "properties": {"mission_id": {"type": "string"}}}
        },
        {
            "name": "wave_gate",
            "description": "Record a lightweight Wave gate. `accepted` requires an eligible executor run; for agent_team it must be a completed TeamRun linked to the same Mission/Wave. `revise` and `blocked` preserve attempts.",
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
            "description": "Create an Agent Team run with at least one member: journals the planning run, member rows, canonical queued Assignment messages, and events. Returns run/member ids and the Assignment messages with their correlation ids. Drive it with `harness team-run start`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "objective": {"type": "string", "minLength": 1, "description": "What the team should accomplish; also seeds each member's assignment message body."},
                    "wave_index": {"type": "integer", "minimum": 1, "description": "Compatibility wave number for unlinked runs (default 1); a native Wave supplies its own index."},
                    "budget_limit_usd": {"type": "number", "minimum": 0, "description": "Optional budget cap in USD, recorded on the run."},
                    "previous_run_id": {"type": "string", "description": "Optional previous attempt id. For a linked native Wave it must belong to the same Mission/Wave."},
                    "mission_id": {"type": "string", "description": "Optional durable Mission id. New native linkage requires a concrete wave_id; omit both ids only for an unlinked compatibility run."},
                    "wave_id": {"type": "string", "description": "Optional durable Wave id for this run. The run inherits that Wave's Mission when mission_id is omitted; a supplied mission_id must match."},
                    "members": {
                        "type": "array",
                        "description": "One entry per team member.",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string", "minLength": 1, "description": "Member display name, unique within the run."},
                                "role": {"type": "string", "minLength": 1, "description": "e.g. coordinator / implementer / reviewer."},
                                "provider": {"type": "string", "minLength": 1, "description": "Provider id (kimi is the v0 adapter)."},
                                "model": {"type": "string", "minLength": 1, "description": "Optional provider model override."},
                                "owned_paths": {"type": "array", "items": {"type": "string", "minLength": 1}, "description": "Paths this member exclusively owns."}
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
            "description": "Route one message inside a team run and fold it into the run's event log. `from_member_id` may be a member run id or the reserved sender `host`. Omit lineage fields for a fresh opaque correlation; to reuse an assignment's ownership correlation, pass that assignment's `correlation_id` (and optionally its message id as `causation_id`). Returns the new message id and its correlation id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "from_member_id": {"type": "string", "description": "Sender: a member run id, or `host`."},
                    "to_member_ids": {"type": "array", "minItems": 1, "uniqueItems": true, "items": {"type": "string", "minLength": 1}, "description": "One or more recipient member run ids, or the reserved host recipient."},
                    "kind": {"type": "string", "enum": ["assignment", "question", "answer", "progress", "blocker", "handoff", "review_request", "review_result", "control", "broadcast"]},
                    "body": {"type": "string"},
                    "task_id": {"type": "string", "description": "Optional legacy task this message refers to."},
                    "correlation_id": {"type": "string", "description": "Optional assignment correlation to reuse. For a non-assignment message, it must identify an Assignment in this team run."},
                    "causation_id": {"type": "string", "description": "Optional earlier TeamMessage id in this team run. When paired with correlation_id, it must carry that same correlation."}
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
