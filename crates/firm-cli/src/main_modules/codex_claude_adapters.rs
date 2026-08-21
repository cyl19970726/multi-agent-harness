use super::*;

// ============================================================================
// WP-3: Claude stream-json event parser and delivery (replaces stub)
// ============================================================================

/// Represents a single event from `claude -p --output-format stream-json --verbose` NDJSON stream.
/// Stream-json format emits: system (init), stream_event (message lifecycle), result (terminal).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ClaudeStreamEvent {
    /// Event type: "system", "stream_event", "result"
    pub(super) event_type: String,
    /// Raw JSON payload for extraction
    pub(super) payload: serde_json::Value,
}

impl ClaudeStreamEvent {
    /// Parse one NDJSON line into a ClaudeStreamEvent if valid, else None (skip).
    pub(super) fn parse_line(line: &str) -> Option<ClaudeStreamEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(payload) => {
                let event_type = payload
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(ClaudeStreamEvent {
                    event_type,
                    payload,
                })
            }
            Err(_) => None,
        }
    }

    /// Extract session_id from system init event.
    pub(super) fn session_id(&self) -> Option<String> {
        if self.event_type == "system" {
            self.payload
                .get("session_id")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

/// Infer provider execution status from Claude stream-json events.
pub(super) fn infer_claude_session_status(
    events: &[ClaudeStreamEvent],
    process_success: bool,
) -> ProviderExecutionStatus {
    if !process_success {
        return ProviderExecutionStatus::Failed;
    }
    let has_result = events.iter().any(|e| e.event_type == "result");
    if has_result {
        if let Some(result_event) = events.iter().find(|e| e.event_type == "result") {
            if result_event.payload.get("error").is_some()
                || result_event
                    .payload
                    .get("is_error")
                    .and_then(|value| value.as_bool())
                    == Some(true)
                || result_event.payload.get("api_error_status").is_some()
            {
                return ProviderExecutionStatus::Failed;
            }
        }
        ProviderExecutionStatus::Succeeded
    } else if events.is_empty() {
        ProviderExecutionStatus::Failed
    } else {
        ProviderExecutionStatus::Stale
    }
}

/// Extract session_id from Claude stream events.
pub(super) fn extract_session_id_from_claude_events(
    events: &[ClaudeStreamEvent],
) -> Option<String> {
    events.iter().find_map(|e| e.session_id())
}

/// Extract the assistant's ACTUAL reply text from a `claude -p
/// --output-format stream-json` stream, so the delivery report surfaces what
/// the agent said rather than a meta event count. Prefers the terminal
/// `result` event's `result` field; falls back to concatenating the text
/// blocks of `assistant` messages. Returns None when the turn produced no
/// assistant text (e.g. tool-only), letting the caller keep a status summary.
pub(super) fn extract_claude_reply_text(events: &[ClaudeStreamEvent]) -> Option<String> {
    // The terminal result event carries the final assistant text.
    for event in events.iter().rev() {
        if event.event_type != "result" {
            continue;
        }
        if let Some(text) = event.payload.get("result").and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    // Fallback: concatenate text blocks from assistant messages in order.
    let mut parts = Vec::new();
    for event in events {
        if event.event_type != "assistant" {
            continue;
        }
        let Some(content) = event
            .payload
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                if !text.trim().is_empty() {
                    parts.push(text.trim().to_string());
                }
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Map ProviderExecutionStatus to terminal source.
pub(super) fn status_to_terminal_source(
    status: &ProviderExecutionStatus,
) -> Option<MessageTerminalSource> {
    match status {
        ProviderExecutionStatus::Succeeded => Some(MessageTerminalSource::TurnCompleted),
        ProviderExecutionStatus::Failed => Some(MessageTerminalSource::Failed),
        _ => None,
    }
}

// --- Codex exec --json delivery (WP-2) ---
// Parse the short-lived transport stream in memory. Harness records only the
// delivery outcome and NativeSessionRef; Codex owns the durable item history.

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CodexExecEvent {
    /// Event discriminant extracted from NDJSON payload.
    pub(super) event_type: String,
    /// Raw JSON payload for extraction.
    pub(super) payload: serde_json::Value,
}

impl CodexExecEvent {
    /// Parse one NDJSON line into a CodexExecEvent if valid, else None (skip).
    pub(super) fn parse_line(line: &str) -> Option<CodexExecEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(payload) => {
                let event_type = payload
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(CodexExecEvent {
                    event_type,
                    payload,
                })
            }
            Err(_) => None,
        }
    }

    /// Extract the terminal source from this event if it is a completion event.
    pub(super) fn terminal_source(&self) -> Option<MessageTerminalSource> {
        if codex_event_is_terminal(&self.event_type) {
            Some(MessageTerminalSource::TurnCompleted)
        } else {
            None
        }
    }
}

/// True when a codex exec event type marks the end of a turn/thread.
///
/// Codex 0.13x `exec --json` emits dot-separated discriminants
/// (`turn.completed`, `thread.idle`). Older notes used underscore names
/// (`turn_completed`, `thread_idle`); both are accepted so the parser is
/// robust across codex versions.
pub(super) fn codex_event_is_terminal(event_type: &str) -> bool {
    matches!(
        event_type,
        "turn.completed" | "thread.idle" | "turn_completed" | "thread_idle"
    )
}

/// Parse NDJSON from codex exec stdout into CodexExecEvent stream.
/// Resilient: silently skip invalid lines, partial final lines, unknown events.
// Thin no-tee wrapper; only the unit tests use it now (the delivery path uses
// the callback form), so it is dead in the binary target.
#[allow(dead_code)]
pub(super) fn parse_codex_ndjson(reader: impl BufRead) -> Vec<CodexExecEvent> {
    parse_codex_ndjson_to(reader, None::<fn(&serde_json::Value)>)
}

/// Like `parse_codex_ndjson`, but invokes `on_event` with each parsed event's
/// payload AS IT IS READ — used to tee codex events MID-TURN to the session
/// NDJSON (poll) and the shared turn-events file (live SSE), mirroring the
/// claude path. The returned Vec is identical to the no-callback path.
pub(super) fn parse_codex_ndjson_to<F: FnMut(&serde_json::Value)>(
    reader: impl BufRead,
    mut on_event: Option<F>,
) -> Vec<CodexExecEvent> {
    let mut events = Vec::new();
    for line in reader.lines() {
        let Ok(line_str) = line else { continue };
        if let Some(event) = CodexExecEvent::parse_line(&line_str) {
            if let Some(callback) = on_event.as_mut() {
                callback(&event.payload);
            }
            events.push(event);
        }
    }
    events
}

/// Infer the lifecycle status from a stream of CodexExecEvent.
/// Follows the same logic as the app-server path: queued → running → (succeeded|failed).
pub(super) fn infer_provider_execution_status(
    events: &[CodexExecEvent],
    process_success: bool,
) -> ProviderExecutionStatus {
    if !process_success {
        return ProviderExecutionStatus::Failed;
    }
    // If we saw a terminal event, we succeeded.
    let has_terminal = events
        .iter()
        .any(|e| codex_event_is_terminal(&e.event_type));
    if has_terminal {
        ProviderExecutionStatus::Succeeded
    } else if events.is_empty() {
        ProviderExecutionStatus::Failed
    } else {
        // We have events but no terminal: stale (timed out waiting for completion).
        ProviderExecutionStatus::Stale
    }
}

/// Extract provider_thread_id from the exec output events if present.
///
/// Codex `exec --json` emits a `thread.started` event carrying the real
/// `thread_id` (e.g. `{"thread_id":"019e...","type":"thread.started"}`). We
/// scan every event payload for a top-level `thread_id` string and return the
/// first match so the provider execution attempt records the provider's real thread id.
pub(super) fn extract_thread_id_from_exec_events(events: &[CodexExecEvent]) -> Option<String> {
    events.iter().find_map(|event| {
        event
            .payload
            .get("thread_id")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    })
}

/// Extract provider_turn_id from the exec output events if present.
///
/// Newer codex builds may attach a `turn_id` to turn lifecycle events. When
/// present we surface it; otherwise None (the harness session id scopes the
/// turn). We accept either a top-level `turn_id` or one nested under `turn`.
pub(super) fn extract_turn_id_from_exec_events(events: &[CodexExecEvent]) -> Option<String> {
    events.iter().find_map(|event| {
        event
            .payload
            .get("turn_id")
            .and_then(|value| value.as_str())
            .or_else(|| {
                event
                    .payload
                    .get("turn")
                    .and_then(|turn| turn.get("id"))
                    .and_then(|value| value.as_str())
            })
            .map(|value| value.to_string())
    })
}

/// Extract the agent's ACTUAL reply text from a `codex exec --json` stream, so
/// the delivery report surfaces what the agent said rather than a meta status
/// line. Codex emits `item.completed` events whose `item.type` is
/// `agent_message` and whose `item.text` is the assistant's prose; concatenate
/// them in order. Returns None when the turn produced no agent message (e.g.
/// command-only), letting the caller keep a status summary.
pub(super) fn extract_codex_reply_text(events: &[CodexExecEvent]) -> Option<String> {
    let mut parts = Vec::new();
    for event in events {
        let Some(item) = event.payload.get("item") else {
            continue;
        };
        if item.get("type").and_then(|t| t.as_str()) != Some("agent_message") {
            continue;
        }
        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
            if !text.trim().is_empty() {
                parts.push(text.trim().to_string());
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// The codex turn's FINAL assistant message — the LAST non-empty `agent_message`
/// item. Where [`extract_codex_reply_text`] concatenates every message for the
/// human-facing reply, this returns only the terminal one, so structured-output
/// parsing reads the schema-constrained answer rather than an earlier streamed
/// preamble (issue #139 item 2).
pub(super) fn extract_codex_final_message(events: &[CodexExecEvent]) -> Option<String> {
    let mut last = None;
    for event in events {
        let Some(item) = event.payload.get("item") else {
            continue;
        };
        if item.get("type").and_then(|t| t.as_str()) != Some("agent_message") {
            continue;
        }
        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
            if !text.trim().is_empty() {
                last = Some(text.trim().to_string());
            }
        }
    }
    last
}

/// Write a temporary MCP config JSON file for Claude.
/// Returns the path to the temporary file, or None if mcp is empty/None.
pub(super) fn write_temp_mcp_config(mcp: Option<&LaunchMcp>) -> CliResult<Option<String>> {
    if let Some(mcp_config) = mcp {
        if mcp_config.servers.is_empty() {
            return Ok(None);
        }

        // Build MCP servers config as expected by Claude
        let mut servers = serde_json::Map::new();
        for server in &mcp_config.servers {
            let mut server_obj = serde_json::Map::new();
            server_obj.insert("id".to_string(), serde_json::json!(server.id));

            if let Some(transport) = &server.transport {
                server_obj.insert("transport".to_string(), serde_json::json!(transport));
            }

            if !server.command.is_empty() {
                server_obj.insert("command".to_string(), serde_json::json!(server.command));
            }

            if let Some(url) = &server.url {
                server_obj.insert("url".to_string(), serde_json::json!(url));
            }

            if !server.allowed_tools.is_empty() {
                server_obj.insert(
                    "allowed_tools".to_string(),
                    serde_json::json!(server.allowed_tools),
                );
            }

            servers.insert(server.id.clone(), serde_json::Value::Object(server_obj));
        }

        let config = serde_json::json!({
            "mcp_servers": servers
        });

        // Write to temp file
        let config_str = serde_json::to_string(&config)
            .map_err(|e| CliError::Usage(format!("failed to serialize MCP config: {e}")))?;

        let temp_path =
            std::env::temp_dir().join(format!("mcp_config_{}.json", std::process::id()));
        let temp_path_str = temp_path.to_string_lossy().to_string();

        std::fs::write(&temp_path, config_str).map_err(|e| {
            CliError::Usage(format!("failed to write MCP config to temp file: {e}"))
        })?;

        Ok(Some(temp_path_str))
    } else {
        Ok(None)
    }
}

pub(super) fn run_codex_exec_process(
    session_dir: &Path,
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<CodexExecDeliveryRun> {
    // Build the command: `codex exec --json <prompt>`
    // The LaunchSpec is composed from the member/message; the exec arg is the message_content.
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    let developer_instructions = provider_developer_instructions(member);
    // cwd precedence (P3, Stage 3): member.provider_cwd_hint → selected
    // project.project_root → process cwd. Codex discovers AGENTS.md from its cwd,
    // so a `serve` that switched projects must still spawn here in the project root.
    let cwd = delivery_worker_cwd(member, project);

    // Build LaunchSpec from member and message
    let spec = build_launch_spec(member, message);

    let mut cmd = Command::new("codex");
    cmd.arg("exec");

    // Resume an existing session when the member already carries a provider
    // thread id (from a prior delivery). `codex exec resume <id>` continues the
    // same conversation so memory carries across deliveries. The resume
    // subcommand inherits the original session's sandbox / working roots and
    // does not accept `--sandbox` / `-C` / `--add-dir`, so those are only mapped
    // on the fresh-session path below.
    let resuming = spec.resume.is_some();
    if let Some(resume_id) = &spec.resume {
        cmd.arg("resume")
            .arg("--json")
            .arg(resume_id)
            .arg(&message_content);
    } else {
        cmd.arg("--json").arg(&message_content);
    }
    cmd.env("CODEX_DEVELOPER_INSTRUCTIONS", developer_instructions);

    // Map LaunchSpec to codex flags
    apply_codex_model_and_effort_args(&mut cmd, &spec);
    apply_codex_output_schema_arg(&mut cmd, &spec, session_dir)?;
    apply_codex_mcp_args(&mut cmd, &spec)?;

    if !resuming {
        // Map permission to sandbox (fresh sessions only).
        let sandbox = CodexAdapter.map_permission(spec.permission);
        cmd.arg("--sandbox").arg(sandbox);

        // Map workspace and writable roots (fresh sessions only).
        if let Some(workspace) = &spec.workspace {
            cmd.arg("-C").arg(workspace);
        }
        for root in &spec.writable_roots {
            cmd.arg("--add-dir").arg(root);
        }
    }

    cmd.current_dir(&cwd);

    let run = run_ndjson_child(
        cmd,
        session_dir,
        delivery_id,
        "codex.stream-json.ndjson",
        timeout_ms,
        None,
        None,
        "codex exec",
    )?;
    let events = run
        .events
        .iter()
        .filter_map(|payload| serde_json::to_string(payload).ok())
        .filter_map(|line| CodexExecEvent::parse_line(&line))
        .collect();

    Ok((run.process_success, events, run.events, run.stderr))
}

pub(super) fn apply_codex_model_and_effort_args(cmd: &mut Command, spec: &LaunchSpec) {
    if let Some(model) = &spec.model {
        cmd.arg("-m").arg(model);
    }
    // Reasoning effort: codex takes it as a config override (no dedicated flag).
    if let Some(effort) = &spec.effort {
        cmd.arg("-c")
            .arg(format!("model_reasoning_effort={effort}"));
    }
}

pub(super) fn apply_codex_output_schema_arg(
    cmd: &mut Command,
    spec: &LaunchSpec,
    session_dir: &Path,
) -> CliResult<()> {
    if let Some(schema) = &spec.output_schema {
        let schema_path = session_dir.join("output-schema.json");
        let schema_json = schema_to_json_schema(schema);
        fs::write(&schema_path, schema_json.to_string()).map_err(|e| {
            CliError::Usage(format!(
                "failed to write codex output schema to {}: {e}",
                schema_path.display()
            ))
        })?;
        cmd.arg("--output-schema").arg(&schema_path);
    }
    Ok(())
}

pub(super) fn apply_codex_mcp_args(cmd: &mut Command, spec: &LaunchSpec) -> CliResult<()> {
    let Some(mcp) = &spec.mcp else {
        return Ok(());
    };

    for server in &mcp.servers {
        let id_key = codex_mcp_id_key(&server.id);
        if !server.command.is_empty() {
            // Codex stdio MCP config stores the binary separately from argv rest.
            let bin = serde_json::to_string(&server.command[0])
                .map_err(|e| CliError::Usage(format!("mcp command serialize: {e}")))?;
            cmd.arg("-c")
                .arg(format!("mcp_servers.{id_key}.command={bin}"));
            if server.command.len() > 1 {
                let args = serde_json::to_string(&server.command[1..])
                    .map_err(|e| CliError::Usage(format!("mcp args serialize: {e}")))?;
                cmd.arg("-c")
                    .arg(format!("mcp_servers.{id_key}.args={args}"));
            }
        } else if let Some(url) = &server.url {
            let u = serde_json::to_string(url)
                .map_err(|e| CliError::Usage(format!("mcp url serialize: {e}")))?;
            cmd.arg("-c").arg(format!("mcp_servers.{id_key}.url={u}"));
        }
        // Codex's mcp_servers schema has no allowed_tools field, so the neutral
        // allowlist is intentionally not mapped; transport is implied by
        // command-vs-url.
    }

    Ok(())
}

pub(super) fn codex_mcp_id_key(id: &str) -> String {
    if !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        id.to_string()
    } else {
        serde_json::to_string(id).expect("serializing string key should not fail")
    }
}

/// This is the exec-stream variant of run_codex_app_server_exchange.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_codex_exec_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    _runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<DeliveryOutcome> {
    let session_dir = store
        .root()
        .join("runtimes")
        .join("deliveries")
        .join(delivery_id);
    fs::create_dir_all(&session_dir)?;
    let spec = build_launch_spec(member, message);

    let (process_success, events, raw_events, _stderr_log) = run_codex_exec_process(
        &session_dir,
        member,
        message,
        delivery_id,
        timeout_ms,
        project,
    )?;
    let (tokens, cost_usd, model) = codex_delivery_telemetry(&raw_events, &spec);

    // Infer the delivery status from events and process exit.
    let status = infer_provider_execution_status(&events, process_success);
    let terminal_source = if matches!(status, ProviderExecutionStatus::Succeeded) {
        events
            .iter()
            .find_map(|e| e.terminal_source())
            .or(Some(MessageTerminalSource::Unknown))
    } else {
        Some(MessageTerminalSource::Failed)
    };

    let provider_thread_id = extract_thread_id_from_exec_events(&events);
    let provider_turn_id = extract_turn_id_from_exec_events(&events);
    let exit_code = if process_success { Some(0) } else { Some(1) };
    let reply = extract_codex_reply_text(&events);
    let structured =
        structured_for_status(&status, codex_delivery_structured(reply.as_deref(), &spec));

    let summary = match status {
        ProviderExecutionStatus::Succeeded => {
            "Codex provider delivery completed; transcript remains provider-native".into()
        }
        ProviderExecutionStatus::Failed => {
            "Codex provider delivery failed; inspect the provider-native session".into()
        }
        ProviderExecutionStatus::Stale => {
            "Codex exec --json produced output but did not complete before timeout".into()
        }
        _ => "Codex exec --json session ended".into(),
    };

    let _ = fs::remove_dir_all(&session_dir);
    let native_session = provider_thread_id
        .as_ref()
        .map(|id| provider_native_session_ref("codex", id));
    Ok(DeliveryOutcome {
        status: status.clone(),
        native_session,
        provider_thread_id,
        provider_turn_id,
        terminal_source,
        provider_request_id: None, // exec stream does not use request_id
        exit_code,
        tokens,
        cost_usd,
        model,
        structured,
        response_text: reply,
        summary,
    })
}

/// Run a single message delivery against the member's runtime, routed by provider.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_provider_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<DeliveryOutcome> {
    match provider_adapter(&member.provider) {
        Some(adapter) => adapter.run_delivery(
            store,
            member,
            runtime,
            message,
            delivery_id,
            timeout_ms,
            project,
        ),
        None => Err(unknown_provider_error(&member.provider, "delivery")),
    }
}

// WP-5: Codex exec-stream runtime (no persistent process).
// Each delivery spawns `codex exec --json`, so no app-server socket is needed.

pub(super) type CodexExecDeliveryRun = (bool, Vec<CodexExecEvent>, Vec<serde_json::Value>, String);
pub(super) type ClaudeDeliveryRun = (
    bool,
    Vec<ClaudeStreamEvent>,
    Vec<serde_json::Value>,
    Option<String>,
    String,
);

pub(super) fn start_codex_exec_runtime(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
) -> CliResult<ProviderProcess> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;

    // For Codex, we use exec-stream delivery (no persistent app-server).
    // Each delivery spawns `codex exec --json`, so there's no long-lived process.
    // The control_endpoint is a marker for the runtime directory.
    let endpoint = format!("codex-exec-runtime://{}", runtime_dir.display());

    let args = vec![
        // Codex will be spawned on each delivery via codex exec --json
    ];

    // Check if codex binary is available
    let process_alive = Command::new("which")
        .arg("codex")
        .output()
        .ok()
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(ProviderProcess {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: ProviderProcessStatus::Running,
        pid: None, // Codex exec runs on-demand; no persistent PID
        control_endpoint: Some(endpoint),
        command: "codex".into(),
        args,
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: ProviderProcessHealth {
            process_alive,
            socket_exists: true,                        // Runtime dir exists
            protocol_probe: Some("exec-stream".into()), // Codex uses exec-stream
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

// --- Claude runtime (BE-WP7) ---
// The claude CLI shape: spawn the claude binary as a local process, run message
// delivery exchanges via stdin/stdout, record sessions and evidence.

pub(super) fn start_claude_runtime(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
) -> CliResult<ProviderProcess> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;

    // For Claude CLI, we don't spawn a persistent process on runtime start.
    // Instead, we record the runtime as "ready" and each delivery will spawn
    // claude with the message. This matches the behavior of claude as a
    // request-response tool rather than a persistent app-server.
    // The control_endpoint is a marker for the runtime directory.
    let endpoint = format!("claude-runtime://{}", runtime_dir.display());

    let args = vec![
        // Claude CLI will be spawned on each delivery with the message prompt
    ];

    // Check if claude binary is available, but don't require it at test time
    let process_alive = Command::new("which")
        .arg("claude")
        .output()
        .ok()
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(ProviderProcess {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: ProviderProcessStatus::Running,
        pid: None, // Claude runs on-demand; no persistent PID
        control_endpoint: Some(endpoint),
        command: "claude".into(),
        args,
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: ProviderProcessHealth {
            process_alive,
            socket_exists: true,                    // Runtime dir exists
            protocol_probe: Some("unknown".into()), // Will probe on first delivery
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_claude_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    _runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<DeliveryOutcome> {
    let session_dir = store
        .root()
        .join("runtimes")
        .join("deliveries")
        .join(delivery_id);
    fs::create_dir_all(&session_dir)?;

    // WP-3: Spawn real `claude -p --output-format stream-json --verbose`.
    //
    // Opt-in resident path (HARNESS_CLAUDE_RESIDENT=1): instead of spawning a
    // fresh `claude -p <prompt>` that exits per turn, hold a `claude
    // --input-format stream-json` process open and feed the turn as a stdin
    // frame (see `resident.rs`). The returned tuple shape is identical to the
    // default path, so status inference and telemetry stay provider-neutral.
    let resident = env::var("HARNESS_CLAUDE_RESIDENT").as_deref() == Ok("1");
    let (process_success, events, raw_events, session_id, _stderr_log) = if resident {
        run_claude_resident_delivery_real(&session_dir, member, message, timeout_ms, project)?
    } else {
        run_claude_exec_delivery_real(&session_dir, member, message, timeout_ms, project)?
    };
    let (tokens, cost_usd, model, raw_structured) = claude_delivery_telemetry(&raw_events);

    let status = infer_claude_session_status(&events, process_success);
    let structured = structured_for_status(&status, raw_structured);
    let terminal_source = status_to_terminal_source(&status);
    // The id we hand back as the member's provider thread for the NEXT delivery
    // to resume. Only a real session id parsed from the provider output is
    // resumable; the synthetic fallback id above is not, so it is not surfaced
    // as a resume token.
    let resumable_session_id = session_id.clone();

    let _ = fs::remove_dir_all(&session_dir);
    Ok(DeliveryOutcome {
        native_session: resumable_session_id
            .as_ref()
            .map(|id| provider_native_session_ref("claude", id)),
        // Surface the real claude session id as the member's provider thread so
        // the next delivery resumes this conversation (memory across deliveries).
        provider_thread_id: resumable_session_id,
        provider_turn_id: None,
        terminal_source,
        status,
        provider_request_id: None,
        exit_code: if process_success { Some(0) } else { Some(1) },
        tokens,
        cost_usd,
        model,
        structured,
        response_text: process_success
            .then(|| extract_claude_reply_text(&events))
            .flatten(),
        summary: if process_success {
            "Claude provider delivery completed; transcript remains provider-native".to_string()
        } else {
            "Claude provider delivery failed; inspect the provider-native session".to_string()
        },
    })
}

/// Spawn `claude -p --output-format stream-json --verbose` and parse NDJSON output.
/// WP-3: Real implementation replacing the stub; parses session_id and events.
pub(super) fn run_claude_exec_delivery_real(
    session_dir: &Path,
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<ClaudeDeliveryRun> {
    // Build the message content envelope (harness context).
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    // Compose system prompt (developer instructions from member prompt_ref).
    let system_prompt = provider_developer_instructions(member);

    // cwd precedence (P3, Stage 3): member.provider_cwd_hint → selected
    // project.project_root → process cwd. Claude Code discovers CLAUDE.md /
    // .claude/ (and keys per-project memory) from its cwd, so a `serve` that
    // switched projects must still spawn here in the selected project root.
    let cwd = delivery_worker_cwd(member, project);

    // Build LaunchSpec from member and message
    let spec = build_launch_spec(member, message);

    // Build command: claude -p "<message_content>" --output-format stream-json --verbose
    // plus mapped flags from launch spec.
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg(&message_content)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose");

    // Resume an existing session when the member already carries a provider
    // session id (from a prior delivery). `claude -p --resume <session_id>`
    // continues the same conversation so memory carries across deliveries.
    if let Some(resume_id) = &spec.resume {
        cmd.arg("--resume").arg(resume_id);
    }

    // Append system prompt if present.
    if !system_prompt.is_empty() {
        cmd.arg("--append-system-prompt").arg(&system_prompt);
    }

    // Map LaunchSpec to claude flags
    // Model selection
    apply_claude_model_and_effort_args(&mut cmd, &spec);
    apply_claude_output_schema_arg(&mut cmd, &spec);

    // Permission mapping
    let permission_mode = ClaudeAdapter.map_permission(spec.permission);
    cmd.arg("--permission-mode").arg(permission_mode);

    // Tools (allowed-tools if spec.tools is non-empty)
    if !spec.tools.is_empty() {
        let tools_arg = spec.tools.join(",");
        cmd.arg("--allowedTools").arg(tools_arg);
    }

    // MCP config (write temp JSON if present)
    if let Some(mcp_path) = write_temp_mcp_config(spec.mcp.as_ref())? {
        cmd.arg("--mcp-config").arg(&mcp_path);
    }

    // Workspace roots (from spec.workspace and spec.writable_roots)
    if let Some(workspace) = &spec.workspace {
        cmd.arg("--add-dir").arg(workspace);
    }
    for root in &spec.writable_roots {
        cmd.arg("--add-dir").arg(root);
    }

    // Add working directory.
    cmd.current_dir(&cwd);

    let delivery_id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let run = run_ndjson_child(
        cmd,
        session_dir,
        &delivery_id,
        "claude.stream-json.ndjson",
        timeout_ms,
        None,
        None,
        "claude -p process",
    )?;
    let events = run
        .events
        .iter()
        .filter_map(|payload| serde_json::to_string(payload).ok())
        .filter_map(|line| ClaudeStreamEvent::parse_line(&line))
        .collect::<Vec<_>>();

    let session_id = extract_session_id_from_claude_events(&events);
    Ok((
        run.process_success,
        events,
        run.events,
        session_id,
        run.stderr,
    ))
}

pub(super) fn apply_claude_model_and_effort_args(cmd: &mut Command, spec: &LaunchSpec) {
    if let Some(model) = &spec.model {
        cmd.arg("--model").arg(model);
    }
    // Reasoning effort: claude has a native session flag.
    if let Some(effort) = &spec.effort {
        cmd.arg("--effort").arg(effort);
    }
}

pub(super) fn apply_claude_output_schema_arg(cmd: &mut Command, spec: &LaunchSpec) {
    if let Some(schema) = &spec.output_schema {
        cmd.arg("--json-schema")
            .arg(schema_to_json_schema(schema).to_string());
    }
}

/// Build a [`resident::ResidentConfig`] from the same launch inputs the default
/// path uses, so the resident invocation surface matches `claude -p` flag for
/// flag (only `-p <prompt>` becomes `--input-format stream-json`).
pub(super) fn build_resident_config(
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    project: &ProjectContext,
) -> resident::ResidentConfig {
    let spec = build_launch_spec(member, message);
    let system_prompt = provider_developer_instructions(member);
    // Same cwd precedence as the default Claude path (P3, Stage 3):
    // member.provider_cwd_hint → selected project.project_root → process cwd.
    let cwd = delivery_worker_cwd(member, project);

    let mcp_config_path = write_temp_mcp_config(spec.mcp.as_ref()).ok().flatten();

    let mut add_dirs = Vec::new();
    if let Some(workspace) = &spec.workspace {
        add_dirs.push(workspace.clone());
    }
    for root in &spec.writable_roots {
        add_dirs.push(root.clone());
    }

    resident::ResidentConfig {
        binary: "claude".into(),
        model: spec.model.clone(),
        effort: spec.effort.clone(),
        output_schema_json: spec
            .output_schema
            .as_ref()
            .map(|schema| schema_to_json_schema(schema).to_string()),
        permission_mode: ClaudeAdapter.map_permission(spec.permission).to_string(),
        tools: spec.tools.clone(),
        system_prompt,
        mcp_config_path,
        add_dirs,
        cwd,
        resume: spec.resume.clone(),
    }
}

/// Opt-in resident sibling of [`run_claude_exec_delivery_real`]. Holds a
/// `claude --input-format stream-json` process open and feeds the turn as a
/// stdin frame, returning the SAME `(success, events, raw_events, session_id, stderr)`
/// tuple shape as the default path so `run_claude_delivery` can share the same
/// status, telemetry, and recording logic.
///
/// This transport is process-local: the machine NodeDaemon is the only durable
/// daemon authority. The resident is dropped after the turn, which closes
/// stdin and reaps the child without creating a second lifecycle controller.
pub(super) fn run_claude_resident_delivery_real(
    session_dir: &Path,
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<ClaudeDeliveryRun> {
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    let config = build_resident_config(member, message, project);
    let stderr_path = session_dir.join("claude.stderr");
    let timeout = Duration::from_millis(timeout_ms.max(1));

    let mut resident = resident::ResidentClaude::spawn(config, &stderr_path).map_err(|error| {
        CliError::Usage(format!("failed to spawn resident claude process: {error}"))
    })?;

    // Drive exactly one turn. On error (timeout / dead child) the resident is
    // dropped (stdin closed, child reaped) and we surface a failed tuple,
    // mirroring the default path's timeout behavior.
    let turn = match resident.send_turn(&message_content, timeout) {
        Ok(turn) => turn,
        Err(error) => {
            let stderr_log = fs::read_to_string(&stderr_path).unwrap_or_default();
            let session_id = resident.session_id();
            drop(resident);
            return Ok((
                false,
                Vec::new(),
                Vec::new(),
                session_id,
                format!("{error}\n{stderr_log}"),
            ));
        }
    };

    // Map ResidentEvent -> ClaudeStreamEvent (same shape, local type bridge).
    let events: Vec<ClaudeStreamEvent> = turn
        .events
        .into_iter()
        .map(|event| ClaudeStreamEvent {
            event_type: event.event_type,
            payload: event.payload,
        })
        .collect();
    let raw_events = events.iter().map(|event| event.payload.clone()).collect();
    let session_id = turn.session_id;
    let stderr_log = fs::read_to_string(&stderr_path).unwrap_or_default();

    // Clean shutdown: closes stdin (EOF) and reaps the child. v1 is one turn
    // per delivery so we do not keep the resident across `run_claude_delivery`
    // calls; the in-process pool (resident.rs) is the seam for that later.
    resident.shutdown();

    Ok((turn.success, events, raw_events, session_id, stderr_log))
}

pub(super) fn parse_hook_payload(input: &str) -> serde_json::Value {
    if input.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(input).unwrap_or_else(|error| {
            serde_json::json!({
                "parse_error": error.to_string(),
                "raw": input
            })
        })
    }
}

pub(super) fn record_provider_hook_event(
    store: &HarnessStore,
    args: &[String],
    provider: &str,
) -> CliResult<()> {
    store.init()?;
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    accept_provider_hook_event(store, args, provider, &parse_hook_payload(&stdin))
}

pub(super) fn accept_provider_hook_event(
    store: &HarnessStore,
    args: &[String],
    provider: &str,
    _payload: &serde_json::Value,
) -> CliResult<()> {
    let agent_id = value(args, "--agent")
        .or_else(|| env::var("HARNESS_AGENT_MEMBER_ID").ok())
        .ok_or_else(|| CliError::Usage("--agent is required".into()))?;
    let runtime_id = value(args, "--runtime").or_else(|| env::var("HARNESS_AGENT_RUNTIME_ID").ok());
    let member = latest_member(store, &agent_id)?;
    if member.provider != provider {
        return Err(CliError::Usage(format!(
            "provider hook binding mismatch: AgentMember {agent_id} uses {}, not {provider}",
            member.provider
        )));
    }
    if let Some(runtime_id) = runtime_id {
        let runtime = latest_runtime(store, &runtime_id)?.ok_or_else(|| {
            CliError::Usage(format!("provider hook runtime {runtime_id} does not exist"))
        })?;
        if runtime.agent_member_id != agent_id || runtime.provider != provider {
            return Err(CliError::Usage(format!(
                "provider hook runtime {runtime_id} is not bound to {provider} AgentMember {agent_id}"
            )));
        }
    }
    // Compatibility ingress only. Provider hooks are neither AgentSession
    // lifecycle authority nor Evidence/Delivery authority; the NodeDaemon and
    // canonical transport writers own those transitions. The raw frame is
    // deliberately discarded after validating the bound AgentMember.
    Ok(())
}

pub(super) fn append_harness_runtime_control_fact(
    store: &HarnessStore,
    agent_member_id: &str,
    runtime_id: Option<&str>,
    task_id: Option<&str>,
    event_type: &str,
    summary: &str,
    payload_ref: Option<&str>,
) -> CliResult<()> {
    let event = Evidence {
        id: generated_id("event"),
        task_id: task_id.map(str::to_string),
        source_type: "harness_runtime_control_fact".into(),
        source_ref: serde_json::json!({
            "agent_member_id": agent_member_id,
            "provider_runtime_id": runtime_id,
            "provider": "codex",
            "event_type": event_type,
            "payload_ref": payload_ref,
        })
        .to_string(),
        summary: summary.into(),
        created_at: now_string(),
        evidence_kind: Some("harness_runtime_control_fact".into()),
        goal_id: None,
    };
    store.append_evidence(&event)?;
    Ok(())
}
