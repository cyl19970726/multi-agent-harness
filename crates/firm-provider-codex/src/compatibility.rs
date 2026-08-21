//! Codex `exec --json` parsing for the explicit direct-delivery compatibility
//! binding. The events stay process-local; only coordination receipts leave
//! the application boundary.

use std::io::BufRead;

use harness_core::{MessageTerminalSource, ProviderExecutionStatus};

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
