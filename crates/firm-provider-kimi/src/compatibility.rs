//! Kimi `--output-format stream-json` decoding for the explicit historical
//! direct-delivery compatibility binding.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use harness_core::{LaunchSpec, ProviderExecutionStatus};

#[derive(Debug)]
pub struct KimiCompatibilityRun {
    pub process_success: bool,
    pub raw_events: Vec<serde_json::Value>,
    pub session_id: Option<String>,
    pub stderr: String,
}

pub fn run_kimi_compatibility(
    binary: &str,
    spec: &LaunchSpec,
    prompt: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<KimiCompatibilityRun, String> {
    let mut command = Command::new(binary);
    command
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json");
    if let Some(resume_id) = &spec.resume {
        command.arg("--session").arg(resume_id);
    }
    if let Some(model) = &spec.model {
        command.arg("--model").arg(model);
    }
    command.current_dir(cwd);
    let run = harness_runtime_host::run_ndjson_child(command, timeout, None, "kimi -p process")
        .map_err(|error| error.to_string())?;
    let session_id = extract_kimi_session_id(&run.events);
    Ok(KimiCompatibilityRun {
        process_success: run.process_success,
        raw_events: run.events,
        session_id,
        stderr: run.stderr,
    })
}

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
