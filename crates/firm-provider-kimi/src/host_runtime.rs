//! Exact-session Kimi ACP binding for headless Host turns.
//!
//! This binding has no Team lifecycle and no coordination writer authority.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{KimiAcpClient, KimiError, KimiResult, PromptControl};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KimiHostTurn {
    pub native_session_id: String,
    pub provider_receipt_id: String,
    pub response_text: String,
}

pub fn run_kimi_host_turn(
    cwd: &Path,
    native_session_id: &str,
    prompt: &str,
    timeout: Duration,
) -> KimiResult<KimiHostTurn> {
    if native_session_id.trim().is_empty() {
        return Err(KimiError::Usage(
            "KIMI_HOST_EXACT_SESSION_REQUIRED".to_string(),
        ));
    }
    let response = Arc::new(Mutex::new(String::new()));
    let response_sink = Arc::clone(&response);
    let receipt = Arc::new(Mutex::new(None::<String>));
    let receipt_sink = Arc::clone(&receipt);
    let mut client = KimiAcpClient::spawn(cwd, None, None, Some(native_session_id), &[])?;
    let outcome = client.prompt(
        prompt,
        timeout,
        move |provider_receipt_id| {
            *receipt_sink.lock().map_err(|error| {
                KimiError::Usage(format!("Host receipt lock poisoned: {error}"))
            })? = Some(provider_receipt_id.to_string());
            Ok(())
        },
        move |update| {
            if update
                .get("sessionUpdate")
                .and_then(serde_json::Value::as_str)
                == Some("agent_message_chunk")
            {
                if let Some(text) = update
                    .get("content")
                    .and_then(|content| content.get("text"))
                    .and_then(serde_json::Value::as_str)
                {
                    if let Ok(mut collected) = response_sink.lock() {
                        collected.push_str(text);
                    }
                }
            }
        },
        |_| {
            Err(KimiError::Usage(
                "headless Host triage refuses provider permission requests".to_string(),
            ))
        },
        |_| Ok(()),
        || Ok(PromptControl::Continue),
    )?;
    if let Some(error) = outcome.provider_error {
        return Err(KimiError::Usage(format!(
            "headless Kimi Host turn failed: {error}"
        )));
    }
    let provider_receipt_id = receipt
        .lock()
        .map_err(|error| KimiError::Usage(format!("Host receipt lock poisoned: {error}")))?
        .clone()
        .ok_or_else(|| {
            KimiError::Usage("headless Kimi Host turn returned no prompt receipt".into())
        })?;
    let response_text = response
        .lock()
        .map_err(|error| KimiError::Usage(format!("Host response lock poisoned: {error}")))?
        .clone();
    Ok(KimiHostTurn {
        native_session_id: native_session_id.to_string(),
        provider_receipt_id,
        response_text,
    })
}
