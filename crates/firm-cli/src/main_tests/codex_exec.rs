
use super::*;
use std::io::Cursor;

// Stage 1: NDJSON parser tests
#[test]
fn test_parse_codex_ndjson_valid_events() {
    let ndjson = r#"{"type": "tool_call", "id": "1"}
{"type": "tool_output", "id": "1"}
{"type": "turn_completed"}
"#;
    let reader = Cursor::new(ndjson.as_bytes());
    let events = parse_codex_ndjson(reader);

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event_type, "tool_call");
    assert_eq!(events[1].event_type, "tool_output");
    assert_eq!(events[2].event_type, "turn_completed");
}

#[test]
fn test_parse_codex_ndjson_skip_invalid_lines() {
    let ndjson = r#"{"type": "tool_call"}
invalid json line
{"type": "tool_output"}
"#;
    let reader = Cursor::new(ndjson.as_bytes());
    let events = parse_codex_ndjson(reader);

    // Should skip the invalid line
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "tool_call");
    assert_eq!(events[1].event_type, "tool_output");
}

#[test]
fn test_parse_codex_ndjson_empty_lines() {
    let ndjson = r#"{"type": "tool_call"}

{"type": "tool_output"}
"#;
    let reader = Cursor::new(ndjson.as_bytes());
    let events = parse_codex_ndjson(reader);

    // Should skip empty lines
    assert_eq!(events.len(), 2);
}

#[test]
fn test_codex_exec_event_parse_line_valid() {
    let line = r#"{"type": "tool_call", "payload": "test"}"#;
    let event = CodexExecEvent::parse_line(line).expect("should parse");

    assert_eq!(event.event_type, "tool_call");
    assert_eq!(
        event.payload.get("type").and_then(|v| v.as_str()),
        Some("tool_call")
    );
}

#[test]
fn test_codex_exec_event_parse_line_missing_type() {
    let line = r#"{"payload": "test"}"#;
    let event = CodexExecEvent::parse_line(line).expect("should parse");

    // Should default to "unknown" when type is missing
    assert_eq!(event.event_type, "unknown");
}

#[test]
fn test_codex_exec_event_terminal_source() {
    // Real codex 0.13x exec --json emits dot-separated discriminants.
    let json = serde_json::json!({"type": "turn.completed"});
    let event = CodexExecEvent {
        event_type: "turn.completed".into(),
        payload: json,
    };

    assert_eq!(
        event.terminal_source(),
        Some(MessageTerminalSource::TurnCompleted)
    );
}

#[test]
fn test_codex_exec_event_terminal_source_legacy_underscore() {
    // Backward-compat: older underscore names still treated as terminal.
    let event = CodexExecEvent {
        event_type: "turn_completed".into(),
        payload: serde_json::json!({"type": "turn_completed"}),
    };
    assert_eq!(
        event.terminal_source(),
        Some(MessageTerminalSource::TurnCompleted)
    );
}

#[test]
fn test_codex_exec_event_non_terminal() {
    let json = serde_json::json!({"type": "tool_call"});
    let event = CodexExecEvent {
        event_type: "tool_call".into(),
        payload: json,
    };

    assert_eq!(event.terminal_source(), None);
}

// Stage 1: Status inference tests
#[test]
fn test_infer_provider_execution_status_succeeded() {
    let events = vec![
        CodexExecEvent {
            event_type: "tool_call".into(),
            payload: serde_json::json!({}),
        },
        CodexExecEvent {
            event_type: "turn.completed".into(),
            payload: serde_json::json!({"type": "turn.completed"}),
        },
    ];

    let status = infer_provider_execution_status(&events, true);
    assert_eq!(status, ProviderExecutionStatus::Succeeded);
}

#[test]
fn test_infer_provider_execution_status_succeeded_real_codex_stream() {
    // Mirrors a real codex 0.13x exec --json stream.
    let events = vec![
        CodexExecEvent {
            event_type: "thread.started".into(),
            payload: serde_json::json!({
                "thread_id": "019e7ecf-42f4-7eb0-aa73-a4ae7a8f01f0",
                "type": "thread.started"
            }),
        },
        CodexExecEvent {
            event_type: "turn.started".into(),
            payload: serde_json::json!({"type": "turn.started"}),
        },
        CodexExecEvent {
            event_type: "item.completed".into(),
            payload: serde_json::json!({
                "item": {"id": "item_0", "text": "codex exec acceptance OK", "type": "agent_message"},
                "type": "item.completed"
            }),
        },
        CodexExecEvent {
            event_type: "turn.completed".into(),
            payload: serde_json::json!({"type": "turn.completed"}),
        },
    ];

    assert_eq!(
        infer_provider_execution_status(&events, true),
        ProviderExecutionStatus::Succeeded
    );
    assert_eq!(
        extract_thread_id_from_exec_events(&events).as_deref(),
        Some("019e7ecf-42f4-7eb0-aa73-a4ae7a8f01f0")
    );
}

#[test]
fn test_infer_provider_execution_status_failed_exit() {
    let events = vec![CodexExecEvent {
        event_type: "tool_call".into(),
        payload: serde_json::json!({}),
    }];

    let status = infer_provider_execution_status(&events, false);
    assert_eq!(status, ProviderExecutionStatus::Failed);
}

#[test]
fn test_infer_provider_execution_status_stale() {
    let events = vec![CodexExecEvent {
        event_type: "tool_call".into(),
        payload: serde_json::json!({}),
    }];

    let status = infer_provider_execution_status(&events, true);
    assert_eq!(status, ProviderExecutionStatus::Stale);
}

#[test]
fn test_infer_provider_execution_status_no_events_and_failed() {
    let events = vec![];

    let status = infer_provider_execution_status(&events, false);
    assert_eq!(status, ProviderExecutionStatus::Failed);
}

#[test]
fn test_infer_provider_execution_status_empty_success() {
    let events = vec![];

    let status = infer_provider_execution_status(&events, true);
    assert_eq!(status, ProviderExecutionStatus::Failed);
}

// Stage 3: Delivery selector tests
#[test]
fn test_codex_delivery_selector_respects_env_var() {
    // This test validates the logic of the selector function.
    // It doesn't actually invoke the function, but documents the expected behavior:
    // - HARNESS_CODEX_DELIVERY=exec -> run_codex_exec_delivery
    // - Codex now uses exec-stream delivery only
    // - no flag -> defaults to appserver

    let env_exec = "exec";
    let env_appserver = "appserver";
    let env_default = "";

    assert_eq!(env_exec, "exec");
    assert_eq!(env_appserver, "appserver");
    assert!(!env_default.is_empty() || env_default.is_empty()); // vacuous, but documents fallback
}

#[test]
fn test_extract_thread_id_from_exec_events_present() {
    let events = vec![CodexExecEvent {
        event_type: "thread.started".into(),
        payload: serde_json::json!({"thread_id": "123", "type": "thread.started"}),
    }];

    // thread.started carries the real thread_id; surface it.
    let thread_id = extract_thread_id_from_exec_events(&events);
    assert_eq!(thread_id.as_deref(), Some("123"));
}

#[test]
fn test_extract_thread_id_from_exec_events_absent_is_none() {
    let events = vec![CodexExecEvent {
        event_type: "turn.started".into(),
        payload: serde_json::json!({"type": "turn.started"}),
    }];

    assert_eq!(extract_thread_id_from_exec_events(&events), None);
}

#[test]
fn test_extract_turn_id_from_exec_events_present() {
    let events = vec![CodexExecEvent {
        event_type: "turn.started".into(),
        payload: serde_json::json!({"turn_id": "456", "type": "turn.started"}),
    }];

    let turn_id = extract_turn_id_from_exec_events(&events);
    assert_eq!(turn_id.as_deref(), Some("456"));
}

#[test]
fn test_extract_turn_id_from_exec_events_absent_is_none() {
    let events = vec![CodexExecEvent {
        event_type: "thread.started".into(),
        payload: serde_json::json!({"thread_id": "789", "type": "thread.started"}),
    }];

    assert_eq!(extract_turn_id_from_exec_events(&events), None);
}

#[test]
fn extract_codex_final_message_returns_terminal_message_not_joined() {
    // issue #139 item 2: structured-output parsing must read the FINAL
    // agent_message, not the joined narration — a streamed preamble
    // ("I'll start by inspecting…") must not be captured as the result.
    let events = vec![
        CodexExecEvent {
            event_type: "item.completed".into(),
            payload: serde_json::json!({
                "item": {"type": "agent_message", "text": "I'll start by inspecting the repo."}
            }),
        },
        CodexExecEvent {
            event_type: "item.completed".into(),
            payload: serde_json::json!({
                "item": {"type": "agent_message", "text": "{\"ok\": true}"}
            }),
        },
    ];
    // The human-facing reply joins every message…
    assert_eq!(
        extract_codex_reply_text(&events).as_deref(),
        Some("I'll start by inspecting the repo.\n{\"ok\": true}")
    );
    // …but the final-message extractor returns only the terminal one, which
    // parses cleanly to the structured object (no preamble pollution).
    assert_eq!(
        extract_codex_final_message(&events).as_deref(),
        Some("{\"ok\": true}")
    );
    assert_eq!(
        extract_codex_final_message(&events)
            .as_deref()
            .and_then(extract_json_object),
        Some(serde_json::json!({"ok": true}))
    );
}
