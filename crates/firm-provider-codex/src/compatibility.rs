//! Codex `exec --json` parsing for the explicit direct-delivery compatibility
//! binding. The events stay process-local; only coordination receipts leave
//! the application boundary.

use std::io::BufRead;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use harness_core::{
    normalize_output_schema, LaunchPermission, LaunchSpec, MessageTerminalSource,
    ProviderExecutionStatus,
};

#[derive(Debug)]
pub struct CodexCompatibilityRun {
    pub process_success: bool,
    pub events: Vec<CodexExecEvent>,
    pub raw_events: Vec<serde_json::Value>,
    pub stderr: String,
}

pub fn run_codex_compatibility(
    spec: &LaunchSpec,
    prompt: &str,
    developer_instructions: &str,
    cwd: &Path,
    session_dir: &Path,
    timeout: Duration,
) -> Result<CodexCompatibilityRun, String> {
    let mut command = Command::new("codex");
    command.arg("exec");
    let resuming = spec.resume.is_some();
    if let Some(resume_id) = &spec.resume {
        command
            .arg("resume")
            .arg("--json")
            .arg(resume_id)
            .arg(prompt);
    } else {
        command.arg("--json").arg(prompt);
    }
    command.env("CODEX_DEVELOPER_INSTRUCTIONS", developer_instructions);
    if let Some(model) = &spec.model {
        command.arg("-m").arg(model);
    }
    if let Some(effort) = &spec.effort {
        command
            .arg("-c")
            .arg(format!("model_reasoning_effort={effort}"));
    }
    if let Some(schema) = &spec.output_schema {
        let schema_path = session_dir.join("output-schema.json");
        std::fs::write(&schema_path, normalize_output_schema(schema).to_string())
            .map_err(|error| format!("failed to write {}: {error}", schema_path.display()))?;
        command.arg("--output-schema").arg(schema_path);
    }
    compile_codex_mcp(&mut command, spec)?;
    if !resuming {
        command.arg("--sandbox").arg(match spec.permission {
            LaunchPermission::ReadOnly => "read-only",
            LaunchPermission::WorkspaceWrite => "workspace-write",
            LaunchPermission::FullAccess => "danger-full-access",
        });
        if let Some(workspace) = &spec.workspace {
            command.arg("-C").arg(workspace);
        }
        for root in &spec.writable_roots {
            command.arg("--add-dir").arg(root);
        }
    }
    command.current_dir(cwd);
    let run = harness_runtime_host::run_ndjson_child(command, timeout, None, "codex exec")
        .map_err(|error| error.to_string())?;
    let events = run
        .events
        .iter()
        .filter_map(|payload| serde_json::to_string(payload).ok())
        .filter_map(|line| CodexExecEvent::parse_line(&line))
        .collect();
    Ok(CodexCompatibilityRun {
        process_success: run.process_success,
        events,
        raw_events: run.events,
        stderr: run.stderr,
    })
}

fn compile_codex_mcp(command: &mut Command, spec: &LaunchSpec) -> Result<(), String> {
    let Some(mcp) = &spec.mcp else { return Ok(()) };
    for server in &mcp.servers {
        let key = codex_mcp_id_key(&server.id);
        if let Some((binary, args)) = server.command.split_first() {
            command.arg("-c").arg(format!(
                "mcp_servers.{key}.command={}",
                serde_json::to_string(binary).map_err(|error| error.to_string())?
            ));
            if !args.is_empty() {
                command.arg("-c").arg(format!(
                    "mcp_servers.{key}.args={}",
                    serde_json::to_string(args).map_err(|error| error.to_string())?
                ));
            }
        } else if let Some(url) = &server.url {
            command.arg("-c").arg(format!(
                "mcp_servers.{key}.url={}",
                serde_json::to_string(url).map_err(|error| error.to_string())?
            ));
        }
    }
    Ok(())
}

fn codex_mcp_id_key(id: &str) -> String {
    if !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        id.to_string()
    } else {
        serde_json::to_string(id).expect("string serialization cannot fail")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexExecEvent {
    pub event_type: String,
    pub payload: serde_json::Value,
}

impl CodexExecEvent {
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

    pub fn terminal_source(&self) -> Option<MessageTerminalSource> {
        codex_event_is_terminal(&self.event_type).then_some(MessageTerminalSource::TurnCompleted)
    }
}

pub fn codex_event_is_terminal(event_type: &str) -> bool {
    matches!(
        event_type,
        "turn.completed" | "thread.idle" | "turn_completed" | "thread_idle"
    )
}

pub fn parse_codex_ndjson(reader: impl BufRead) -> Vec<CodexExecEvent> {
    parse_codex_ndjson_to(reader, None::<fn(&serde_json::Value)>)
}

pub fn parse_codex_ndjson_to<F: FnMut(&serde_json::Value)>(
    reader: impl BufRead,
    mut on_event: Option<F>,
) -> Vec<CodexExecEvent> {
    let mut events = Vec::new();
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if let Some(event) = CodexExecEvent::parse_line(&line) {
            if let Some(callback) = on_event.as_mut() {
                callback(&event.payload);
            }
            events.push(event);
        }
    }
    events
}

pub fn infer_provider_execution_status(
    events: &[CodexExecEvent],
    process_success: bool,
) -> ProviderExecutionStatus {
    if !process_success {
        ProviderExecutionStatus::Failed
    } else if events
        .iter()
        .any(|event| codex_event_is_terminal(&event.event_type))
    {
        ProviderExecutionStatus::Succeeded
    } else if events.is_empty() {
        ProviderExecutionStatus::Failed
    } else {
        ProviderExecutionStatus::Stale
    }
}

pub fn extract_thread_id_from_exec_events(events: &[CodexExecEvent]) -> Option<String> {
    events.iter().find_map(|event| {
        event
            .payload
            .get("thread_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    })
}

pub fn extract_turn_id_from_exec_events(events: &[CodexExecEvent]) -> Option<String> {
    events.iter().find_map(|event| {
        event
            .payload
            .get("turn_id")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                event
                    .payload
                    .pointer("/turn/id")
                    .and_then(serde_json::Value::as_str)
            })
            .map(str::to_string)
    })
}

pub fn extract_codex_reply_text(events: &[CodexExecEvent]) -> Option<String> {
    let parts = events
        .iter()
        .filter_map(|event| event.payload.get("item"))
        .filter(|item| {
            item.get("type").and_then(serde_json::Value::as_str) == Some("agent_message")
        })
        .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

pub fn extract_codex_final_message(events: &[CodexExecEvent]) -> Option<String> {
    events
        .iter()
        .filter_map(|event| event.payload.get("item"))
        .filter(|item| {
            item.get("type").and_then(serde_json::Value::as_str) == Some("agent_message")
        })
        .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .next_back()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_event_and_last_message_are_provider_owned() {
        let input = b"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"done\"}}\n{\"type\":\"turn.completed\"}\n";
        let events = parse_codex_ndjson(&input[..]);
        assert_eq!(
            infer_provider_execution_status(&events, true),
            ProviderExecutionStatus::Succeeded
        );
        assert_eq!(
            extract_codex_final_message(&events).as_deref(),
            Some("done")
        );
    }
}
