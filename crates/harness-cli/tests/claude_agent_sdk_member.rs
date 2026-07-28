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

mod harness_env;

use harness_env::{current_project_id, run_harness, TempHome};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Write a fake runner speaking the NDJSON protocol in
/// `apps/claude-member-runner/src/protocol.mjs`.
///
/// `follow_up_after_first_turn` makes it send one TeamMessage back into the
/// ledger *after* reporting turn 1, which is the case the mode exists for.
fn write_fake_runner(dir: &Path, follow_up_after_first_turn: bool) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("fake-runner.mjs");
    let follow_up = if follow_up_after_first_turn {
        "true"
    } else {
        "false"
    };
    let script = format!(
        r#"
import {{ spawnSync }} from "node:child_process";
import {{ createInterface }} from "node:readline";

const FOLLOW_UP = {follow_up};
let cfg = null;
let turns = 0;
let sentFollowUp = false;

const emit = (event, data) => process.stdout.write(JSON.stringify({{ event, data }}) + "\n");

const rl = createInterface({{ input: process.stdin }});
for await (const line of rl) {{
  if (!line.trim()) continue;
  const {{ command, payload }} = JSON.parse(line);

  if (command === "start") {{
    cfg = payload;
    emit("member_started", {{ memberRunId: cfg.memberRunId }});
    emit("session_bound", {{
      sessionId: "fake-native-session-0001",
      providerVersion: "2.1.220-test",
    }});
  }} else if (command === "deliver") {{
    turns += 1;
    emit("delivered", {{ id: payload.id, kind: payload.kind }});
    emit("assistant_message", {{
      content: [{{ type: "text", text: `## RESULT\ndone\n\n## SUMMARY\nturn-${{turns}}` }}],
    }});
    if (FOLLOW_UP) {{
      const handoff = spawnSync(process.env.HARNESS_BIN, [
        "team-run", "send",
        "--id", cfg.teamRunId,
        "--from", cfg.memberRunId,
        "--to", "host",
        "--kind", "handoff",
        "--body", `## RESULT\ndone\n\n## SUMMARY\nturn-${{turns}} explicit handoff`,
        "--correlation-id", payload.correlation_id,
        "--causation-id", payload.id,
        "--json",
      ], {{ encoding: "utf8" }});
      if (handoff.status !== 0) throw new Error(handoff.stderr);
      JSON.parse(handoff.stdout);
    }}
    emit("turn_complete", {{
      subtype: "success",
      triggerMessageId: payload.id,
      evidenceRefs: turns === 1 ? ["src/member.ts"] : [],
    }});

    // Strictly after turn 1 is reported, so the Host has already seen an empty
    // queue by the time this lands.
    if (FOLLOW_UP && turns === 1 && !sentFollowUp) {{
      sentFollowUp = true;
      const sent = spawnSync(process.env.HARNESS_BIN, [
        "team-run", "send",
        "--id", cfg.teamRunId,
        "--from", "host",
        "--to", cfg.memberRunId,
        "--kind", "message",
        "--body", "late follow-up",
        "--correlation-id", payload.correlation_id,
        "--causation-id", payload.id,
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
            "SdkMember:Runtime owner:claude/agent-sdk",
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
    let runner = write_fake_runner(&home.base().join("runner"), false);

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
            .join("projects")
            .join(project_id)
            .join("member_runs.jsonl")
            .is_file(),
        "MemberRun and its native-session binding remain in the execution/project store"
    );
}

#[test]
fn agent_sdk_member_consumes_a_message_that_arrives_after_the_queue_emptied() {
    let home = TempHome::new("agent-sdk-late-message");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner(&home.base().join("runner"), true);

    let run_id = create_run(&home, &root);
    // Grace wide enough that the fake's post-turn send lands inside it.
    let out = start_with_fake_runner(&home, &root, &runner, "8000", &run_id);
    assert!(out.status.success(), "start failed: {out:?}");

    let inbox = run_harness(
        &home,
        &root,
        &[
            "team-run",
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--all",
            "--json",
        ],
    );
    let body = String::from_utf8_lossy(&inbox.stdout);
    let handoffs = body.matches("\"handoff\"").count();

    // Two rounds means the member was still alive when the late message
    // arrived. Under `claude_cli` this is 1: the member is gone by then.
    assert_eq!(
        handoffs, 2,
        "expected the member to survive the empty queue and take a second \
         round without duplicate Adapter handoffs, got {handoffs}.\ninbox: {body}"
    );
    assert!(
        body.contains("turn-2"),
        "second round should report turn-2.\ninbox: {body}"
    );
    assert!(
        body.contains("src/member.ts"),
        "runner evidence refs must survive into the durable handoff.\ninbox: {body}"
    );
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
    let assignment = member_inbox
        .iter()
        .find(|message| message["kind"] == "assignment")
        .expect("assignment");
    let follow_up = member_inbox
        .iter()
        .find(|message| message["body"] == "late follow-up")
        .expect("follow-up");
    let first_handoff = member_outbox
        .iter()
        .find(|message| {
            message["kind"] == "handoff"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains("turn-1"))
        })
        .expect("first handoff");
    let second_handoff = member_outbox
        .iter()
        .find(|message| {
            message["kind"] == "handoff"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains("turn-2"))
        })
        .expect("second handoff");
    assert_eq!(
        first_handoff["causation_id"], assignment["id"],
        "round one is caused by the Assignment"
    );
    assert_eq!(
        second_handoff["causation_id"], follow_up["id"],
        "round two is caused by the exact follow-up TeamMessage"
    );
    assert_eq!(
        second_handoff["correlation_id"], assignment["correlation_id"],
        "all rounds remain in the Assignment correlation"
    );
    let status_body = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_body.contains("\"status\": \"idle\""),
        "turn completion must leave the persistent Member idle: {status_body}"
    );
    assert!(
        status_body.contains("\"status\": \"running\""),
        "handoff must not decide TeamRun completion: {status_body}"
    );
}

#[test]
fn agent_sdk_member_binds_one_native_session_and_turn_completion_is_idle() {
    let home = TempHome::new("agent-sdk-session-bind");
    init_project(&home, "proj");
    let root = home.base().join("proj");
    let runner = write_fake_runner(&home.base().join("runner"), false);

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
        body.contains("2.1.220-test"),
        "the profile and native session must use the SDK-reported execution-mode version.\n{body}"
    );
    assert!(
        body.contains("\"status\": \"idle\""),
        "provider turn completion must not terminalize the MemberRun.\n{body}"
    );
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
