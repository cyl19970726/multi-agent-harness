//! Claude CLI stream parsing for the explicit Host and direct-delivery
//! compatibility bindings.

use harness_core::{MessageTerminalSource, ProviderExecutionStatus};

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
