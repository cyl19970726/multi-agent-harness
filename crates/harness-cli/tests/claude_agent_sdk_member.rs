//! Deterministic coverage for the `claude_agent_sdk` execution mode.
//!
//! The point under test is ADR 0037 §Acceptance item 6 — "the Host revises or
//! advances while another member continues on the same `MemberRun` and native
//! session" — which has no coverage anywhere else in the repo.
//!
//! `claude_cli` cannot satisfy it by construction: its loop ends the member the
//! instant `queued_messages_for` returns empty, so a TeamMessage that arrives a
//! moment later has no recipient. Reproducing "arrives *after* the queue was
//! already empty" as a wall-clock race would be flaky, so the fake runner does
//! it itself: it emits `turn_complete`, and only then shells out to
//! `team-run send`. The ordering is therefore guaranteed by the fake, not by
//! timing luck. A real provider is never invoked.

use std::path::Path;
use std::process::Command;

mod harness_env;

use harness_env::{
    clear_inherited_native_harness_env, current_project_id, run_harness, TempHome,
    INHERITED_NATIVE_HARNESS_ENV,
};

#[test]
fn shared_test_commands_clear_every_native_harness_selector() {
    let mut command = Command::new("harness");
    for key in INHERITED_NATIVE_HARNESS_ENV {
        command.env(key, "ambient-member-value");
    }

    clear_inherited_native_harness_env(&mut command);
    let configured = command
        .get_envs()
        .map(|(key, value)| (key.to_string_lossy().into_owned(), value.is_none()))
        .collect::<std::collections::HashMap<_, _>>();

    for key in INHERITED_NATIVE_HARNESS_ENV {
        assert_eq!(
            configured.get(*key),
            Some(&true),
            "{key} must be removed from spawned test processes"
        );
    }

    let home = TempHome::new("native-selector-defaults");
    let defaults = home
        .envs()
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    for key in INHERITED_NATIVE_HARNESS_ENV
        .iter()
        .filter(|key| **key != "HARNESS_ROOT")
    {
        assert_eq!(
            defaults.get(*key).map(String::as_str),
            Some(""),
            "{key} must be neutralized for direct test commands"
        );
    }
}

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Which turn shape the fake runner reproduces.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FakeTurnShape {
    /// A normal round with a structured member report.
    Report,
    /// A provider API failure the runner CAN classify: `isError: true` with a
    /// terminal reason and HTTP status.
    ClassifiedApiError,
    /// A terminal provider failure the runner CANNOT classify: the turn simply
    /// ends with no agent message at all.
    SilentTurn,
}

/// Write a fake runner speaking the NDJSON protocol in
/// `apps/claude-member-runner/src/protocol.mjs`.
///
/// `follow_up_after_first_turn` makes it send one TeamMessage back into the
/// ledger *after* reporting turn 1, which is the case the mode exists for.
/// `shape` selects the turn outcome: a normal report, the classified SDK error
/// result (issue #293 — subtype stays "success" while `isError` carries the
/// truth), or a silent turn with no agent message at all.
fn write_fake_runner(
    dir: &Path,
    follow_up_after_first_turn: bool,
    shape: FakeTurnShape,
) -> std::path::PathBuf {
    write_fake_runner_with_version(dir, follow_up_after_first_turn, shape, "2.1.220")
}

fn write_fake_runner_with_version(
    dir: &Path,
    follow_up_after_first_turn: bool,
    shape: FakeTurnShape,
    provider_version: &str,
) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let sdk_package = dir.join("node_modules/@anthropic-ai/claude-agent-sdk/package.json");
    std::fs::create_dir_all(sdk_package.parent().expect("SDK package parent")).unwrap();
    std::fs::write(
        &sdk_package,
        serde_json::json!({
            "name": "@anthropic-ai/claude-agent-sdk",
            "version": "0.2.70",
            "claudeCodeVersion": provider_version,
        })
        .to_string(),
    )
    .unwrap();
    let path = dir.join("fake-runner.mjs");
    let follow_up = if follow_up_after_first_turn {
        "true"
    } else {
        "false"
    };
    let api_error = if shape == FakeTurnShape::ClassifiedApiError {
        "true"
    } else {
        "false"
    };
    let silent_turn = if shape == FakeTurnShape::SilentTurn {
        "true"
    } else {
        "false"
    };
    let script = format!(
        r#"
import {{ spawnSync }} from "node:child_process";
import {{ createInterface }} from "node:readline";

const FOLLOW_UP = {follow_up};
const API_ERROR = {api_error};
const SILENT_TURN = {silent_turn};
let cfg = null;
let turns = 0;
let sentFollowUp = false;

const emit = (event, data) => process.stdout.write(JSON.stringify({{ event, data }}) + "\n");
const harness = (args) => {{
  const result = spawnSync(process.env.HARNESS_BIN, args, {{ encoding: "utf8" }});
  if (result.status !== 0) throw new Error(result.stderr);
  return result.stdout;
}};

const rl = createInterface({{ input: process.stdin }});
for await (const line of rl) {{
  if (!line.trim()) continue;
  const {{ command, payload }} = JSON.parse(line);

  if (command === "start") {{
    cfg = payload;
    emit("member_started", {{ memberRunId: cfg.memberRunId }});
    emit("session_bound", {{
      sessionId: "fake-native-session-0001",
      providerVersion: "{provider_version}",
    }});
  }} else if (command === "deliver") {{
    turns += 1;
    emit("delivered", {{ id: payload.id, kind: payload.kind }});
    emit("consumed", {{
      id: payload.id,
      sessionId: "fake-native-session-0001",
    }});
    if (API_ERROR) {{
      emit("assistant_message", {{
        content: [{{ type: "text", text: "Failed to authenticate. API Error: 403 Request not allowed" }}],
      }});
      emit("turn_complete", {{
        subtype: "success",
        isError: true,
        terminalReason: "api_error",
        apiErrorStatus: 403,
        triggerMessageId: payload.id,
        evidenceRefs: [],
      }});
      continue;
    }}
    if (SILENT_TURN) {{
      // No assistant_message at all: the provider ended the turn without an
      // agent message and the runner has nothing to classify.
      emit("turn_complete", {{
        subtype: "success",
        isError: false,
        terminalReason: null,
        apiErrorStatus: null,
        triggerMessageId: payload.id,
        evidenceRefs: [],
      }});
      continue;
    }}
    emit("assistant_message", {{
      content: [{{ type: "text", text: `## RESULT\ndone\n\n## SUMMARY\nturn-${{turns}}` }}],
    }});
    if (payload.kind === "work") {{
      harness([
        "team-run", "work", "start",
        "--team-run-id", cfg.teamRunId,
        "--work-id", payload.correlation_id,
        "--member-run-id", cfg.memberRunId,
        "--expected-version", "1",
      ]);
      harness([
        "team-run", "work", "submit",
        "--team-run-id", cfg.teamRunId,
        "--work-id", payload.correlation_id,
        "--member-run-id", cfg.memberRunId,
        "--expected-version", "2",
        "--result", `turn-${{turns}} completed`,
        "--artifact-ref", "src/member.ts",
      ]);
    }}
    emit("turn_complete", {{
      subtype: "success",
      isError: false,
      terminalReason: null,
      apiErrorStatus: null,
      triggerMessageId: payload.id,
      evidenceRefs: turns === 1 ? ["src/member.ts"] : [],
    }});

    // Strictly after turn 1 is reported, so the Host has already seen an empty
    // queue by the time this lands.
    if (FOLLOW_UP && turns === 1 && !sentFollowUp) {{
      sentFollowUp = true;
      await new Promise((resolve) => setTimeout(resolve, 250));
      const sent = spawnSync(process.env.HARNESS_BIN, [
        "team-run", "send",
        "--id", cfg.teamRunId,
        "--from", "host",
        "--to", cfg.memberRunId,
        "--kind", "message",
        "--response-required",
        "--body", "late follow-up",
        "--work-id", payload.correlation_id,
        "--json",
      ], {{ encoding: "utf8" }});
      if (sent.status !== 0) throw new Error(sent.stderr);
      JSON.parse(sent.stdout);
    }}
  }} else if (command === "close") {{
    emit("member_closed", {{ reason: payload?.reason ?? "closed", undelivered: [] }});
    rl.close();
  }}
}}
"#
    );
    std::fs::write(&path, script).unwrap();
    path
}

fn start_with_fake_runner(
    home: &TempHome,
    root: &Path,
    runner: &Path,
    grace_ms: &str,
    run_id: &str,
) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(["team-run", "start", "--id", run_id])
        .current_dir(root)
        .envs(home.envs())
        .env("HARNESS_CLAUDE_MEMBER_RUNNER", runner)
        .env("HARNESS_CLAUDE_AGENT_SDK_IDLE_GRACE_MS", grace_ms)
        .env("HARNESS_BIN", env!("CARGO_BIN_EXE_harness"))
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_SPACE")
        .env_remove("HARNESS_COMPANY")
        .output()
        .expect("team-run start")
}

fn create_run(home: &TempHome, root: &Path) -> String {
    let out = run_harness(
        home,
        root,
        &[
            "team-run",
            "create",
            "--objective",
            "deterministic agent-sdk coverage",
            "--member",
            "SdkMember:Runtime owner:claude/agent-sdk#Complete deterministic Agent SDK Work",
        ],
    );
    assert!(out.status.success(), "create failed: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn current_company_does_not_capture_claude_member_session_or_desktop_target() {
    let home = TempHome::new("agent-sdk-company-store-boundary");
    let project_id = init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner(&home.base().join("runner"), false, FakeTurnShape::Report);

    let company = run_harness(
        &home,
        &root,
        &[
            "company",
            "init",
            "--id",
            "agent-company",
            "--name",
            "Agent Company",
        ],
    );
    assert!(company.status.success(), "company init failed: {company:?}");

    let run_id = create_run(&home, &root);
    let started = start_with_fake_runner(&home, &root, &runner, "200", &run_id);
    assert!(started.status.success(), "start failed: {started:?}");

    let status = run_harness(
        &home,
        &root,
        &["team-run", "status", "--id", &run_id, "--json"],
    );
    assert!(status.status.success(), "status failed: {status:?}");
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("status JSON");
    let member_id = status_json["members"][0]["member_run"]["id"]
        .as_str()
        .expect("member id");

    let target = run_harness(
        &home,
        &root,
        &[
            "member-run",
            "open-native",
            "--id",
            member_id,
            "--print-only",
            "--json",
        ],
    );
    assert!(target.status.success(), "open-native failed: {target:?}");
    let target_json: serde_json::Value =
        serde_json::from_slice(&target.stdout).expect("open-native JSON");
    assert_eq!(target_json["provider"], "claude");
    assert_eq!(target_json["execution_mode"], "claude_agent_sdk");
    assert_eq!(
        target_json["uri"],
        "claude://resume?session=fake-native-session-0001"
    );
    assert_eq!(target_json["opened"], false);

    let company_store = home.harness_home().join("companies").join("agent-company");
    assert!(
        !company_store.join("member_runs.jsonl").exists(),
        "Company Store must not capture execution MemberRuns or native-session bindings"
    );
    assert!(
        home.harness_home()
            .join("execution-spaces")
            .join(project_id)
            .join("member_runs.jsonl")
            .is_file(),
        "MemberRun and its native-session binding remain in the Execution Space"
    );
}

// TODO: This test requires live Claude SDK credentials; CI runners lack them.
// Annotated #[ignore] until the CI environment provides the SDK key (tracked in #XXX).
#[test]
#[ignore = "needs-claude-sdk-credentials"]
fn agent_sdk_member_consumes_a_message_that_arrives_after_the_queue_emptied() {
    let home = TempHome::new("agent-sdk-late-message");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner(&home.base().join("runner"), true, FakeTurnShape::Report);

    let run_id = create_run(&home, &root);
    // Grace wide enough that the fake's post-turn send lands inside it.
    let out = start_with_fake_runner(&home, &root, &runner, "8000", &run_id);
    assert!(out.status.success(), "start failed: {out:?}");

    let status = run_harness(
        &home,
        &root,
        &["team-run", "status", "--id", &run_id, "--json"],
    );
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("status JSON");
    let member_id = status_json["members"][0]["member_run"]["id"]
        .as_str()
        .expect("member id");
    let inbox = run_harness(
        &home,
        &root,
        &[
            "team-run",
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            member_id,
            "--all",
            "--json",
        ],
    );
    let body = String::from_utf8_lossy(&inbox.stdout);
    assert!(
        body.contains("late follow-up"),
        "the post-idle Host message must stay reconstructable in the recipient inbox.\ninbox: {body}"
    );
    let detail = run_harness(
        &home,
        &root,
        &["member-run", "show", "--id", member_id, "--json"],
    );
    let detail_json: serde_json::Value =
        serde_json::from_slice(&detail.stdout).expect("member detail JSON");
    let member_inbox = detail_json["mailbox"]["inbox"]
        .as_array()
        .expect("member inbox");
    let member_outbox = detail_json["mailbox"]["outbox"]
        .as_array()
        .expect("member outbox");
    let follow_up = member_inbox
        .iter()
        .find(|message| message["body"] == "late follow-up")
        .expect("follow-up");
    assert!(
        member_outbox.is_empty(),
        "the adapter must not fabricate outbox messages"
    );
    let works = detail_json["works"].as_array().expect("member Works");
    assert_eq!(
        works.len(),
        1,
        "one durable Work owns the first round: {works:?}"
    );
    assert_eq!(works[0]["status"], "review");
    assert_eq!(
        follow_up["work_id"], works[0]["id"],
        "the later conversation links to Work without replacing ownership"
    );
    let completed_rounds = detail_json["actions"]
        .as_array()
        .expect("member actions")
        .iter()
        .filter(|action| action["action_type"] == "turn_completed")
        .count();
    assert_eq!(
        completed_rounds, 2,
        "the persistent member must execute both turns"
    );
    // Issue #232: turn_evidence_refs must land on the recorded action.
    let turn_actions: Vec<_> = detail_json["actions"]
        .as_array()
        .expect("member actions")
        .iter()
        .filter(|action| action["action_type"] == "turn_completed")
        .collect();
    assert_eq!(turn_actions.len(), 2);
    let turn1_refs = turn_actions[0]["evidence_refs"]
        .as_array()
        .expect("evidence_refs for turn 1");
    assert_eq!(
        turn1_refs
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>(),
        vec!["src/member.ts"],
        "turn 1 must record the provider-emitted evidence refs"
    );
    let turn2_refs = turn_actions[1]["evidence_refs"]
        .as_array()
        .expect("evidence_refs for turn 2");
    assert!(
        turn2_refs.is_empty(),
        "turn 2 must record empty evidence refs from provider"
    );
    let status_body = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_body.contains("\"status\": \"idle\""),
        "turn completion must leave the persistent Member idle: {status_body}"
    );
    assert!(
        status_body.contains("\"status\": \"running\""),
        "member activity must not decide TeamRun completion: {status_body}"
    );
}

#[test]
fn agent_sdk_member_records_provider_errors_instead_of_successful_rounds() {
    // Issue #293: a provider API failure (e.g. 403 from a blocked egress)
    // arrives with subtype "success". The ledger must show a failed
    // provider_error action, not an ordinary completed outcome or Work submit.
    let home = TempHome::new("agent-sdk-provider-error");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner(
        &home.base().join("runner"),
        false,
        FakeTurnShape::ClassifiedApiError,
    );

    let run_id = create_run(&home, &root);
    let out = start_with_fake_runner(&home, &root, &runner, "500", &run_id);
    assert!(out.status.success(), "start failed: {out:?}");

    let status = run_harness(
        &home,
        &root,
        &["team-run", "status", "--id", &run_id, "--json"],
    );
    assert!(status.status.success(), "status failed: {status:?}");
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("status JSON");
    let member_id = status_json["members"][0]["member_run"]["id"]
        .as_str()
        .expect("member id");
    let detail = run_harness(
        &home,
        &root,
        &["member-run", "show", "--id", member_id, "--json"],
    );
    assert!(detail.status.success(), "show failed: {detail:?}");
    let detail_json: serde_json::Value =
        serde_json::from_slice(&detail.stdout).expect("member detail JSON");

    let actions = detail_json["actions"].as_array().expect("actions");
    let provider_errors = actions
        .iter()
        .filter(|action| action["action_type"] == "provider_error")
        .count();
    assert_eq!(
        provider_errors, 1,
        "the provider-failure round must be recorded as provider_error: {detail_json}"
    );
    let provider_error = actions
        .iter()
        .find(|action| action["action_type"] == "provider_error")
        .expect("provider_error action");
    assert_eq!(provider_error["status"], "failed");
    let detail_text = provider_error["summary"].as_str().unwrap_or("");
    assert!(
        detail_text.contains("api_error") && detail_text.contains("403"),
        "the action must name the provider failure and its HTTP status: {detail_text}"
    );
    assert!(
        actions
            .iter()
            .all(|action| action["action_type"] != "completed"),
        "a provider-down round must not be recorded as completed: {detail_json}"
    );

    let outbox = detail_json["mailbox"]["outbox"].as_array().expect("outbox");
    assert!(
        outbox.iter().all(|message| message["kind"] != "handoff"),
        "a provider error is not a member-authored completion message: {detail_json}"
    );

    assert_eq!(
        detail_json["member_run"]["status"], "idle",
        "the persistent member survives a provider error and stays available"
    );
}

// TODO: This test requires live Claude SDK credentials; CI runners lack them.
// Annotated #[ignore] until the CI environment provides the SDK key (tracked in #XXX).
#[test]
#[ignore = "needs-claude-sdk-credentials"]
fn a_silent_provider_turn_is_a_provider_error_and_stays_reconstructable() {
    // The unclassified half of the same defect: a terminal provider failure the
    // runner cannot label ends the turn with NO agent message. `## RESULT`
    // parsing reads empty text as `done`, so without a guard this published a
    // fabricated completion action no member ever wrote.
    let home = TempHome::new("agent-sdk-silent-turn");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner(
        &home.base().join("runner"),
        false,
        FakeTurnShape::SilentTurn,
    );

    let run_id = create_run(&home, &root);
    let out = start_with_fake_runner(&home, &root, &runner, "500", &run_id);
    assert!(out.status.success(), "start failed: {out:?}");

    let status = run_harness(
        &home,
        &root,
        &["team-run", "status", "--id", &run_id, "--json"],
    );
    assert!(status.status.success(), "status failed: {status:?}");
    let status_json: serde_json::Value =
        serde_json::from_slice(&status.stdout).expect("status JSON");
    let member_id = status_json["members"][0]["member_run"]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let detail = run_harness(
        &home,
        &root,
        &["member-run", "show", "--id", &member_id, "--json"],
    );
    assert!(detail.status.success(), "show failed: {detail:?}");
    let detail_json: serde_json::Value =
        serde_json::from_slice(&detail.stdout).expect("member detail JSON");

    // 1. No semantic completion, fabricated message, or Work submit.
    let actions = detail_json["actions"].as_array().expect("actions");
    assert!(
        actions
            .iter()
            .all(|action| action["action_type"] != "completed"),
        "a silent provider turn must not be recorded as completed: {detail_json}"
    );
    let provider_error = actions
        .iter()
        .find(|action| action["action_type"] == "provider_error")
        .unwrap_or_else(|| panic!("no provider_error action: {detail_json}"));
    assert_eq!(provider_error["status"], "failed");
    assert!(
        provider_error["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("empty_final_report"),
        "the record names the silence honestly: {provider_error}"
    );
    let outbox = detail_json["mailbox"]["outbox"].as_array().expect("outbox");
    assert!(
        outbox.iter().all(|message| message["kind"] != "handoff"),
        "silence is not a member-authored completion message: {detail_json}"
    );

    // 2. Everything needed to resume instead of re-create is still on record.
    let member_run = &detail_json["member_run"];
    assert_eq!(member_run["status"], "idle");
    assert_eq!(member_run["id"], serde_json::json!(member_id));
    assert_eq!(member_run["team_run_id"], serde_json::json!(run_id));
    assert_eq!(
        member_run["native_session"]["native_session_id"],
        serde_json::json!("fake-native-session-0001"),
        "the resumable provider session must survive a terminal provider error"
    );
    assert_eq!(
        member_run["native_session"]["supports_resume"],
        serde_json::json!(true)
    );
    assert!(
        member_run["workspace_snapshot"]["cwd"].is_string(),
        "the Workspace must remain reconstructable: {member_run}"
    );
    // The durable Work is still joinable from the member and remains open for
    // a later retry on this same native session.
    let works = detail_json["works"].as_array().expect("member Works");
    assert_eq!(
        works.len(),
        1,
        "one Work must remain assigned: {detail_json}"
    );
    let work = &works[0];
    assert_eq!(work["status"], serde_json::json!("open"));
    assert!(
        work["id"].as_str().is_some_and(|id| !id.is_empty()),
        "the Work identity must remain reconstructable: {work}"
    );
}

#[test]
fn agent_sdk_member_binds_one_native_session_and_turn_completion_is_idle() {
    let home = TempHome::new("agent-sdk-session-bind");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner(&home.base().join("runner"), false, FakeTurnShape::Report);

    let run_id = create_run(&home, &root);
    let out = start_with_fake_runner(&home, &root, &runner, "500", &run_id);
    assert!(out.status.success(), "start failed: {out:?}");

    let status = run_harness(
        &home,
        &root,
        &["team-run", "status", "--id", &run_id, "--json"],
    );
    let body = String::from_utf8_lossy(&status.stdout);
    assert!(
        body.contains("fake-native-session-0001"),
        "native session should be bound from the runner's session_bound event.\n{body}"
    );
    assert!(
        body.contains("claude_agent_sdk"),
        "member profile should record the agent-sdk execution mode.\n{body}"
    );
    assert!(
        body.contains("2.1.220"),
        "the profile and native session must use the SDK-reported execution-mode version.\n{body}"
    );
    assert!(
        body.contains("\"status\": \"idle\""),
        "provider turn completion must not terminalize the MemberRun.\n{body}"
    );
}

#[test]
fn review_required_agent_sdk_package_is_refused_before_fake_runner_execution() {
    let home = TempHome::new("agent-sdk-review-required");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner_with_version(
        &home.base().join("runner"),
        false,
        FakeTurnShape::Report,
        "2.1.221",
    );

    let run_id = create_run(&home, &root);
    let out = start_with_fake_runner(&home, &root, &runner, "500", &run_id);
    assert!(!out.status.success(), "unreviewed SDK unexpectedly started");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("PROVIDER_COMPATIBILITY_BLOCKED") && stderr.contains("2.1.221"),
        "stderr: {stderr}"
    );

    let status = run_harness(
        &home,
        &root,
        &["team-run", "status", "--id", &run_id, "--json"],
    );
    assert!(status.status.success(), "status failed: {status:?}");
    let body = String::from_utf8_lossy(&status.stdout);
    assert!(!body.contains("fake-native-session-0001"), "{body}");
    assert!(body.contains("\"status\": \"planning\""), "{body}");
}

#[test]
fn a_bare_claude_member_defaults_to_the_agent_sdk_mode() {
    // The default is the point: `claude_cli` ends a member on an empty queue,
    // so defaulting to it means defaulting to a mode that cannot satisfy
    // ADR 0037 acceptance item 6. Naming no mode must land on agent-sdk.
    let home = TempHome::new("agent-sdk-default");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let out = run_harness(
        &home,
        &root,
        &[
            "team-run",
            "create",
            "--objective",
            "default mode coverage",
            "--member",
            "Bare:Role:claude",
            "--json",
        ],
    );
    assert!(out.status.success(), "create failed: {out:?}");
    let body = String::from_utf8_lossy(&out.stdout);
    assert!(
        body.contains("claude_agent_sdk"),
        "a member declared as plain `claude` should get the agent-sdk \
         profile.\n{body}"
    );
    assert!(
        !body.contains("claude-cli-native-v1"),
        "plain `claude` must not fall back to the one-shot adapter.\n{body}"
    );
}

#[test]
fn unknown_claude_execution_mode_is_rejected() {
    let home = TempHome::new("agent-sdk-reject-mode");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let out = run_harness(
        &home,
        &root,
        &[
            "team-run",
            "create",
            "--objective",
            "reject unknown modes",
            "--member",
            "Ghost:Role:claude/not-a-mode",
        ],
    );
    assert!(
        !out.status.success(),
        "an unregistered execution mode must fail explicitly rather than \
         silently falling back to claude_cli"
    );
}

#[test]
fn claude_cli_is_rejected_for_agent_team_members() {
    let home = TempHome::new("agent-sdk-reject-cli");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let out = run_harness(
        &home,
        &root,
        &[
            "team-run",
            "create",
            "--objective",
            "reject one-shot Team mode",
            "--member",
            "Legacy:Role:claude/cli",
        ],
    );
    assert!(!out.status.success(), "claude/cli must be workflow-only");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(
            "claude_cli is workflow-only; Agent Team Claude members use claude_agent_sdk"
        ),
        "rejection should explain the supported boundary: {out:?}"
    );
}
