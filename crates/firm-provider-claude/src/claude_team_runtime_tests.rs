use super::*;

fn protected_test_environment(
    token: &str,
) -> harness_runtime_contract::CollaborationCapabilityEnvironment {
    let envelope = harness_runtime_contract::CollaborationCapabilityEnvelope::new(
        harness_runtime_contract::CollaborationCapabilitySecret::new(token.to_string()).unwrap(),
        harness_runtime_contract::CollaborationCapabilityBinding {
            team_run_id: "team-run-test".into(),
            member_run_id: "member-run-test".into(),
            member_run_generation: 1,
            agent_session_id: "session-test".into(),
            agent_session_generation: 1,
            node_daemon_id: "daemon-test".into(),
            node_daemon_generation: 1,
            supervisor_id: "supervisor-test".into(),
            supervisor_generation: 1,
        },
        COLLABORATION_CAPABILITY_MECHANISM,
    )
    .unwrap();
    collaboration_agent_tool_environment(&envelope).unwrap()
}

#[test]
fn start_frame_uses_the_shared_versioned_runner_contract() {
    let token = "cd".repeat(32);
    let config = ClaudeTeamRuntimeConfig {
        runner_path: PathBuf::from("runner.mjs"),
        cwd: PathBuf::from("/tmp/project"),
        team_run_id: "team-run-test".into(),
        member_run_id: "member-run-test".into(),
        member_name: "Claude test".into(),
        role_label: "developer".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "bypassPermissions".into(),
        allowed_tools: None,
        disallowed_tools: None,
        setting_sources: Vec::new(),
        resume_session_id: None,
        environment: protected_test_environment(&token),
    };
    assert!(!format!("{config:?}").contains(&token));
    let frame = config.start_frame().expect("shared contract must parse");
    let contract: Value = serde_json::from_str(include_str!(
        "../../../apps/claude-member-runner/contract/runner-v1.json"
    ))
    .unwrap();
    assert_eq!(
        frame.pointer("/payload/protocolVersion"),
        contract.get("protocolVersion")
    );
    assert_eq!(
        frame.pointer("/payload/protocolFingerprint"),
        contract.get("fingerprint")
    );
}

#[test]
fn capability_surface_does_not_overclaim_goal_steer_or_strict_quiesce() {
    let bindings = ClaudeTeamRuntime::capability_bindings();
    let status = |name: &str| {
        bindings
            .iter()
            .find(|binding| binding.capability == name)
            .map(|binding| binding.status)
            .expect("capability")
    };
    assert_eq!(status("start_cycle"), CapabilityStatus::Supported);
    assert_eq!(
        status("interrupt_current_cycle"),
        CapabilityStatus::Supported
    );
    assert_eq!(status("close_runtime"), CapabilityStatus::Supported);
    assert_eq!(
        status("inject_current_cycle"),
        CapabilityStatus::Unsupported
    );
    assert_eq!(
        status("inspect_continuation"),
        CapabilityStatus::Unsupported
    );
    assert_eq!(status("resume_continuation"), CapabilityStatus::Unsupported);
    assert_eq!(status("quiesce"), CapabilityStatus::Degraded);
    assert_eq!(status("release"), CapabilityStatus::Degraded);
}

#[test]
fn assistant_projection_discards_thinking_and_counts_tool_use() {
    let data = json!({
        "content": [
            {"type": "thinking", "thinking": "private chain"},
            {"type": "text", "text": "safe"},
            {"type": "tool_use", "name": "Read"},
            {"type": "redacted_thinking", "data": "secret"},
            {"type": "text", "text": " output"}
        ]
    });
    assert_eq!(assistant_projection(&data), ("safe output".into(), 1));
}

#[test]
fn runner_events_are_deny_unknown_at_the_envelope_boundary() {
    let event = RunnerEvent::parse(
        r#"{"event":"session_bound","data":{"sessionId":"native-1","tag":"team:member","title":"Claude member","providerVersion":"2.1.220","model":null,"effort":null}}"#,
    )
    .unwrap();
    assert_eq!(event.name, "session_bound");
    assert_eq!(event.data["sessionId"], "native-1");
    assert!(RunnerEvent::parse(r#"{"data":{}}"#).is_err());
}

#[test]
fn sdk_package_must_be_exactly_repo_reviewed_version() {
    let root = unique_temp_dir("claude-version");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let runner = bin.join("runner.mjs");
    fs::write(&runner, "// test").unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@anthropic-ai/claude-agent-sdk":"0.3.220"}}"#,
    )
    .unwrap();
    verify_runner_sdk_version(&runner).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@anthropic-ai/claude-agent-sdk":"^0.3.220"}}"#,
    )
    .unwrap();
    assert!(verify_runner_sdk_version(&runner)
        .unwrap_err()
        .to_string()
        .contains("VERSION_UNREVIEWED"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn exact_native_session_file_is_synced_without_following_symlinks() {
    let root = unique_temp_dir("claude-session-sync");
    let project = root.join("project-a");
    fs::create_dir_all(&project).unwrap();
    let session = project.join("session-123.jsonl");
    fs::write(&session, "{\"type\":\"result\"}\n").unwrap();
    assert_eq!(
        find_and_sync_claude_session_under(&root, "session-123").unwrap(),
        session
    );
    assert!(find_and_sync_claude_session_under(&root, "other").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn persistent_transport_interrupts_then_closes_and_retains_same_native_session() {
    let root = unique_temp_dir("claude-transport");
    let bin = root.join("bin");
    let cwd = root.join("workspace");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@anthropic-ai/claude-agent-sdk":"0.3.220"}}"#,
    )
    .unwrap();
    let runner = bin.join("runner.mjs");
    fs::write(
        &runner,
        r#"
import readline from "node:readline";
const input = readline.createInterface({input: process.stdin, crlfDelay: Infinity});
const emit = (event, data) => process.stdout.write(JSON.stringify({event, data}) + "\n");
let sessionId = null;
let bound = false;
for await (const line of input) {
  const frame = JSON.parse(line);
  if (frame.command === "start") {
    sessionId = frame.payload.resumeSessionId ?? "native-session-1";
    emit("member_started", {
      memberRunId: frame.payload.memberRunId,
      cwd: frame.payload.cwd,
      permissionMode: frame.payload.permissionMode,
      ownedPathCount: frame.payload.ownedPaths.length,
      resumed: Boolean(frame.payload.resumeSessionId),
    });
  } else if (frame.command === "deliver") {
    const id = frame.payload.id;
    emit("delivered", {id, kind: frame.payload.kind});
    emit("consumed", {id, kind: frame.payload.kind, sessionId});
    if (!bound) {
      bound = true;
      emit("session_bound", {
        sessionId,
        tag: "team-run-test:member-run-test",
        title: "Claude test · developer",
        providerVersion: "2.1.220",
        model: "claude-test",
        effort: null,
      });
    }
  } else if (frame.command === "interrupt") {
    emit("interrupted", {stillQueued: [], abandonedTriggerMessageIds: []});
    emit("query_ended_by_interrupt", {sessionId, error: "test interrupt"});
    emit("member_resumed_after_interrupt", {sessionId});
  } else if (frame.command === "close") {
    emit("member_closed", {
      sessionId,
      reason: frame.payload.reason,
      undelivered: [],
      evidenceRefs: [],
    });
    break;
  }
}

"#,
    )
    .unwrap();

    let config = ClaudeTeamRuntimeConfig {
        runner_path: runner,
        cwd,
        team_run_id: "team-run-test".into(),
        member_run_id: "member-run-test".into(),
        member_name: "Claude test".into(),
        role_label: "developer".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "bypassPermissions".into(),
        allowed_tools: None,
        disallowed_tools: None,
        setting_sources: vec!["project".into(), "user".into()],
        resume_session_id: None,
        environment: harness_runtime_contract::CollaborationCapabilityEnvironment::empty(),
    };
    let mut transport = ClaudeRunnerTransport::spawn(&config).unwrap();
    let mut accepted = None;
    let outcome = transport
        .run_cycle(
            "interrupt this cycle",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_secs(5)),
            &mut |receipt| {
                accepted = receipt.response_id.clone();
                Ok(())
            },
            &mut |_pending, _result| Ok(()),
            &mut |_event| {},
            &mut || CycleControl {
                interrupt: true,
                ..CycleControl::default()
            },
        )
        .unwrap();
    assert!(accepted
        .as_deref()
        .is_some_and(|receipt| receipt.contains("native-session-1")));
    assert_eq!(
        outcome.interrupt,
        Some(harness_runtime_contract::InterruptCause::HostControl)
    );
    assert!(outcome
        .native_correlation
        .terminal_provider_input_id
        .as_deref()
        .is_some_and(|terminal| terminal == outcome.native_correlation.provider_input_id));
    assert!(outcome
        .native_correlation
        .exact_terminal_ref
        .as_deref()
        .is_some_and(|terminal| terminal.starts_with("claude_sdk.interrupt_resume:")));
    assert!(!outcome.close_requested_by_harness);
    assert!(outcome.terminal_observation.settled_boundary_observed);
    assert!(transport.last_interrupt_resumed_same_session);
    assert_eq!(transport.native_session_id, "native-session-1");

    transport.close("harness_team_close").unwrap();
    let closed = transport
        .wait_for_member_closed(Duration::from_secs(5))
        .unwrap();
    assert_eq!(closed["sessionId"], "native-session-1");
    assert_eq!(closed["reason"], "harness_team_close");
    assert!(matches!(transport.state, TransportState::Closed));
    assert_eq!(transport.native_session_id, "native-session-1");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn accepted_cycle_can_outlive_the_delivery_timeout_and_still_complete() {
    let root = unique_temp_dir("claude-long-accepted-cycle");
    let bin = root.join("bin");
    let cwd = root.join("workspace");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@anthropic-ai/claude-agent-sdk":"0.3.220"}}"#,
    )
    .unwrap();
    let runner = bin.join("runner.mjs");
    fs::write(
        &runner,
        r#"
import readline from "node:readline";
const input = readline.createInterface({input: process.stdin, crlfDelay: Infinity});
const emit = (event, data) => process.stdout.write(JSON.stringify({event, data}) + "\n");
for await (const line of input) {
  const frame = JSON.parse(line);
  if (frame.command === "start") {
    emit("member_started", {memberRunId: frame.payload.memberRunId, cwd: frame.payload.cwd, permissionMode: frame.payload.permissionMode, ownedPathCount: 0, resumed: false});
  } else if (frame.command === "deliver") {
    emit("delivered", {id: frame.payload.id, kind: frame.payload.kind});
    emit("consumed", {id: frame.payload.id, kind: frame.payload.kind, sessionId: "native-long-cycle"});
    emit("session_bound", {sessionId: "native-long-cycle", tag: "team-run-test:member-run-test", title: "Claude test · developer", providerVersion: "2.1.220", model: null, effort: null});
    setTimeout(() => {
      emit("assistant_message", {sessionId: "native-long-cycle", content: [{type: "text", text: "completed after a long silent tool"}]});
      emit("turn_complete", {sessionId: "native-long-cycle", subtype: "success", triggerMessageId: frame.payload.id, evidenceRefs: [], isError: false, terminalReason: null, apiErrorStatus: null});
    }, 180);
  }
}
"#,
    )
    .unwrap();
    let config = ClaudeTeamRuntimeConfig {
        runner_path: runner,
        cwd,
        team_run_id: "team-run-test".into(),
        member_run_id: "member-run-test".into(),
        member_name: "Claude test".into(),
        role_label: "developer".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "bypassPermissions".into(),
        allowed_tools: None,
        disallowed_tools: None,
        setting_sources: vec!["project".into()],
        resume_session_id: None,
        environment: harness_runtime_contract::CollaborationCapabilityEnvironment::empty(),
    };
    let mut transport = ClaudeRunnerTransport::spawn(&config).unwrap();
    let mut accepted = false;
    let outcome = transport
        .run_cycle(
            "run longer than the delivery timeout",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_millis(
                60,
            )),
            &mut |_receipt| {
                accepted = true;
                Ok(())
            },
            &mut |_pending, _result| Ok(()),
            &mut |_event| {},
            &mut CycleControl::default,
        )
        .unwrap();
    assert!(accepted);
    assert_eq!(outcome.final_text, "completed after a long silent tool");
    assert!(outcome.provider_terminal_failure.is_none());
    assert_eq!(transport.native_session_id, "native-long-cycle");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unconsumed_input_times_out_without_claiming_provider_acceptance() {
    let root = unique_temp_dir("claude-unconsumed-input-timeout");
    let bin = root.join("bin");
    let cwd = root.join("workspace");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@anthropic-ai/claude-agent-sdk":"0.3.220"}}"#,
    )
    .unwrap();
    let runner = bin.join("runner.mjs");
    fs::write(
        &runner,
        r#"
import readline from "node:readline";
const input = readline.createInterface({input: process.stdin, crlfDelay: Infinity});
const emit = (event, data) => process.stdout.write(JSON.stringify({event, data}) + "\n");
for await (const line of input) {
  const frame = JSON.parse(line);
  if (frame.command === "start") {
    emit("member_started", {memberRunId: frame.payload.memberRunId, cwd: frame.payload.cwd, permissionMode: frame.payload.permissionMode, ownedPathCount: 0, resumed: false});
  } else if (frame.command === "deliver") {
    emit("delivered", {id: frame.payload.id, kind: frame.payload.kind});
  }
}
"#,
    )
    .unwrap();
    let config = ClaudeTeamRuntimeConfig {
        runner_path: runner,
        cwd,
        team_run_id: "team-run-test".into(),
        member_run_id: "member-run-test".into(),
        member_name: "Claude test".into(),
        role_label: "developer".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "bypassPermissions".into(),
        allowed_tools: None,
        disallowed_tools: None,
        setting_sources: vec!["project".into()],
        resume_session_id: None,
        environment: harness_runtime_contract::CollaborationCapabilityEnvironment::empty(),
    };
    let mut transport = ClaudeRunnerTransport::spawn(&config).unwrap();
    let mut accepted = false;
    let error = transport
        .run_cycle(
            "never accepted",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_millis(
                80,
            )),
            &mut |_receipt| {
                accepted = true;
                Ok(())
            },
            &mut |_pending, _result| Ok(()),
            &mut |_event| {},
            &mut CycleControl::default,
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("CLAUDE_AGENT_SDK_INPUT_ACCEPTANCE_TIMEOUT"));
    assert!(!accepted);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn accepted_cycle_still_fails_closed_when_the_runner_transport_exits() {
    let root = unique_temp_dir("claude-accepted-runner-exit");
    let bin = root.join("bin");
    let cwd = root.join("workspace");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@anthropic-ai/claude-agent-sdk":"0.3.220"}}"#,
    )
    .unwrap();
    let runner = bin.join("runner.mjs");
    fs::write(
        &runner,
        r#"
import readline from "node:readline";
const input = readline.createInterface({input: process.stdin, crlfDelay: Infinity});
const emit = (event, data) => process.stdout.write(JSON.stringify({event, data}) + "\n");
for await (const line of input) {
  const frame = JSON.parse(line);
  if (frame.command === "start") {
    emit("member_started", {memberRunId: frame.payload.memberRunId, cwd: frame.payload.cwd, permissionMode: frame.payload.permissionMode, ownedPathCount: 0, resumed: false});
  } else if (frame.command === "deliver") {
    emit("consumed", {id: frame.payload.id, kind: frame.payload.kind, sessionId: "native-before-exit"});
    emit("session_bound", {sessionId: "native-before-exit", tag: "team-run-test:member-run-test", title: "Claude test · developer", providerVersion: "2.1.220", model: null, effort: null});
    setTimeout(() => process.exit(17), 40);
  }
}
"#,
    )
    .unwrap();
    let config = ClaudeTeamRuntimeConfig {
        runner_path: runner,
        cwd,
        team_run_id: "team-run-test".into(),
        member_run_id: "member-run-test".into(),
        member_name: "Claude test".into(),
        role_label: "developer".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "bypassPermissions".into(),
        allowed_tools: None,
        disallowed_tools: None,
        setting_sources: vec!["project".into()],
        resume_session_id: None,
        environment: harness_runtime_contract::CollaborationCapabilityEnvironment::empty(),
    };
    let mut transport = ClaudeRunnerTransport::spawn(&config).unwrap();
    let mut accepted = false;
    let error = transport
        .run_cycle(
            "accept then lose the runner",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_secs(1)),
            &mut |_receipt| {
                accepted = true;
                Ok(())
            },
            &mut |_pending, _result| Ok(()),
            &mut |_event| {},
            &mut CycleControl::default,
        )
        .unwrap_err();
    assert!(accepted);
    assert!(error
        .to_string()
        .contains("CLAUDE_AGENT_SDK_TRANSPORT_CLOSED"));
    assert!(matches!(transport.state, TransportState::Disconnected));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires authenticated live Claude Agent SDK 0.3.220 / Claude Code 2.1.220"]
fn live_claude_21220_round_interrupt_close_and_same_session_resume() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root");
    let runner = repository.join("apps/claude-member-runner/bin/claude-member-runner.mjs");
    let config = |resume_session_id: Option<String>| ClaudeTeamRuntimeConfig {
        runner_path: runner.clone(),
        cwd: repository.clone(),
        team_run_id: "live-claude-21220-canary".into(),
        member_run_id: "live-claude-21220-member".into(),
        member_name: "Claude 2.1.220 live canary".into(),
        role_label: "runtime canary".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "bypassPermissions".into(),
        allowed_tools: Some(Vec::new()),
        disallowed_tools: None,
        setting_sources: Vec::new(),
        resume_session_id,
        environment: harness_runtime_contract::CollaborationCapabilityEnvironment::empty(),
    };
    let mut no_steer = |_pending: &SteerRequest, _result: &SteerProviderResult| Ok(());
    let mut no_event = |_event: &Value| {};

    let mut first = ClaudeRunnerTransport::spawn(&config(None)).unwrap();
    let first_outcome = first
        .run_cycle(
            "Reply with exactly CLAUDE-LIVE-ROUND-1. Do not use tools.",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_secs(
                120,
            )),
            &mut |_receipt| Ok(()),
            &mut no_steer,
            &mut no_event,
            &mut CycleControl::default,
        )
        .unwrap();
    assert!(
        first_outcome.final_text.contains("CLAUDE-LIVE-ROUND-1"),
        "unexpected first live outcome: {first_outcome:?}"
    );
    assert!(first_outcome.provider_terminal_failure.is_none());
    let native_session_id = first.native_session_id.clone();
    assert!(!native_session_id.is_empty());

    let interrupted = first
        .run_cycle(
            "Count slowly from 1 to 200, one number per line, with a short pause between numbers.",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_secs(
                120,
            )),
            &mut |_receipt| Ok(()),
            &mut no_steer,
            &mut no_event,
            &mut || CycleControl {
                interrupt: true,
                ..CycleControl::default()
            },
        )
        .unwrap();
    assert_eq!(
        interrupted.interrupt,
        Some(harness_runtime_contract::InterruptCause::HostControl)
    );
    assert!(first.last_interrupt_resumed_same_session);
    first.close("live_canary_generation_one_close").unwrap();
    let closed = first
        .wait_for_member_closed(Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        closed.get("sessionId").and_then(Value::as_str),
        Some(native_session_id.as_str())
    );

    let mut resumed = ClaudeRunnerTransport::spawn(&config(Some(native_session_id.clone())))
        .expect("resume exact Claude session");
    let resumed_outcome = resumed
        .run_cycle(
            "Reply with exactly CLAUDE-LIVE-RESUMED. Do not use tools.",
            harness_runtime_contract::CycleTimeouts::with_input_acceptance(Duration::from_secs(
                120,
            )),
            &mut |_receipt| Ok(()),
            &mut no_steer,
            &mut no_event,
            &mut CycleControl::default,
        )
        .unwrap();
    assert!(resumed_outcome.final_text.contains("CLAUDE-LIVE-RESUMED"));
    assert_eq!(resumed.native_session_id, native_session_id);
    resumed.close("live_canary_generation_two_close").unwrap();
    resumed
        .wait_for_member_closed(Duration::from_secs(10))
        .unwrap();
    eprintln!("CLAUDE_LIVE_NATIVE_SESSION_ID={native_session_id}");
}

#[test]
fn close_before_first_cycle_releases_handle_without_inventing_a_native_session() {
    let root = unique_temp_dir("claude-pre-session-close");
    let bin = root.join("bin");
    let cwd = root.join("workspace");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@anthropic-ai/claude-agent-sdk":"0.3.220"}}"#,
    )
    .unwrap();
    let runner = bin.join("runner.mjs");
    fs::write(
        &runner,
        r#"
import readline from "node:readline";
const input = readline.createInterface({input: process.stdin, crlfDelay: Infinity});
const emit = (event, data) => process.stdout.write(JSON.stringify({event, data}) + "\n");
for await (const line of input) {
  const frame = JSON.parse(line);
  if (frame.command === "close") {
    emit("member_closed", {
      sessionId: null,
      reason: frame.payload.reason,
      undelivered: [],
      evidenceRefs: [],
    });
    break;
  }
}
"#,
    )
    .unwrap();
    let config = ClaudeTeamRuntimeConfig {
        runner_path: runner,
        cwd,
        team_run_id: "team-run-test".into(),
        member_run_id: "member-run-test".into(),
        member_name: "Claude test".into(),
        role_label: "developer".into(),
        owned_paths: Vec::new(),
        model: None,
        effort: None,
        permission_mode: "bypassPermissions".into(),
        allowed_tools: None,
        disallowed_tools: None,
        setting_sources: vec!["project".into(), "user".into()],
        resume_session_id: None,
        environment: harness_runtime_contract::CollaborationCapabilityEnvironment::empty(),
    };
    let mut runtime = ClaudeTeamRuntime::spawn(config).unwrap();
    let evidence = runtime.close_owned_runtime("harness_team_close").unwrap();
    assert!(evidence.active_cycle_terminal);
    assert!(evidence.owned_runtime_closed);
    assert!(evidence.native_session_retained);
    assert_eq!(evidence.native_session_id, None);
    fs::remove_dir_all(root).unwrap();
}

fn unique_temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "firm-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ))
}

// ---------------------------------------------------------------------------
// SPEC-TYPED-CYCLE-OUTCOME-01 §5: the S1 assertion family against Claude.

#[cfg(unix)]
fn scripted_claude_transport() -> (ClaudeRunnerTransport, std::sync::mpsc::Sender<String>) {
    use std::sync::mpsc;
    let mut child = std::process::Command::new("sh")
        .args(["-c", "cat >/dev/null"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn scripted runner sink");
    let stdin = child.stdin.take().expect("scripted runner stdin");
    let (line_tx, lines) = mpsc::channel();
    (
        ClaudeRunnerTransport {
            child: ClaudeRunnerChild::new(child).expect("scripted runner child"),
            stdin: Some(stdin),
            lines,
            stdout_reader: None,
            stderr_reader: None,
            native_session_id: "scripted-session".to_string(),
            expected_resume_session_id: None,
            provider_version: None,
            state: TransportState::Idle,
            next_input_id: 1,
            pending_input_count: 0,
            last_cycle_terminal: false,
            last_interrupt_resumed_same_session: false,
            close_reason: None,
        },
        line_tx,
    )
}

fn claude_event(event: &str, data: serde_json::Value) -> String {
    serde_json::json!({"event": event, "data": data}).to_string()
}

fn claude_consumed(input_id: &str) -> String {
    claude_event(
        "consumed",
        serde_json::json!({"id": input_id, "kind": "runtime_cycle", "sessionId": "scripted-session"}),
    )
}

fn claude_assistant_message() -> String {
    claude_event(
        "assistant_message",
        serde_json::json!({"sessionId": "scripted-session", "content": "done"}),
    )
}

fn claude_turn_complete(input_id: &str) -> String {
    claude_event(
        "turn_complete",
        serde_json::json!({
            "sessionId": "scripted-session",
            "subtype": "success",
            "triggerMessageId": input_id,
            "evidenceRefs": [],
            "isError": false,
            "terminalReason": null,
            "apiErrorStatus": null
        }),
    )
}

fn claude_conformance_timeouts() -> harness_runtime_contract::CycleTimeouts {
    harness_runtime_contract::CycleTimeouts {
        input_acceptance: Duration::from_millis(1),
        transport_liveness: Duration::from_millis(1),
        control_settle: Duration::ZERO,
    }
}

fn drive_claude_cycle(
    events: Vec<String>,
    disconnect: bool,
    timeouts: &harness_runtime_contract::CycleTimeouts,
    mut control: impl FnMut() -> harness_runtime_contract::CycleControl,
) -> Result<harness_runtime_contract::ExecutionCycleOutcome, String> {
    let (mut transport, line_tx) = scripted_claude_transport();
    for event in events {
        line_tx.send(event).map_err(|error| error.to_string())?;
    }
    if disconnect {
        drop(line_tx);
    }
    transport
        .run_cycle(
            "conformance cycle",
            *timeouts,
            &mut |_receipt| Ok(()),
            &mut |_pending, _result| Ok(()),
            &mut |_event| {},
            &mut control,
        )
        .map_err(|error| error.to_string())
}

struct ClaudeCycleConformanceFixture;

impl harness_runtime_contract::CycleConformanceFixture for ClaudeCycleConformanceFixture {
    type Error = String;

    fn run_receipt_then_silence(
        &mut self,
        timeouts: &harness_runtime_contract::CycleTimeouts,
    ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
        let outcome = drive_claude_cycle(
            vec![
                claude_consumed("claude-cycle-2"),
                claude_assistant_message(),
                claude_turn_complete("claude-cycle-2"),
            ],
            false,
            timeouts,
            harness_runtime_contract::CycleControl::default,
        )?;
        Ok(harness_runtime_contract::CycleConformanceOutcome {
            interrupt: outcome.interrupt.clone(),
            control_unproven: false,
            result: harness_runtime_contract::CycleConformanceResult::Outcome(Box::new(outcome)),
        })
    }

    fn run_no_receipt(
        &mut self,
        timeouts: &harness_runtime_contract::CycleTimeouts,
    ) -> Result<harness_runtime_contract::CycleConformanceOutcome, Self::Error> {
        let error = match drive_claude_cycle(
            Vec::new(),
            false,
            timeouts,
            harness_runtime_contract::CycleControl::default,
        ) {
            Ok(_) => return Err("a never-accepted cycle produced an outcome".to_string()),
            Err(error) => error,
        };
        assert!(error.contains("INPUT_ACCEPTANCE_TIMEOUT"), "{error}");
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
        let error = match drive_claude_cycle(
            vec![claude_consumed("claude-cycle-2")],
            true,
            timeouts,
            harness_runtime_contract::CycleControl::default,
        ) {
            Ok(_) => return Err("a dead transport produced an outcome".to_string()),
            Err(error) => error,
        };
        assert!(error.contains("TRANSPORT_CLOSED"), "{error}");
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
        let error = match drive_claude_cycle(
            vec![claude_consumed("claude-cycle-2")],
            false,
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
            Ok(_) => return Err("an unacknowledged interrupt produced an outcome".to_string()),
            Err(error) => error,
        };
        assert!(error.contains("CONTROL_SETTLE_TIMEOUT"), "{error}");
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
        let mut first = true;
        let outcome = drive_claude_cycle(
            vec![
                claude_consumed("claude-cycle-2"),
                claude_event(
                    "interrupted",
                    serde_json::json!({"stillQueued": [], "abandonedTriggerMessageIds": []}),
                ),
                claude_event(
                    "member_resumed_after_interrupt",
                    serde_json::json!({"sessionId": "scripted-session"}),
                ),
            ],
            false,
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
        )?;
        Ok(harness_runtime_contract::CycleConformanceOutcome {
            interrupt: outcome.interrupt.clone(),
            control_unproven: false,
            result: harness_runtime_contract::CycleConformanceResult::Outcome(Box::new(outcome)),
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

#[cfg(unix)]
#[test]
fn claude_passes_the_s1_cycle_conformance_family() {
    let timeouts = claude_conformance_timeouts();
    let mut fixture = ClaudeCycleConformanceFixture;
    harness_runtime_contract::assert_a1_accepted_input_survives_silence(&mut fixture, &timeouts)
        .expect("A1");
    harness_runtime_contract::assert_a2_delivery_timeout_fails_closed(&mut fixture, &timeouts)
        .expect("A2");
    harness_runtime_contract::assert_a3_transport_death_fails_closed(&mut fixture, &timeouts)
        .expect("A3");
    harness_runtime_contract::assert_a5_control_settle_only_bounds_control(&mut fixture, &timeouts)
        .expect("A5");
    harness_runtime_contract::assert_b1_host_interrupt_attribution(&mut fixture, &timeouts)
        .expect("B1");
}
