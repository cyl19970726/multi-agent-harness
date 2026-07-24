//! Read-only projections over provider-owned session storage.
//!
//! This module deliberately returns display projections without copying them
//! into Harness ledgers. Provider paths stay server-side and provider thinking
//! is dropped before a projection can reach the Dashboard.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use harness_core::{NativeSessionAvailability, NativeSessionRef};

use crate::{CliError, CliResult};

const MAX_ITEMS: usize = 300;
const MAX_SUMMARY_CHARS: usize = 600;

pub(crate) fn read_activity(session: &NativeSessionRef) -> CliResult<serde_json::Value> {
    let path = locate(session)?;
    let Some(path) = path else {
        return Ok(serde_json::json!({
            "native_session_id": session.native_session_id,
            "provider": session.provider,
            "execution_mode": session.execution_mode,
            "availability": NativeSessionAvailability::Missing,
            "items": [],
            "truncated": false,
        }));
    };
    let file = fs::File::open(&path)?;
    let mut items = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let projected = match session.provider.as_str() {
            "codex" => project_codex(&value),
            "kimi" => project_kimi(&value),
            "claude" => project_claude(&value),
            _ => None,
        };
        if let Some(item) = projected {
            items.push(item);
        }
    }
    let truncated = items.len() > MAX_ITEMS;
    if truncated {
        items.drain(..items.len() - MAX_ITEMS);
    }
    Ok(serde_json::json!({
        "native_session_id": session.native_session_id,
        "provider": session.provider,
        "execution_mode": session.execution_mode,
        "availability": NativeSessionAvailability::Available,
        "items": items,
        "truncated": truncated,
    }))
}

fn locate(session: &NativeSessionRef) -> CliResult<Option<PathBuf>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("HOME is unavailable for native session discovery".into())
    })?;
    let result = match session.provider.as_str() {
        "codex" => find_file(
            &std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("sessions"),
            &format!("{}.jsonl", session.native_session_id),
            5,
        ),
        "kimi" => find_kimi_wire(
            &home.join(".kimi-code/sessions"),
            &session.native_session_id,
            4,
        ),
        "claude" => find_file(
            &home.join(".claude/projects"),
            &format!("{}.jsonl", session.native_session_id),
            4,
        ),
        _ => None,
    };
    Ok(result)
}

fn find_file(root: &Path, suffix: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !root.is_dir() {
        return None;
    }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
        {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file(&path, suffix, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn find_kimi_wire(root: &Path, session_dir: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !root.is_dir() {
        return None;
    }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let expected = if session_dir.starts_with("session_") {
                session_dir.to_string()
            } else {
                format!("session_{session_dir}")
            };
            if path.file_name().and_then(|name| name.to_str()) == Some(expected.as_str()) {
                let wire = path.join("agents/main/wire.jsonl");
                return wire.is_file().then_some(wire);
            }
            if let Some(found) = find_kimi_wire(&path, session_dir, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn project_codex(value: &serde_json::Value) -> Option<serde_json::Value> {
    let timestamp = value.get("timestamp").and_then(|value| value.as_str());
    let row_type = value.get("type").and_then(|value| value.as_str())?;
    let payload = value.get("payload")?;
    if row_type == "event_msg" {
        return match payload.get("type").and_then(|value| value.as_str())? {
            "agent_message" => activity(
                "message",
                "completed",
                "Codex",
                payload.get("message")?.as_str()?,
                timestamp,
            ),
            "user_message" => activity(
                "message",
                "completed",
                "Lead",
                payload.get("message")?.as_str()?,
                timestamp,
            ),
            // Includes agent_reasoning: provider thinking is never projected.
            _ => None,
        };
    }
    if row_type != "response_item" {
        return None;
    }
    match payload.get("type").and_then(|value| value.as_str())? {
        "function_call" => activity(
            "tool",
            "started",
            payload
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("tool"),
            payload
                .get("arguments")
                .and_then(|value| value.as_str())
                .unwrap_or("tool called"),
            timestamp,
        ),
        "function_call_output" => activity(
            "tool",
            "completed",
            "tool result",
            "provider recorded tool output",
            timestamp,
        ),
        _ => None,
    }
}

fn project_kimi(value: &serde_json::Value) -> Option<serde_json::Value> {
    let row_type = value.get("type").and_then(|value| value.as_str())?;
    let timestamp = value.get("time").map(timestamp_value);
    match row_type {
        "turn.prompt" => activity(
            "message",
            "completed",
            "Lead",
            text_from_parts(value.get("input")?)?.as_str(),
            timestamp.as_deref(),
        ),
        "context.append_loop_event" => {
            let event = value.get("event")?;
            match event.get("type").and_then(|value| value.as_str())? {
                "tool.call" => activity(
                    "tool",
                    "started",
                    event
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("tool"),
                    event
                        .get("description")
                        .and_then(|value| value.as_str())
                        .unwrap_or("tool called"),
                    timestamp.as_deref(),
                ),
                "tool.result" => activity(
                    "tool",
                    "completed",
                    "tool result",
                    "Kimi recorded tool output",
                    timestamp.as_deref(),
                ),
                "content.part" => {
                    let part = event.get("part")?;
                    if part.get("type").and_then(|value| value.as_str()) != Some("text") {
                        return None;
                    }
                    activity(
                        "message",
                        "completed",
                        "Kimi",
                        part.get("text")?.as_str()?,
                        timestamp.as_deref(),
                    )
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn project_claude(value: &serde_json::Value) -> Option<serde_json::Value> {
    let row_type = value.get("type").and_then(|value| value.as_str())?;
    if !matches!(row_type, "user" | "assistant") {
        return None;
    }
    let timestamp = value.get("timestamp").and_then(|value| value.as_str());
    let content = value.pointer("/message/content")?;
    if let Some(text) = content.as_str() {
        return activity(
            "message",
            "completed",
            if row_type == "user" { "Lead" } else { "Claude" },
            text,
            timestamp,
        );
    }
    for part in content.as_array()? {
        match part.get("type").and_then(|value| value.as_str()) {
            Some("text") => {
                return activity(
                    "message",
                    "completed",
                    if row_type == "user" { "Lead" } else { "Claude" },
                    part.get("text")?.as_str()?,
                    timestamp,
                )
            }
            Some("tool_use") => {
                return activity(
                    "tool",
                    "started",
                    part.get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("tool"),
                    "Claude recorded tool call",
                    timestamp,
                )
            }
            Some("tool_result") => {
                return activity(
                    "tool",
                    "completed",
                    "tool result",
                    "Claude recorded tool output",
                    timestamp,
                )
            }
            _ => {}
        }
    }
    None
}

fn activity(
    kind: &str,
    status: &str,
    title: &str,
    summary: &str,
    occurred_at: Option<&str>,
) -> Option<serde_json::Value> {
    let summary = summary.chars().take(MAX_SUMMARY_CHARS).collect::<String>();
    Some(serde_json::json!({
        "kind": kind,
        "status": status,
        "title": title,
        "summary": summary,
        "occurred_at": occurred_at,
    }))
}

fn text_from_parts(value: &serde_json::Value) -> Option<String> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn timestamp_value(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|value| format!("unix-ms:{value}")))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_provider_thinking_from_native_projection() {
        let codex = serde_json::json!({"timestamp":"t", "type":"event_msg", "payload":{"type":"agent_reasoning", "text":"secret"}});
        let kimi = serde_json::json!({"type":"context.append_loop_event", "event":{"type":"content.part", "part":{"type":"think", "think":"secret"}}, "time":1});
        assert!(project_codex(&codex).is_none());
        assert!(project_kimi(&kimi).is_none());
    }
}
