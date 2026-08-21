use super::*;

#[cfg(unix)]
fn scripted_client() -> (KimiAcpClient, Sender<serde_json::Value>) {
    // The child is only a sink for the prompt/reverse-request response
    // writes. Scripted frames enter through the exact channels populated
    // by the production reader thread, making ordering deterministic
    // without sleeps or scheduler guesses.
    let mut child = Command::new("sh")
        .args(["-c", "cat >/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn scripted ACP sink");
    let stdin = child.stdin.take().expect("scripted ACP stdin");
    let (update_tx, updates) = channel();
    (
        KimiAcpClient {
            child,
            stdin: Some(stdin),
            next_request_id: 2,
            pending: Arc::new(Mutex::new(HashMap::new())),
            updates,
            reader: None,
            stderr_tail: Arc::new(Mutex::new(String::new())),
            session_id: Some("scripted-session".to_string()),
            model: None,
            effort: None,
            effective_model: None,
            effective_effort: None,
            config_options: Vec::new(),
            provider_version: None,
            supports_session_close: true,
            prompt_active: false,
            settled_boundary_observed: true,
            shutdown_receipt: None,
        },
        update_tx,
    )
}

fn session_update(kind: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "scripted-session",
            "update": {"sessionUpdate": kind}
        }
    })
}

#[test]
fn resume_handshake_fails_closed_when_provider_returns_a_different_session() {
    let mismatch = verified_attached_session_id(
        &serde_json::json!({"result": {"sessionId": "provider-selected-other"}}),
        Some("requested-exact-session"),
        "session/resume",
    )
    .expect_err("mismatched resume must fail before a prompt");
    assert!(mismatch.to_string().contains("KIMI_ACP_RESUME_MISMATCH"));
    assert_eq!(
        verified_attached_session_id(
            &serde_json::json!({"result": {"sessionId": "requested-exact-session"}}),
            Some("requested-exact-session"),
            "session/resume",
        )
        .unwrap(),
        "requested-exact-session"
    );
    assert_eq!(
        verified_attached_session_id(
            &serde_json::json!({"result": {"configOptions": []}}),
            Some("requested-exact-session"),
            "session/resume",
        )
        .unwrap(),
        "requested-exact-session"
    );
}

fn provider_error_response() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {"code": -32000, "message": "scripted provider error"}
    })
}

fn terminal_success_response() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"stopReason": "end_turn"}
    })
}

#[cfg(unix)]
fn drive_scripted_prompt(
    client: &mut KimiAcpClient,
    response: Receiver<serde_json::Value>,
    on_accepted: &mut impl FnMut(&str) -> CliResult<()>,
    on_update: &mut impl FnMut(&serde_json::Value),
    on_request: &mut impl FnMut(&serde_json::Value) -> CliResult<serde_json::Value>,
) -> PromptOutcome {
    client
        .drive_prompt(
            (1, response),
            PromptTimeouts::production(Duration::from_secs(30)),
            on_accepted,
            on_update,
            on_request,
            &mut |_| Ok(()),
            &mut || Ok(PromptControl::Continue),
        )
        .expect("scripted prompt completes")
}

fn config_options() -> Vec<serde_json::Value> {
    serde_json::json!([
        {
            "id": "model",
            "currentValue": "kimi-code/k3",
            "options": [{"value": "kimi-code/k3"}]
        },
        {
            "id": "thinking",
            "currentValue": "high",
            "options": [{"value": "low"}, {"value": "high"}, {"value": "max"}]
        }
    ])
    .as_array()
    .expect("array")
    .clone()
}

#[test]
fn kimi_acp_maps_neutral_effort_to_the_advertised_thinking_option() {
    let options = config_options();
    assert_eq!(
        current_config_value(&options, "thinking").as_deref(),
        Some("high")
    );
    assert_eq!(
        config_option_supports(&options, "thinking", "max"),
        Some(true)
    );
    assert_eq!(
        config_option_supports(&options, "thinking", "ultra"),
        Some(false)
    );
}

#[cfg(unix)]
#[test]
fn close_uses_correlated_session_close_then_clean_stdio_exit_and_is_one_shot() {
    let mut client = KimiAcpClient::scripted_for_close_contract();
    let before = client.observe_runtime().expect("observe live fake");
    assert!(before.transport_alive);
    assert!(before.process_alive);
    assert!(before.settled_boundary_observed);

    let receipt = client
        .close_session_and_runtime()
        .expect("session/close and clean process exit");
    assert_eq!(receipt.session_id, "scripted-session");
    assert_eq!(receipt.response_id, 2);
    assert!(receipt.shutdown.process_was_running);
    assert!(receipt.shutdown.process_reaped);
    assert!(receipt.shutdown.stdout_reader_joined);
    assert_eq!(receipt.shutdown.exit_status, "exit status: 0");

    let after = client.observe_runtime().expect("observe released fake");
    assert!(!after.transport_alive);
    assert!(!after.process_alive);
    assert!(after.settled_boundary_observed);
    assert!(client.close_session_and_runtime().is_err());
    assert!(client.shutdown_with_receipt().is_err());
}

/// Manual exact-provider bootstrap canary for a newly reviewed
/// `close_runtime` binding. Normal Team execution must not use this test
/// as a compatibility bypass: it exists so a real Kimi version can earn
/// LiveCanary evidence before that binding is admitted as Active.
#[cfg(unix)]
#[test]
#[ignore = "requires the authenticated local Kimi Code 0.36.1 runtime"]
fn live_kimi_0361_session_close_cleanly_reaps_and_retains_session_id() {
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let mut client = KimiAcpClient::spawn(&workspace, None, None, None, &[])
        .expect("spawn authenticated Kimi ACP");
    assert_eq!(client.provider_version(), Some("0.36.1"));
    let native_session_id = client
        .session_id()
        .expect("session/new returned native session id")
        .to_string();
    let receipt = client
        .close_session_and_runtime()
        .expect("session/close acknowledgement and clean owned-process reap");
    assert_eq!(receipt.session_id, native_session_id);
    assert!(receipt.shutdown.process_was_running);
    assert!(receipt.shutdown.process_reaped);
    assert!(receipt.shutdown.stdout_reader_joined);
    assert!(receipt.shutdown.exit_status.contains("status: 0"));
    eprintln!(
        "KIMI_CLOSE_LIVE_CANARY provider=0.36.1 native_session_id={} response_id={} exit_status={}",
        native_session_id, receipt.response_id, receipt.shutdown.exit_status
    );
}

#[cfg(unix)]
#[test]
fn force_disposal_returns_only_a_process_receipt_and_runs_once() {
    let (mut client, _updates) = scripted_client();
    let receipt = client
        .shutdown_with_receipt()
        .expect("force-dispose owned fake process");
    assert!(receipt.process_was_running);
    assert!(receipt.process_reaped);
    assert!(receipt.stdout_reader_joined);
    assert!(client.shutdown_with_receipt().is_err());
}

#[test]
fn prompt_outcome_marks_error_frames_and_missing_stop_reason_as_provider_errors() {
    let normal = prompt_outcome(&serde_json::json!({
        "jsonrpc": "2.0", "id": 5, "result": {"stopReason": "end_turn"}
    }));
    assert_eq!(normal.stop_reason, "end_turn");
    assert_eq!(normal.provider_error, None);

    let rejected = prompt_outcome(&serde_json::json!({
        "jsonrpc": "2.0", "id": 5,
        "error": {"code": -32000, "message": "provider API 403: usage limit reached"}
    }));
    assert_eq!(rejected.stop_reason, "error");
    assert_eq!(
        rejected.provider_error.as_deref(),
        Some("session/prompt rejected (code -32000): provider API 403: usage limit reached")
    );

    let malformed = prompt_outcome(&serde_json::json!({
        "jsonrpc": "2.0", "id": 5, "result": {}
    }));
    assert_eq!(malformed.stop_reason, "unknown");
    assert!(malformed
        .provider_error
        .as_deref()
        .is_some_and(|error| error.contains("missing result.stopReason")));
}

#[test]
fn prompt_outcome_ignores_a_null_error_key() {
    // Servers that serialize every field (serde without
    // skip_serializing_if, Python dataclasses.asdict) emit `error: null`
    // on success. `frame.get("error").is_some()` is true for that key, so
    // a non-null filter is what separates success from failure.
    let success = prompt_outcome(&serde_json::json!({
        "jsonrpc": "2.0", "id": 5, "result": {"stopReason": "end_turn"}, "error": null
    }));
    assert_eq!(success.stop_reason, "end_turn");
    assert_eq!(success.provider_error, None);

    // The mirrored shape: a real error alongside a null result.
    let failure = prompt_outcome(&serde_json::json!({
        "jsonrpc": "2.0", "id": 5, "result": null,
        "error": {"code": -32000, "message": "rate limited"}
    }));
    assert_eq!(failure.stop_reason, "error");
    assert!(failure.provider_error.is_some());
}

#[test]
fn prompt_outcome_refuses_to_call_an_incomplete_stop_reason_a_success() {
    for (stop_reason, expected_fragment) in [
        ("max_tokens", "truncated the turn"),
        ("refusal", "declined the turn"),
        ("max_turn_requests", "request budget"),
        ("wat", "unsupported stopReason"),
    ] {
        let outcome = prompt_outcome(&serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "result": {"stopReason": stop_reason}
        }));
        assert_eq!(outcome.stop_reason, stop_reason);
        assert!(
            outcome
                .provider_error
                .as_deref()
                .is_some_and(|error| error.contains(expected_fragment)),
            "{stop_reason} must record a provider_error, got {:?}",
            outcome.provider_error
        );
    }
    // Harness-requested cancellation is recorded by the caller as a
    // cancelled round, not as a provider failure.
    for stop_reason in ["cancelled", "canceled"] {
        let outcome = prompt_outcome(&serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "result": {"stopReason": stop_reason}
        }));
        assert_eq!(outcome.provider_error, None, "{stop_reason}");
    }
}

#[test]
fn prompt_receipt_evidence_excludes_session_level_and_unknown_updates() {
    let session_level = [
        "available_commands_update",
        "current_mode_update",
        "config_option_update",
        "session_info_update",
        "usage_update",
        "future_session_state_update",
    ];
    let prompt_scoped = [
        "user_message_chunk",
        "agent_message_chunk",
        "agent_thought_chunk",
        "tool_call",
        "tool_call_update",
        "plan",
        "plan_update",
        "plan_removed",
    ];

    // Exercise the exact predicate repeatedly without timing or sleeps so
    // scheduler order cannot hide a regression in this acceptance gate.
    for _ in 0..200 {
        for kind in session_level {
            let frame = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {"sessionId": "s", "update": {"sessionUpdate": kind}}
            });
            assert!(!prompt_acceptance_evidence(&frame, "s"), "{kind}");
        }
        for kind in prompt_scoped {
            let frame = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {"sessionId": "s", "update": {"sessionUpdate": kind}}
            });
            assert!(prompt_acceptance_evidence(&frame, "s"), "{kind}");
            let mut wrong_session = frame.clone();
            wrong_session["params"]["sessionId"] = serde_json::json!("other");
            assert!(
                !prompt_acceptance_evidence(&wrong_session, "s"),
                "wrong-session {kind}"
            );
        }
        assert!(prompt_acceptance_evidence(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 41,
                "method": "session/request_permission",
                "params": {"sessionId": "s"}
            }),
            "s"
        ));
        assert!(!prompt_acceptance_evidence(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": {"sessionId": "s"}
            }),
            "s"
        ));
        assert!(!prompt_acceptance_evidence(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 42,
                "method": "session/request_permission",
                "params": {"sessionId": "other"}
            }),
            "s"
        ));
        assert!(!prompt_acceptance_evidence(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 43,
                "method": "future/reverse_rpc",
                "params": {"sessionId": "s"}
            }),
            "s"
        ));
    }
}

#[test]
fn prompt_idle_activity_requires_a_well_formed_frame_for_the_active_session() {
    assert!(active_session_activity(
        &session_update("available_commands_update"),
        "scripted-session"
    ));
    assert!(active_session_activity(
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "session/request_permission",
            "params": {"sessionId": "scripted-session"}
        }),
        "scripted-session"
    ));

    for frame in [
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "another-session",
                "update": {"sessionUpdate": "agent_message_chunk"}
            }
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "session/request_permission",
            "params": {"sessionId": "another-session"}
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": "scripted-session", "update": {}}
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "future/reverse_rpc",
            "params": {"sessionId": "scripted-session"}
        }),
    ] {
        assert!(
            !active_session_activity(&frame, "scripted-session"),
            "{frame}"
        );
    }
}

#[cfg(unix)]
#[test]
fn wrong_session_frame_flood_cannot_prevent_prompt_idle_timeout() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let (mut client, update_tx) = scripted_client();
    let (_response_tx, response) = channel::<serde_json::Value>();
    let stop = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(AtomicUsize::new(0));
    let producer_stop = Arc::clone(&stop);
    let producer_sent = Arc::clone(&sent);
    let producer = std::thread::spawn(move || {
        let mut id = 100_u64;
        while !producer_stop.load(Ordering::Relaxed) {
            let frame = if id.is_multiple_of(2) {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": "stale-session",
                        "update": {"sessionUpdate": "agent_message_chunk"}
                    }
                })
            } else {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "session/request_permission",
                    "params": {"sessionId": "stale-session"}
                })
            };
            if update_tx.send(frame).is_err() {
                break;
            }
            producer_sent.fetch_add(1, Ordering::Relaxed);
            id += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    let started = Instant::now();
    let result = client.drive_prompt(
        (1, response),
        PromptTimeouts {
            idle: Duration::from_millis(40),
            cancel_grace: Duration::from_millis(40),
        },
        &mut |_| panic!("wrong-session traffic must not publish a receipt"),
        &mut |_| panic!("wrong-session updates must not reach the callback"),
        &mut |_| panic!("wrong-session requests must not reach the callback"),
        &mut |_| panic!("wrong-session requests must not be written"),
        &mut || Ok(PromptControl::Continue),
    );
    stop.store(true, Ordering::Relaxed);
    producer.join().expect("join wrong-session producer");

    let error = match result {
        Ok(_) => panic!("wrong-session flood must not mask idle timeout"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("prompt idle"), "{error}");
    assert!(
        sent.load(Ordering::Relaxed) >= 10,
        "test must sustain a real wrong-session flood"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "test timeout path should be bounded"
    );
}

#[cfg(unix)]
#[test]
fn scripted_session_only_update_then_provider_error_publishes_no_receipt() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(session_update("available_commands_update"))
        .expect("queue session update");
    response_tx
        .send(provider_error_response())
        .expect("queue provider error");

    let mut accepted = 0;
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |_| {
            accepted += 1;
            Ok(())
        },
        &mut |_| {},
        &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
    );

    assert!(outcome.provider_error.is_some());
    assert_eq!(accepted, 0);
}

#[cfg(unix)]
#[test]
fn scripted_plan_update_in_response_tail_publishes_receipt_exactly_once() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(session_update("plan_update"))
        .expect("queue plan update");
    response_tx
        .send(provider_error_response())
        .expect("queue provider error");

    let events = std::cell::RefCell::new(Vec::new());
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |receipt| {
            events.borrow_mut().push(format!("receipt:{receipt}"));
            Ok(())
        },
        &mut |update| {
            events.borrow_mut().push(format!(
                "update:{}",
                update["sessionUpdate"].as_str().unwrap_or("unknown")
            ));
        },
        &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
    );

    assert!(outcome.provider_error.is_some());
    assert_eq!(
        events.into_inner(),
        ["receipt:kimi-acp-prompt:1", "update:plan_update"]
    );
}

#[cfg(unix)]
#[test]
fn scripted_plan_removed_before_provider_error_publishes_receipt_exactly_once() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(session_update("plan_removed"))
        .expect("queue plan removal");
    let mut response_tx = Some(response_tx);

    let events = std::cell::RefCell::new(Vec::new());
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |receipt| {
            events.borrow_mut().push(format!("receipt:{receipt}"));
            Ok(())
        },
        &mut |update| {
            events.borrow_mut().push(format!(
                "update:{}",
                update["sessionUpdate"].as_str().unwrap_or("unknown")
            ));
            response_tx
                .take()
                .expect("one terminal response")
                .send(provider_error_response())
                .expect("queue provider error after update");
        },
        &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
    );

    assert!(outcome.provider_error.is_some());
    assert_eq!(
        events.into_inner(),
        ["receipt:kimi-acp-prompt:1", "update:plan_removed"]
    );
}

#[cfg(unix)]
#[test]
fn scripted_reverse_request_before_provider_error_publishes_receipt_exactly_once() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 41,
            "method": "session/request_permission",
            "params": {"sessionId": "scripted-session"}
        }))
        .expect("queue reverse request");
    let mut response_tx = Some(response_tx);

    let mut receipts = Vec::new();
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |receipt| {
            receipts.push(receipt.to_string());
            Ok(())
        },
        &mut |_| {},
        &mut |_| {
            response_tx
                .take()
                .expect("one terminal response")
                .send(provider_error_response())
                .expect("queue provider error after reverse request");
            Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}}))
        },
    );

    assert!(outcome.provider_error.is_some());
    assert_eq!(receipts, ["kimi-acp-prompt:1"]);
}

#[cfg(unix)]
#[test]
fn reverse_request_written_callback_runs_only_after_native_write() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 44,
            "method": "session/request_permission",
            "params": {"sessionId": "scripted-session"}
        }))
        .expect("queue reverse request");
    let mut response_tx = Some(response_tx);
    let events = std::cell::RefCell::new(Vec::new());

    let outcome = client
        .drive_prompt(
            (1, response),
            PromptTimeouts::production(Duration::from_secs(30)),
            &mut |_| Ok(()),
            &mut |_| {},
            &mut |_| {
                events.borrow_mut().push("handler_returned");
                response_tx
                    .take()
                    .expect("one terminal response")
                    .send(provider_error_response())
                    .expect("queue provider error");
                Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}}))
            },
            &mut |request| {
                assert_eq!(request["id"].as_u64(), Some(44));
                events.borrow_mut().push("response_written");
                Ok(())
            },
            &mut || Ok(PromptControl::Continue),
        )
        .expect("scripted prompt completes");

    assert!(outcome.provider_error.is_some());
    assert_eq!(
        events.into_inner(),
        ["handler_returned", "response_written"]
    );
}

#[cfg(unix)]
#[test]
fn reverse_request_write_failure_never_publishes_written_callback() {
    let (mut client, update_tx) = scripted_client();
    client.child.kill().expect("kill scripted ACP sink");
    client.child.wait().expect("reap scripted ACP sink");
    // Closing the owned writer is the deterministic failed-write boundary.
    // Relying only on EPIPE after child reap races kernel pipe propagation
    // when the suite runs in parallel and can accept one buffered write.
    client.stdin = None;
    let (_response_tx, response) = channel();
    update_tx
        .send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 45,
            "method": "session/request_permission",
            "params": {"sessionId": "scripted-session"}
        }))
        .expect("queue reverse request");
    let mut written = 0;

    let result = client.drive_prompt(
        (1, response),
        PromptTimeouts::production(Duration::from_secs(30)),
        &mut |_| Ok(()),
        &mut |_| {},
        &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
        &mut |_| {
            written += 1;
            Ok(())
        },
        &mut || Ok(PromptControl::Continue),
    );

    assert!(
        result.is_err(),
        "broken native pipe must fail the response write"
    );
    assert_eq!(written, 0, "failed native write must not publish a receipt");
}

#[cfg(unix)]
#[test]
fn scripted_permission_request_for_another_session_publishes_no_receipt() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "session/request_permission",
            "params": {"sessionId": "another-session"}
        }))
        .expect("queue mismatched reverse request");
    response_tx
        .send(provider_error_response())
        .expect("queue provider error");

    let mut accepted = 0;
    let mut permission_callbacks = 0;
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |_| {
            accepted += 1;
            Ok(())
        },
        &mut |_| {},
        &mut |_| {
            permission_callbacks += 1;
            Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}}))
        },
    );

    assert!(outcome.provider_error.is_some());
    assert_eq!(accepted, 0);
    assert_eq!(permission_callbacks, 0);
}

#[cfg(unix)]
#[test]
fn scripted_unknown_reverse_method_publishes_no_receipt() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 43,
            "method": "future/reverse_rpc",
            "params": {"sessionId": "scripted-session"}
        }))
        .expect("queue unknown reverse request");
    response_tx
        .send(provider_error_response())
        .expect("queue provider error");

    let mut accepted = 0;
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |_| {
            accepted += 1;
            Ok(())
        },
        &mut |_| {},
        &mut |_| panic!("unknown reverse method must not reach permission callback"),
    );

    assert!(outcome.provider_error.is_some());
    assert_eq!(accepted, 0);
}

#[cfg(unix)]
#[test]
fn scripted_prompt_update_and_terminal_success_publish_receipt_exactly_once() {
    let (mut client, update_tx) = scripted_client();
    let (response_tx, response) = channel();
    update_tx
        .send(session_update("agent_message_chunk"))
        .expect("queue prompt update");
    response_tx
        .send(terminal_success_response())
        .expect("queue terminal success");

    let mut receipts = Vec::new();
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |receipt| {
            receipts.push(receipt.to_string());
            Ok(())
        },
        &mut |_| {},
        &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
    );

    assert_eq!(outcome.provider_error, None);
    assert_eq!(outcome.stop_reason, "end_turn");
    assert_eq!(receipts, ["kimi-acp-prompt:1"]);
}

#[cfg(unix)]
#[test]
fn scripted_terminal_success_without_updates_publishes_receipt_exactly_once() {
    let (mut client, _update_tx) = scripted_client();
    let (response_tx, response) = channel();
    response_tx
        .send(terminal_success_response())
        .expect("queue terminal success");

    let mut receipts = Vec::new();
    let outcome = drive_scripted_prompt(
        &mut client,
        response,
        &mut |receipt| {
            receipts.push(receipt.to_string());
            Ok(())
        },
        &mut |_| {},
        &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
    );

    assert_eq!(outcome.provider_error, None);
    assert_eq!(outcome.stop_reason, "end_turn");
    assert_eq!(receipts, ["kimi-acp-prompt:1"]);
}
