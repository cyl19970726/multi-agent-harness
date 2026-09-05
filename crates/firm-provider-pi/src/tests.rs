use super::{
    confirm_pi_session_flush, ensure_session_has_no_persisted_thinking,
    value_contains_persisted_thinking, PermissionCeiling, PiRpcClient,
};

#[test]
fn prompt_and_agent_settled_share_one_provider_cycle_identity() {
    let dir = std::env::temp_dir().join(format!(
        "pi-rpc-cycle-correlation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let session_file = dir.join("session.jsonl");
    std::fs::write(&session_file, "{\"type\":\"agent_start\"}\n").unwrap();
    let shim = dir.join("pi");
    let script = format!(
        r##"#!/usr/bin/env python3
import sys, json
state_calls = 0
for line in sys.stdin:
    cmd = json.loads(line)
    cid = cmd.get('id')
    kind = cmd.get('type')
    if kind == 'get_state':
        state_calls += 1
        if state_calls == 1:
            print(json.dumps({{'type':'agent_settled', 'stale':True}}), flush=True)
        print(json.dumps({{'id': cid, 'type':'response', 'command':'get_state', 'success':True, 'data':{{'sessionFile':'{session_file}', 'autoCompactionEnabled':False, 'isStreaming':False, 'pendingMessageCount':0, 'steeringMode':'one-at-a-time', 'followUpMode':'one-at-a-time'}}}}), flush=True)
    elif kind == 'prompt':
        print(json.dumps({{'id': cid, 'type':'response', 'command':'prompt', 'success':True}}), flush=True)
        print(json.dumps({{'type':'turn_end', 'message':{{'content':[{{'type':'text', 'text':'done'}}]}}}}), flush=True)
        print(json.dumps({{'type':'agent_settled'}}), flush=True)
"##,
        session_file = session_file.display(),
    );
    std::fs::write(&shim, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).unwrap();
    }
    let mut client = PiRpcClient::spawn(
        shim.to_str().unwrap(),
        super::PiSpawnOptions {
            cwd: &dir,
            model: None,
            resume_session_file: None,
            session_dir: &dir,
            member_name: "cycle-correlation",
            collaboration_env: &[],
            tools: None,
            permission_ceiling: PermissionCeiling::FullAccess,
        },
    )
    .unwrap();
    let outcome = client
        .prompt(
            "one cycle",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(
                std::time::Duration::from_secs(2),
            ),
            |_| Ok(()),
            |_, _| Ok(()),
            |_| {},
            harness_runtime_contract::CycleControl::default,
        )
        .unwrap();
    assert_eq!(
        outcome.final_text, "done",
        "stale pre-dispatch idle was ignored"
    );
    let correlation = outcome.native_correlation;
    assert_eq!(
        correlation.terminal_provider_input_id.as_deref(),
        Some(correlation.provider_input_id.as_str())
    );
    assert_eq!(
        correlation.exact_terminal_ref.as_deref(),
        Some(format!("pi.agent_settled:{}", correlation.provider_input_id).as_str())
    );
    drop(client);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Spawn a minimal fake `pi --mode rpc` shim and exercise the RPC-level
/// adapter surface: handshake, follow_up acknowledgement, queue
/// observation, and the --tools permission compilation in the spawn argv.
#[test]
fn follow_up_queue_snapshot_and_tools_compilation() {
    let dir = std::env::temp_dir().join(format!(
        "pi-rpc-rpc-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let session_file = dir.join("session.jsonl");
    std::fs::write(&session_file, "{\"type\":\"agent_start\"}\n").unwrap();
    let args_marker = dir.join("argv.json");
    let shim = dir.join("pi");
    let script = format!(
        r##"#!/usr/bin/env python3
import sys, json, os
with open('{args_marker}', 'w') as f:
    json.dump(sys.argv[1:], f)
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        cmd = json.loads(line)
    except json.JSONDecodeError:
        continue
    t = cmd.get('type', '')
    cid = cmd.get('id', '')
    if t == 'get_state':
        resp = {{'id': cid, 'type': 'response', 'command': 'get_state', 'success': True,
                 'data': {{'sessionFile': '{session_file}', 'autoCompactionEnabled': False,
                           'steeringMode': 'one-at-a-time', 'followUpMode': 'one-at-a-time',
                           'pendingMessageCount': 2, 'isStreaming': False}}}}
    elif t == 'follow_up':
        resp = {{'id': cid, 'type': 'response', 'command': 'follow_up', 'success': True}}
    else:
        resp = {{'id': cid, 'type': 'response', 'command': t, 'success': True}}
    print(json.dumps(resp), flush=True)
"##,
        args_marker = args_marker.display(),
        session_file = session_file.display(),
    );
    std::fs::write(&shim, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&shim).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&shim, perms).unwrap();
    }

    let mut client = PiRpcClient::spawn(
        shim.to_str().unwrap(),
        super::PiSpawnOptions {
            cwd: &dir,
            model: None,
            resume_session_file: None,
            session_dir: &dir,
            member_name: "rpc-test",
            collaboration_env: &[],
            tools: Some("read,grep,find,ls"),
            permission_ceiling: PermissionCeiling::ReadOnly,
        },
    )
    .expect("spawn shim");

    // Permission compilation proof: the allowlist is in the process argv.
    let argv: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(&args_marker).unwrap()).unwrap();
    let tools_pos = argv.iter().position(|arg| arg == "--tools");
    assert_eq!(
        tools_pos.map(|pos| argv[pos + 1].as_str()),
        Some("read,grep,find,ls"),
        "restricted ceiling must compile to --tools in the spawn argv: {argv:?}"
    );

    let ack = client.follow_up("queued at the native boundary").unwrap();
    assert_eq!(ack.get("success").and_then(|v| v.as_bool()), Some(true));

    let snapshot = client.queue_snapshot().unwrap();
    assert_eq!(
        snapshot["pending_message_count"].as_u64(),
        Some(2),
        "queue observation must surface the native pending count: {snapshot}"
    );
    assert_eq!(snapshot["steering_mode"].as_str(), Some("one-at-a-time"));

    let (children, children_evidence) = client.writable_children_drain_proof();
    assert_eq!(
        children,
        harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
        "reviewed ReadOnly argv proves writable-child non-creation: {children_evidence}"
    );
    let flush = confirm_pi_session_flush(&session_file)
        .expect("a complete JSONL line must receive file+directory sync evidence");
    assert!(flush.contains("sync_all confirmed"), "{flush}");

    drop(client);

    let full_access = PiRpcClient::spawn(
        shim.to_str().unwrap(),
        super::PiSpawnOptions {
            cwd: &dir,
            model: None,
            resume_session_file: None,
            session_dir: &dir,
            member_name: "rpc-full-access-test",
            collaboration_env: &[],
            tools: None,
            permission_ceiling: PermissionCeiling::FullAccess,
        },
    )
    .expect("spawn FullAccess shim");
    let (children, children_evidence) = full_access.writable_children_drain_proof();
    assert_eq!(
        children,
        harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown,
        "FullAccess cannot claim child drain without a native job inventory: {children_evidence}"
    );
    assert!(children_evidence.contains("may escape the owned process group"));
    drop(full_access);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn flush_evidence_requires_a_complete_regular_jsonl_file() {
    let dir = std::env::temp_dir().join(format!(
        "pi-rpc-flush-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let session_file = dir.join("session.jsonl");
    std::fs::write(&session_file, "{\"type\":\"session\"}").expect("write incomplete session");
    let error = confirm_pi_session_flush(&session_file)
        .expect_err("path existence without a complete record is not flush proof");
    assert!(error.contains("incomplete final JSONL record"));

    std::fs::write(&session_file, "{\"type\":\"session\"}\n").expect("complete session");
    confirm_pi_session_flush(&session_file).expect("complete file can be durably synced");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let linked = dir.join("linked-session.jsonl");
        symlink(&session_file, &linked).expect("create symlink fixture");
        let error = confirm_pi_session_flush(&linked)
            .expect_err("a symlink must not be promoted to native flush evidence");
        assert!(error.contains("regular non-symlink"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn detects_persisted_thinking_blocks_without_rejecting_level_metadata() {
    assert!(value_contains_persisted_thinking(&serde_json::json!({
        "type": "message",
        "message": {"content": [{"type": "thinking", "thinking": "private"}]}
    })));
    assert!(value_contains_persisted_thinking(&serde_json::json!({
        "type": "message",
        "message": {"content": [{"type": "text", "thinkingSignature": "sig"}]}
    })));
    assert!(!value_contains_persisted_thinking(&serde_json::json!({
        "type": "thinking_level_change",
        "thinkingLevel": "off"
    })));
}

#[test]
fn rejects_a_native_session_that_would_replay_thinking() {
    let dir = std::env::temp_dir().join(format!(
        "harness-pi-rpc-thinking-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("session.jsonl");
    std::fs::write(
            &path,
            "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"content\":[{\"type\":\"thinking\",\"thinking\":\"private\"}]}}\n",
        )
        .expect("write session");
    let error = ensure_session_has_no_persisted_thinking(&path).unwrap_err();
    assert!(error.to_string().contains("persisted provider thinking"));
    std::fs::remove_dir_all(dir).expect("remove temp dir");
}

// ---------------------------------------------------------------------------
// SPEC-TYPED-CYCLE-OUTCOME-01 §5: the S1 assertion family against Pi RPC.

#[cfg(unix)]
mod cycle_conformance {
    use super::*;
    use harness_runtime_host::OwnedProcessGroupRegistration;
    use std::collections::HashMap;
    use std::io::BufWriter;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn scripted_pi_client() -> (PiRpcClient, Sender<serde_json::Value>) {
        scripted_pi_client_with_stdin_sink("cat >/dev/null")
    }

    /// A scripted client whose child appends everything it reads on stdin to
    /// `log_path`, so tests can observe exactly which frames were written.
    fn scripted_pi_client_logging(
        log_path: &std::path::Path,
    ) -> (PiRpcClient, Sender<serde_json::Value>) {
        scripted_pi_client_with_stdin_sink(&format!("cat >> {}", log_path.display()))
    }

    fn scripted_pi_client_with_stdin_sink(
        sink_command: &str,
    ) -> (PiRpcClient, Sender<serde_json::Value>) {
        let mut child = Command::new("sh")
            .args(["-c", sink_command])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn scripted pi sink");
        let stdin = child.stdin.take().expect("scripted pi stdin");
        let owned_process_group =
            OwnedProcessGroupRegistration::new(&mut child).expect("scripted process group");
        let (event_tx, incoming) = mpsc::channel();
        (
            PiRpcClient {
                child,
                owned_process_group,
                stdin: BufWriter::new(stdin),
                next_request_id: 0,
                pending: Arc::new(Mutex::new(HashMap::new())),
                incoming,
                reader: None,
                stderr_tail: Arc::new(Mutex::new(String::new())),
                session_file: "scripted-session.jsonl".to_string(),
                permission_ceiling: PermissionCeiling::FullAccess,
                tools_allowlist: None,
                last_observation: None,
                released: false,
            },
            event_tx,
        )
    }

    fn pi_response(id: &str, command: &str, data: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": id, "type": "response", "command": command, "success": true, "data": data
        })
    }

    fn pi_state(streaming: bool) -> serde_json::Value {
        serde_json::json!({
            "sessionFile": "scripted-session.jsonl",
            "autoCompactionEnabled": false,
            "isStreaming": streaming,
            "pendingMessageCount": 0,
            "steeringMode": "one-at-a-time",
            "followUpMode": "one-at-a-time"
        })
    }

    fn pi_timeouts() -> harness_runtime_contract::CycleTimeouts {
        // A generous acceptance bound keeps the scripted prompt answer
        // deterministic under build load; only the A2 no-receipt fixture
        // uses the tiny bound (it is the fixture that must expire).
        harness_runtime_contract::CycleTimeouts {
            input_acceptance: Duration::from_secs(2),
            transport_liveness: Duration::from_millis(1),
            control_settle: Duration::ZERO,
        }
    }

    fn pi_no_receipt_timeouts() -> harness_runtime_contract::CycleTimeouts {
        harness_runtime_contract::CycleTimeouts {
            input_acceptance: Duration::from_millis(1),
            ..pi_timeouts()
        }
    }

    struct PiScript {
        answers: Vec<(String, serde_json::Value)>,
        events: Vec<serde_json::Value>,
        /// Real wall-clock delay before the scripted events are sent — the
        /// "silent tool interval" the A4 regression needs to be real.
        delay_events_ms: u64,
        /// Scripted events are sent only after this many answers have landed
        /// (`prompt_dyn` discards pre-dispatch events at start, and a Host
        /// interrupt must be polled before the terminal event arrives).
        events_after: usize,
        disconnect_after: bool,
    }

    fn drive_pi_cycle(
        script: PiScript,
        timeouts: &harness_runtime_contract::CycleTimeouts,
        control: impl FnMut() -> harness_runtime_contract::CycleControl + Send + 'static,
    ) -> Result<harness_runtime_contract::ExecutionCycleOutcome, String> {
        drive_pi_cycle_with(scripted_pi_client(), script, timeouts, control)
    }

    fn drive_pi_cycle_logging(
        log_path: &std::path::Path,
        script: PiScript,
        timeouts: &harness_runtime_contract::CycleTimeouts,
        control: impl FnMut() -> harness_runtime_contract::CycleControl + Send + 'static,
    ) -> Result<harness_runtime_contract::ExecutionCycleOutcome, String> {
        drive_pi_cycle_with(
            scripted_pi_client_logging(log_path),
            script,
            timeouts,
            control,
        )
    }

    fn drive_pi_cycle_with(
        (client, event_tx): (PiRpcClient, Sender<serde_json::Value>),
        script: PiScript,
        timeouts: &harness_runtime_contract::CycleTimeouts,
        mut control: impl FnMut() -> harness_runtime_contract::CycleControl + Send + 'static,
    ) -> Result<harness_runtime_contract::ExecutionCycleOutcome, String> {
        let pending = Arc::clone(&client.pending);
        let timeouts = *timeouts;
        let handle = std::thread::spawn(move || {
            let mut adapter = crate::team_runtime::PiTeamRuntime::new(client);
            harness_runtime_contract::TeamRuntimeAdapter::run_cycle(
                &mut adapter,
                "conformance cycle",
                timeouts,
                &mut |_receipt| Ok(()),
                &mut |_pending, _result| Ok(()),
                &mut |_event| {},
                &mut control,
            )
        });
        let mut events = script.events.into_iter();
        let mut answered = 0usize;
        for (command, frame) in script.answers {
            let deadline = Instant::now() + Duration::from_secs(2);
            let id = loop {
                if handle.is_finished() {
                    let early = handle
                        .join()
                        .map_err(|_| "adapter thread panicked".to_string())?;
                    return Err(format!(
                        "adapter finished before the scripted {command} answer: {early:?}"
                    ));
                }
                let waiter = pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .keys()
                    .next()
                    .cloned();
                if let Some(id) = waiter {
                    break id;
                }
                if Instant::now() >= deadline {
                    let keys: Vec<String> = pending
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .keys()
                        .cloned()
                        .collect();
                    panic!(
                        "scripted {command} never arrived (thread_finished={}, pending={keys:?})",
                        handle.is_finished()
                    );
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&id)
                .expect("request waiter")
                .send(frame)
                .map_err(|error| error.to_string())?;
            answered += 1;
            if answered == script.events_after {
                if script.delay_events_ms > 0 {
                    std::thread::sleep(Duration::from_millis(script.delay_events_ms));
                }
                for event in events.by_ref() {
                    event_tx.send(event).map_err(|error| error.to_string())?;
                }
            }
        }
        if script.disconnect_after {
            drop(event_tx);
        }
        handle
            .join()
            .map_err(|_| "adapter thread panicked".to_string())?
            .map_err(|error| error.to_string())
    }

    struct PiCycleConformanceFixture;

    impl harness_runtime_contract::CycleConformanceFixture for PiCycleConformanceFixture {
        type Error = String;

        fn run_receipt_then_silence(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let outcome = drive_pi_cycle(
                PiScript {
                    answers: vec![
                        (
                            "prompt".to_string(),
                            pi_response("pi-rpc-1", "prompt", serde_json::json!({})),
                        ),
                        (
                            "get_state".to_string(),
                            pi_response("pi-rpc-2", "get_state", pi_state(false)),
                        ),
                    ],
                    events: vec![
                        serde_json::json!({"type": "turn_end", "message": {"content": [{"type": "text", "text": "done"}]}}),
                        serde_json::json!({"type": "agent_settled"}),
                    ],
                    events_after: 1,
                    delay_events_ms: 0,
                    disconnect_after: false,
                },
                timeouts,
                harness_runtime_contract::CycleControl::default,
            )?;
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: outcome.interrupt.clone(),
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Outcome(Box::new(
                    outcome,
                )),
            })
        }

        fn run_no_receipt(
            &mut self,
            _timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let error = match drive_pi_cycle(
                PiScript {
                    answers: vec![(
                        "prompt".to_string(),
                        serde_json::json!({"id": "pi-rpc-1", "type": "response", "command": "prompt", "success": false, "error": "scripted refusal"}),
                    )],
                    events: Vec::new(),
                    events_after: 0,
                    delay_events_ms: 0,
                    disconnect_after: false,
                },
                &pi_no_receipt_timeouts(),
                harness_runtime_contract::CycleControl::default,
            ) {
                Ok(_) => return Err("a never-accepted cycle produced an outcome".to_string()),
                Err(error) => error,
            };
            assert!(error.contains("prompt"), "{error}");
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: None,
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Failed(
                    harness_runtime_contract::CycleFailureDisposition::InputNeverAccepted,
                ),
            })
        }

        fn run_transport_dies_after_receipt(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let error = match drive_pi_cycle(
                PiScript {
                    answers: vec![(
                        "prompt".to_string(),
                        pi_response("pi-rpc-1", "prompt", serde_json::json!({})),
                    )],
                    events: Vec::new(),
                    events_after: 1,
                    delay_events_ms: 0,
                    disconnect_after: true,
                },
                timeouts,
                harness_runtime_contract::CycleControl::default,
            ) {
                Ok(_) => return Err("a dead transport produced an outcome".to_string()),
                Err(error) => error,
            };
            assert!(error.contains("disconnected"), "{error}");
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: None,
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Failed(
                    harness_runtime_contract::CycleFailureDisposition::AcceptedOutcomeUnknown,
                ),
            })
        }

        fn run_interrupt_not_acknowledged(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            let mut first = true;
            let error = match drive_pi_cycle(
                PiScript {
                    answers: vec![
                        (
                            "prompt".to_string(),
                            pi_response("pi-rpc-1", "prompt", serde_json::json!({})),
                        ),
                        (
                            "abort".to_string(),
                            pi_response("pi-rpc-2", "abort", serde_json::json!({})),
                        ),
                    ],
                    events: Vec::new(),
                    events_after: 0,
                    delay_events_ms: 0,
                    disconnect_after: false,
                },
                timeouts,
                move || {
                    if std::mem::take(&mut first) {
                        harness_runtime_contract::CycleControl {
                            interrupt: true,
                            ..Default::default()
                        }
                    } else {
                        harness_runtime_contract::CycleControl::default()
                    }
                },
            ) {
                Ok(_) => return Err("an unacknowledged abort produced an outcome".to_string()),
                Err(error) => error,
            };
            assert!(error.contains("PI_CONTROL_SETTLE_TIMEOUT"), "{error}");
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: None,
                control_unproven: true,
                result: harness_runtime_contract::CycleConformanceResult::Failed(
                    harness_runtime_contract::CycleFailureDisposition::AcceptedOutcomeUnknown,
                ),
            })
        }

        fn run_host_interrupt(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            // The acknowledged path needs a real settle window: pi's first
            // recv always times out before the control poll, and a zero
            // control_settle would expire before agent_settled arrives.
            let settle = harness_runtime_contract::CycleTimeouts {
                control_settle: Duration::from_secs(2),
                ..*timeouts
            };
            let mut first = true;
            let outcome = drive_pi_cycle(
                PiScript {
                    answers: vec![
                        (
                            "prompt".to_string(),
                            pi_response("pi-rpc-1", "prompt", serde_json::json!({})),
                        ),
                        (
                            "abort".to_string(),
                            pi_response("pi-rpc-2", "abort", serde_json::json!({})),
                        ),
                        (
                            "get_state".to_string(),
                            pi_response("pi-rpc-3", "get_state", pi_state(false)),
                        ),
                    ],
                    events: vec![serde_json::json!({"type": "agent_settled"})],
                    events_after: 2,
                    delay_events_ms: 0,
                    disconnect_after: false,
                },
                &settle,
                move || {
                    if std::mem::take(&mut first) {
                        harness_runtime_contract::CycleControl {
                            interrupt: true,
                            ..Default::default()
                        }
                    } else {
                        harness_runtime_contract::CycleControl::default()
                    }
                },
            )?;
            Ok(harness_runtime_contract::CycleConformanceOutcome {
                interrupt: outcome.interrupt.clone(),
                control_unproven: false,
                result: harness_runtime_contract::CycleConformanceResult::Outcome(Box::new(
                    outcome,
                )),
            })
        }

        fn run_adapter_policy_interrupt(
            &mut self,
            timeouts: &harness_runtime_contract::CycleTimeouts,
            _reason: &str,
        ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
            self.run_receipt_then_silence(timeouts)
        }
    }

    #[test]
    fn pi_passes_the_s1_cycle_conformance_family() {
        let timeouts = pi_timeouts();
        let mut fixture = PiCycleConformanceFixture;
        harness_runtime_contract::assert_a1_accepted_input_survives_silence(
            &mut fixture,
            &timeouts,
        )
        .expect("A1");
        harness_runtime_contract::assert_a2_delivery_timeout_fails_closed(&mut fixture, &timeouts)
            .expect("A2");
        harness_runtime_contract::assert_a3_transport_death_fails_closed(&mut fixture, &timeouts)
            .expect("A3");
        harness_runtime_contract::assert_a5_control_settle_only_bounds_control(
            &mut fixture,
            &timeouts,
        )
        .expect("A5");
        harness_runtime_contract::assert_b1_host_interrupt_attribution(&mut fixture, &timeouts)
            .expect("B1");
    }

    #[test]
    fn pi_a4_silence_after_acceptance_never_aborts() {
        // A4: a REAL silent interval past the OLD idle_timeout never reaches
        // the abort write; the cycle completes normally (B4). The scripted
        // child logs every stdin frame, so "no abort write" is observed, not
        // inferred.
        let log = std::env::temp_dir().join(format!("pi-a4-stdin-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let outcome = drive_pi_cycle_logging(
            &log,
            PiScript {
                answers: vec![
                    ("prompt".to_string(), pi_response("pi-rpc-1", "prompt", serde_json::json!({}))),
                    ("get_state".to_string(), pi_response("pi-rpc-2", "get_state", pi_state(false))),
                ],
                events: vec![
                    serde_json::json!({"type": "turn_end", "message": {"content": [{"type": "text", "text": "done"}]}}),
                    serde_json::json!({"type": "agent_settled"}),
                ],
                events_after: 1,
                delay_events_ms: 250,
                disconnect_after: false,
            },
            &pi_timeouts(),
            harness_runtime_contract::CycleControl::default,
        )
        .expect("a silent accepted cycle completes");
        assert_eq!(outcome.interrupt, None);
        let written = std::fs::read_to_string(&log).expect("stdin log");
        std::fs::remove_file(&log).expect("remove A4 stdin log");
        assert!(
            !written.contains("\"type\": \"abort\"") && !written.contains("\"type\":\"abort\""),
            "no abort frame may be written during silence: {written}"
        );
    }
}

/// C1 (pi): the pi adapter never sets `provider_terminal_failure` from its
/// own frame loop (its terminal is a clean agent_settled), so the assertion
/// is pinned against a synthetically failed outcome — it proves the shared
/// settlement projection pi's StartCycle arm uses can never yield Satisfied
/// with a failure present (#709).
#[test]
fn pi_c1_terminal_failure_settles_unsatisfied() {
    let outcome = harness_runtime_contract::ExecutionCycleOutcome {
        final_text: String::new(),
        provider_terminal_failure: Some(harness_runtime_contract::ProviderTerminalFailure {
            reason: "api_overloaded".into(),
            http_status: Some(529),
        }),
        interrupt: None,
        close_requested_by_harness: false,
        tool_call_count: 0,
        native_correlation: harness_runtime_contract::NativeCycleCorrelation {
            provider_input_id: "pi-cycle-1".into(),
            input_acceptance_receipt: harness_runtime_contract::ControlTransportReceipt {
                command: "deliver".into(),
                response_id: Some("pi-receipt-1".into()),
                success: true,
            },
            terminal_provider_input_id: Some("pi-cycle-1".into()),
            exact_terminal_ref: Some("pi.agent_settled:pi-cycle-1".into()),
        },
        control_receipts: vec![],
        terminal_observation: harness_runtime_contract::CycleRuntimeObservation {
            transport_alive: true,
            process_alive: true,
            is_streaming: Some(false),
            pending_message_count: Some(0),
            steering_mode: None,
            follow_up_mode: None,
            settled_boundary_observed: true,
        },
    };
    let receipt = harness_runtime_contract::EffectReceipt::for_cycle(
        "conformance-c1",
        harness_core::ProviderBindingAdmission::Active,
        harness_runtime_contract::CycleSettlement::from_cycle_outcome(&outcome),
    );
    harness_runtime_contract::assert_c1_terminal_failure_unsatisfied(&receipt).expect("C1");
}
