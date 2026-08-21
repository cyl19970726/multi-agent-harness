//! Kimi `--output-format stream-json` decoding for the explicit historical
//! direct-delivery compatibility binding.

use harness_core::ProviderExecutionStatus;

pub fn extract_kimi_reply_text(frames: &[serde_json::Value]) -> Option<String> {
    let mut parts = Vec::new();
    for frame in frames {
        if frame.get("role").and_then(serde_json::Value::as_str) != Some("assistant") {
            continue;
        }
        match frame.get("content") {
            Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
                parts.push(text.trim().to_string());
            }
            Some(serde_json::Value::Array(blocks)) => {
                for block in blocks {
                    let text = block.as_str().or_else(|| {
                        block
                            .get("text")
                            .or_else(|| block.get("content"))
                            .and_then(serde_json::Value::as_str)
                    });
                    if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                        parts.push(text.trim().to_string());
                    }
                }
            }
            _ => {}
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

pub fn extract_kimi_session_id(frames: &[serde_json::Value]) -> Option<String> {
    frames.iter().find_map(|frame| {
        (frame.get("type").and_then(serde_json::Value::as_str) == Some("session.resume_hint"))
            .then(|| {
                frame
                    .get("session_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .flatten()
    })
}

pub fn infer_kimi_status(
    frames: &[serde_json::Value],
    process_success: bool,
) -> ProviderExecutionStatus {
    if !process_success {
        ProviderExecutionStatus::Failed
    } else if frames.is_empty() {
        ProviderExecutionStatus::Stale
    } else {
        ProviderExecutionStatus::Succeeded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_stream_yields_reply_session_and_status() {
        let frames = vec![
            serde_json::json!({"role": "assistant", "content": "done"}),
            serde_json::json!({"type": "session.resume_hint", "session_id": "session-1"}),
        ];
        assert_eq!(extract_kimi_reply_text(&frames).as_deref(), Some("done"));
        assert_eq!(
            extract_kimi_session_id(&frames).as_deref(),
            Some("session-1")
        );
        assert_eq!(
            infer_kimi_status(&frames, true),
            ProviderExecutionStatus::Succeeded
        );
    }
}
