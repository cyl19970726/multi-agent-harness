//! Claude CLI stream parsing for the explicit Host and direct-delivery
//! compatibility bindings.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use harness_core::{
    normalize_output_schema, LaunchMcp, LaunchPermission, LaunchSpec, MessageTerminalSource,
    ProviderExecutionStatus,
};

#[derive(Debug)]
pub struct ClaudeCompatibilityRun {
    pub process_success: bool,
    pub events: Vec<ClaudeStreamEvent>,
    pub raw_events: Vec<serde_json::Value>,
    pub session_id: Option<String>,
    pub stderr: String,
}

pub fn claude_compatibility_permission(permission: LaunchPermission) -> &'static str {
    match permission {
        LaunchPermission::ReadOnly => "plan",
        LaunchPermission::WorkspaceWrite => "acceptEdits",
        LaunchPermission::FullAccess => "bypassPermissions",
    }
}

pub fn run_claude_compatibility(
    spec: &LaunchSpec,
    prompt: &str,
    system_prompt: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<ClaudeCompatibilityRun, String> {
    let mut command = Command::new("claude");
    command
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose");
    if let Some(resume_id) = &spec.resume {
        command.arg("--resume").arg(resume_id);
    }
    if !system_prompt.is_empty() {
        command.arg("--append-system-prompt").arg(system_prompt);
    }
    if let Some(model) = &spec.model {
        command.arg("--model").arg(model);
    }
    if let Some(effort) = &spec.effort {
        command.arg("--effort").arg(effort);
    }
    if let Some(schema) = &spec.output_schema {
        command
            .arg("--json-schema")
            .arg(normalize_output_schema(schema).to_string());
    }
    command
        .arg("--permission-mode")
        .arg(claude_compatibility_permission(spec.permission));
    if !spec.tools.is_empty() {
        command.arg("--allowedTools").arg(spec.tools.join(","));
    }
    if let Some(path) = write_claude_mcp_config(spec.mcp.as_ref())? {
        command.arg("--mcp-config").arg(path);
    }
    if let Some(workspace) = &spec.workspace {
        command.arg("--add-dir").arg(workspace);
    }
    for root in &spec.writable_roots {
        command.arg("--add-dir").arg(root);
    }
    command.current_dir(cwd);
    let run = harness_runtime_host::run_ndjson_child(command, timeout, None, "claude -p process")
        .map_err(|error| error.to_string())?;
    let events = run
        .events
        .iter()
        .filter_map(|payload| serde_json::to_string(payload).ok())
        .filter_map(|line| ClaudeStreamEvent::parse_line(&line))
        .collect::<Vec<_>>();
    let session_id = extract_session_id_from_claude_events(&events);
    Ok(ClaudeCompatibilityRun {
        process_success: run.process_success,
        events,
        raw_events: run.events,
        session_id,
        stderr: run.stderr,
    })
}

pub fn write_claude_mcp_config(mcp: Option<&LaunchMcp>) -> Result<Option<PathBuf>, String> {
    let Some(mcp) = mcp.filter(|mcp| !mcp.servers.is_empty()) else {
        return Ok(None);
    };
    let servers = mcp
        .servers
        .iter()
        .map(|server| {
            let mut value = serde_json::Map::from_iter([(
                "id".into(),
                serde_json::Value::from(server.id.clone()),
            )]);
            if let Some(transport) = &server.transport {
                value.insert(
                    "transport".into(),
                    serde_json::Value::from(transport.clone()),
                );
            }
            if !server.command.is_empty() {
                value.insert("command".into(), serde_json::json!(server.command));
            }
            if let Some(url) = &server.url {
                value.insert("url".into(), serde_json::Value::from(url.clone()));
            }
            if !server.allowed_tools.is_empty() {
                value.insert(
                    "allowed_tools".into(),
                    serde_json::json!(server.allowed_tools),
                );
            }
            (server.id.clone(), serde_json::Value::Object(value))
        })
        .collect::<serde_json::Map<_, _>>();
    let path = std::env::temp_dir().join(format!("claude-mcp-{}.json", std::process::id()));
    std::fs::write(
        &path,
        serde_json::json!({"mcp_servers": servers}).to_string(),
    )
    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(Some(path))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeStreamEvent {
    pub event_type: String,
    pub payload: serde_json::Value,
}

impl ClaudeStreamEvent {
    pub fn parse_line(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let payload = serde_json::from_str::<serde_json::Value>(trimmed).ok()?;
        let event_type = payload
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        Some(Self {
            event_type,
            payload,
        })
    }

    pub fn session_id(&self) -> Option<String> {
        if self.event_type != "system" {
            return None;
        }
        self.payload
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }
}

pub fn infer_claude_session_status(
    events: &[ClaudeStreamEvent],
    process_success: bool,
) -> ProviderExecutionStatus {
    if !process_success {
        return ProviderExecutionStatus::Failed;
    }
    let Some(result) = events.iter().find(|event| event.event_type == "result") else {
        return if events.is_empty() {
            ProviderExecutionStatus::Failed
        } else {
            ProviderExecutionStatus::Stale
        };
    };
    if result.payload.get("error").is_some()
        || result
            .payload
            .get("is_error")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        || result.payload.get("api_error_status").is_some()
    {
        ProviderExecutionStatus::Failed
    } else {
        ProviderExecutionStatus::Succeeded
    }
}

pub fn extract_session_id_from_claude_events(events: &[ClaudeStreamEvent]) -> Option<String> {
    events.iter().find_map(ClaudeStreamEvent::session_id)
}

pub fn extract_claude_reply_text(events: &[ClaudeStreamEvent]) -> Option<String> {
    for event in events.iter().rev() {
        if event.event_type == "result" {
            if let Some(text) = event
                .payload
                .get("result")
                .and_then(serde_json::Value::as_str)
            {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    let mut parts = Vec::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == "assistant")
    {
        let Some(content) = event
            .payload
            .pointer("/message/content")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(serde_json::Value::as_str) == Some("text") {
                if let Some(text) = block.get("text").and_then(serde_json::Value::as_str) {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

pub fn status_to_terminal_source(
    status: &ProviderExecutionStatus,
) -> Option<MessageTerminalSource> {
    match status {
        ProviderExecutionStatus::Succeeded => Some(MessageTerminalSource::TurnCompleted),
        ProviderExecutionStatus::Failed => Some(MessageTerminalSource::Failed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_result_controls_status_and_reply() {
        let events =
            [ClaudeStreamEvent::parse_line(r#"{"type":"result","result":" done "}"#).unwrap()];
        assert_eq!(
            infer_claude_session_status(&events, true),
            ProviderExecutionStatus::Succeeded
        );
        assert_eq!(extract_claude_reply_text(&events).as_deref(), Some("done"));
    }
}
