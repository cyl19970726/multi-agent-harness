//! Integration coverage for the Agent Team v0 surface (team-run task):
//!   - `harness team-run create|list|status|inbox|ack|send|events` CLI smoke against an
//!     isolated HOME (temp store, real binary),
//!   - `POST /v1/team-runs` creates the run + member runs + optional initial
//!     Works + folded events, and the response snapshot carries the native
//!     ledger projections,
//!   - `POST /v1/team-runs/{id}/messages` routes a message (400 on unknown
//!     run), `POST /v1/team-runs/{id}/start` accepts asynchronous execution,
//!   - `GET /team-console` serves the console page as text/html,
//!   - SSE `/v1/events` streams `team_run_event` frames for appended rows.

use std::time::Duration;

use harness_store::HarnessStore;

mod fake_provider;
mod harness_env;
use harness_env::{
    collect_sse_data, current_project_id, run_harness, run_harness_with_env, ServeHandle, TempHome,
};

const NATIVE_SELECTOR_CLEAN_ENV: &[(&str, &str)] = &[
    ("HARNESS_ROOT", ""),
    ("HARNESS_PROJECT", ""),
    ("HARNESS_PROJECT_ID", ""),
    ("HARNESS_SPACE", ""),
    ("HARNESS_COMPANY", ""),
    ("HARNESS_MISSION_ID", ""),
    ("HARNESS_ORIGIN_WAVE_ID", ""),
    ("HARNESS_TEAM_RUN_ID", ""),
    ("HARNESS_MEMBER_RUN_ID", ""),
    ("HARNESS_WORK_ID", ""),
    ("HARNESS_WORK_VERSION", ""),
];

fn wait_for_file(path: &std::path::Path, context: &str) {
    for _ in 0..500 {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {context}: {}", path.display());
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn replace_supervisor_lease(store: &HarnessStore, run_id: &str) {
    let lease = store
        .latest_team_supervisor_lease(run_id)
        .expect("read current lease")
        .expect("current lease");
    store
        .release_team_supervisor_lease(
            run_id,
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms(),
        )
        .expect("release current lease");
    store
        .acquire_team_supervisor_lease(
            run_id,
            "terminal-frame-fencing-supervisor",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms(),
            15_000,
        )
        .expect("replace current lease");
}

fn member_semantic_row_counts(store: &HarnessStore, member_id: &str) -> (usize, usize, usize) {
    let member_rows = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .filter(|member| member.id == member_id)
        .count();
    let actions = store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .filter(|action| action.member_run_id == member_id)
        .count();
    let handoffs = store
        .team_messages()
        .expect("team messages")
        .into_iter()
        .filter(|message| {
            message.from_member_id == member_id
                && message.kind == harness_core::TeamMessageKind::Handoff
        })
        .count();
    (member_rows, actions, handoffs)
}

fn init_project_selector_clean(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness_with_env(home, &root, &["init"], NATIVE_SELECTOR_CLEAN_ENV);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// `harness init` a project rooted at `<base>/<name>` and return its derived id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Seed the native Mission/Wave ledgers directly so the public team-run
/// surfaces can prove their optional joins without depending on a separate
/// Mission authoring command in this integration suite.
fn seed_native_mission_wave(home: &TempHome, project_id: &str) {
    let store = home.spaces_dir().join(project_id);
    std::fs::write(
        store.join("missions.jsonl"),
        serde_json::json!({
            "id": "mission-test",
            "title": "Test Mission",
            "objective": "Exercise team-run join",
            "desired_outcome": null,
            "status": "running",
            "wave_ids": ["wave-test"],
            "outcome_summary": null,
            "created_at": "2026-07-19T00:00:00Z",
            "updated_at": "2026-07-19T00:00:00Z",
            "completed_at": null
        })
        .to_string()
            + "\n",
    )
    .expect("seed mission");
    std::fs::write(
        store.join("waves.jsonl"),
        serde_json::json!({
            "id": "wave-test",
            "mission_id": "mission-test",
            "index": 2,
            "title": "Test Wave",
            "objective": "Exercise team run",
            "exit_criteria": null,
            "status": "planned",
            "executor_kind": "agent_team",
            "executor_run_ids": [],
            "accepted_run_id": null,
            "plan_note": null,
            "outcome_summary": null,
            "artifact_refs": [],
            "gate_status": "pending",
            "gate_note": null,
            "accepted_by": null,
            "accepted_at": null,
            "created_at": "2026-07-19T00:00:00Z",
            "updated_at": "2026-07-19T00:00:00Z"
        })
        .to_string()
            + "\n",
    )
    .expect("seed wave");
}

/// Run `harness team-run ...` in the given project and return parsed stdout JSON.
fn team_run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id, "team-run"];
    full.extend_from_slice(args);
    let out = run_harness(home, home.base(), &full);
    assert!(
        out.status.success(),
        "team-run {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| panic!("team-run {args:?} stdout not JSON ({e})"))
}

/// Run a member-authorized `harness team-run ...` command with the same
/// runtime binding that a persistent provider process receives.
fn member_team_run_json(
    home: &TempHome,
    project_id: &str,
    team_run_id: &str,
    member_run_id: &str,
    args: &[&str],
) -> serde_json::Value {
    let mut full = vec!["--project", project_id, "team-run"];
    full.extend_from_slice(args);
    let out = run_harness_with_env(
        home,
        home.base(),
        &full,
        &[
            ("HARNESS_TEAM_RUN_ID", team_run_id),
            ("HARNESS_MEMBER_RUN_ID", member_run_id),
        ],
    );
    assert!(
        out.status.success(),
        "member team-run {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| panic!("member team-run {args:?} stdout not JSON ({e})"))
}

fn command_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_harness(home, home.base(), &full);
    assert!(
        out.status.success(),
        "command {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| panic!("command {args:?} stdout not JSON ({e})"))
}

#[test]
fn team_run_cli_create_list_status_send_events() {
    let home = TempHome::new("team-run-cli");
    let project_id = init_project(&home, "alpha");
    seed_native_mission_wave(&home, &project_id);
    let project_root = std::fs::canonicalize(home.base().join("alpha"))
        .expect("canonical project root")
        .display()
        .to_string();

    // create (plain output): bare run id on stdout.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Ship v0",
            "--mission-id",
            "mission-test",
            "--wave-id",
            "wave-test",
            "--budget-usd",
            "5.5",
            "--execution-root",
            &project_root,
            "--member",
            "lead:coordinator:kimi#Coordinate the delivery",
            "--member",
            "worker-1:implementer:codex:gpt-5@crates/a,docs#Implement and verify the change",
            "--member-effort",
            "worker-1:max",
            "--member-service-tier",
            "worker-1:priority",
            "--member-worktree",
            &format!("worker-1:{project_root}"),
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(run_id.starts_with("team-run-"), "run id: {run_id}");

    // list --json: one run, wave/budget/member ids carried through.
    let runs = team_run_json(&home, &project_id, &["list", "--json"]);
    let runs = runs.as_array().expect("runs array");
    assert_eq!(runs.len(), 1, "runs: {runs:?}");
    assert_eq!(runs[0]["id"].as_str(), Some(run_id.as_str()));
    assert_eq!(runs[0]["status"].as_str(), Some("planning"));
    assert_eq!(runs[0]["wave_index"].as_u64(), Some(2));
    assert_eq!(runs[0]["mission_id"].as_str(), Some("mission-test"));
    assert_eq!(
        runs[0]["execution_root"].as_str(),
        Some(project_root.as_str())
    );
    assert_eq!(runs[0]["wave_id"].as_str(), Some("wave-test"));
    assert_eq!(runs[0]["budget_limit_usd"].as_f64(), Some(5.5));
    let member_ids: Vec<&str> = runs[0]["member_run_ids"]
        .as_array()
        .expect("member_run_ids")
        .iter()
        .filter_map(|id| id.as_str())
        .collect();
    assert_eq!(member_ids.len(), 2, "member ids: {member_ids:?}");

    // status --json: members + no actions yet + no conversation mail.
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(status["team_run"]["id"].as_str(), Some(run_id.as_str()));
    let members = status["members"].as_array().expect("members");
    assert_eq!(members.len(), 2, "members: {members:?}");
    let controlled_member = members
        .iter()
        .find(|entry| entry["member_run"]["name"].as_str() == Some("worker-1"))
        .expect("worker-1 MemberRun");
    assert_eq!(
        controlled_member["member_run"]["provider_controls"]["model"]["requested"].as_str(),
        Some("gpt-5")
    );
    assert_eq!(
        controlled_member["member_run"]["provider_controls"]["reasoning_effort"]["requested"]
            .as_str(),
        Some("max")
    );
    assert_eq!(
        controlled_member["member_run"]["provider_controls"]["service_tier"]["requested"].as_str(),
        Some("priority")
    );
    assert_eq!(
        members[0]["member_run"]["name"].as_str(),
        Some("lead"),
        "member order follows --member order"
    );
    assert_eq!(members[1]["member_run"]["model"].as_str(), Some("gpt-5"));
    assert_eq!(
        members[1]["member_run"]["worktree_ref"].as_str(),
        Some(project_root.as_str())
    );
    assert_eq!(
        members[1]["member_run"]["owned_paths"],
        serde_json::json!(["crates/a", "docs"]),
        "owned_paths parsed from @path1,path2"
    );
    assert!(
        members.iter().all(|m| m["latest_action"].is_null()),
        "no member actions journaled yet: {members:?}"
    );
    assert_eq!(
        status["unacked_messages"].as_u64(),
        Some(0),
        "queued deliveries are not actionable manual acknowledgements"
    );
    let member_detail = command_json(
        &home,
        &project_id,
        &["member-run", "show", "--id", member_ids[1], "--json"],
    );
    assert_eq!(
        member_detail["member_run"]["id"].as_str(),
        Some(member_ids[1])
    );
    assert_eq!(
        member_detail["team_run"]["id"].as_str(),
        Some(run_id.as_str())
    );
    assert_eq!(
        member_detail["works"].as_array().map(Vec::len),
        Some(1),
        "member detail includes its durable owned Work"
    );
    assert_eq!(
        member_detail["mailbox"]["inbox"].as_array().map(Vec::len),
        Some(0),
        "Work ownership is not duplicated into TeamMessage"
    );
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let works = store.latest_works().expect("latest Works");
    let worker_work = works
        .iter()
        .find(|work| work.active_member_run_id.as_deref() == Some(member_ids[1]))
        .expect("worker Work");

    // send --json: a blocker from the worker to the lead.
    let message = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            member_ids[1],
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--body",
            "BLOCKER: API design is unresolved",
            "--work-id",
            &worker_work.id,
            "--json",
        ],
    );
    assert_eq!(message["kind"].as_str(), Some("message"));
    assert_eq!(message["from_member_id"].as_str(), Some(member_ids[1]));
    assert_eq!(message["team_run_id"].as_str(), Some(run_id.as_str()));
    assert_eq!(
        message["deliveries"][0]["status"].as_str(),
        Some("queued"),
        "delivery queued: {message:?}"
    );
    assert!(
        !message["correlation_id"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "correlation id assigned"
    );
    assert!(
        message["response_intent"].is_null(),
        "peer-to-peer message mail carries no explicit intent (informational by default): {message:?}"
    );

    // Sender-aware default (ADR 0046 §4): the same bare `--kind message` from
    // Host stays response-required, because `message` is the only legal
    // carrier for Host questions, revisions, and acceptance decisions.
    let host_mail = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--body",
            "Revise the API surface and report back",
            "--json",
        ],
    );
    assert!(
        host_mail["response_intent"].is_null(),
        "Host mail also carries no explicit intent; the default is sender-aware: {host_mail:?}"
    );

    // --informational is the explicit downward override for Host mail that is
    // genuinely FYI-only, mirroring the HTTP/MCP `response_intent` field.
    let host_fyi = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--informational",
            "--body",
            "FYI: the nightly gate is green",
            "--json",
        ],
    );
    assert_eq!(
        host_fyi["response_intent"].as_str(),
        Some("informational"),
        "CLI --informational sets the explicit downward override: {host_fyi:?}"
    );

    // --response-required marks mail that must wake an idle peer into a new
    // provider round (ADR 0046 §4).
    let flagged = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            member_ids[1],
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--response-required",
            "--body",
            "QUESTION: which API revision should the peer lane implement?",
            "--json",
        ],
    );
    assert_eq!(
        flagged["response_intent"].as_str(),
        Some("response_required"),
        "CLI --response-required sets explicit intent: {flagged:?}"
    );
    let inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            member_ids[0],
            "--json",
        ],
    );
    assert!(
        inbox
            .as_array()
            .expect("CLI inbox array")
            .iter()
            .any(|item| item["id"] == message["id"]),
        "CLI inbox must expose peer coordination mail: {inbox}"
    );

    // events --json: 5 create-time events + 4 send events, seq 1..=9 in order.
    let events = team_run_json(&home, &project_id, &["events", "--id", &run_id, "--json"]);
    let events = events.as_array().expect("events array");
    assert_eq!(events.len(), 9, "events: {events:?}");
    let seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    assert_eq!(
        seqs,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
        "seq strictly increasing"
    );
    assert_eq!(events[0]["entity_type"].as_str(), Some("team_run"));
    assert_eq!(events[0]["operation"].as_str(), Some("created"));
    assert_eq!(events[0]["source_kind"].as_str(), Some("host"));
    // The send folded a member-sourced message event (v0: no member status flip).
    let last = &events[5];
    assert_eq!(last["entity_type"].as_str(), Some("message"));
    assert_eq!(last["source_kind"].as_str(), Some("member"));
    assert_eq!(last["member_run_id"].as_str(), Some(member_ids[1]));

    // events --after-seq 5: only the four send events remain.
    let tail = team_run_json(
        &home,
        &project_id,
        &["events", "--id", &run_id, "--after-seq", "5", "--json"],
    );
    let tail = tail.as_array().expect("tail array");
    assert_eq!(tail.len(), 4, "tail: {tail:?}");
    let tail_seqs: Vec<u64> = tail.iter().filter_map(|e| e["seq"].as_u64()).collect();
    assert_eq!(tail_seqs, vec![6, 7, 8, 9]);

    // Member-to-Host mail is actionable immediately; CLI ACK is the complete
    // control-plane path and removes it from the default Inbox without erasing
    // the latest historical projection.
    let host_message = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            member_ids[1],
            "--to",
            "host",
            "--kind",
            "handoff",
            "--body",
            "RESULT: ready for Host review",
            "--work-id",
            &worker_work.id,
            "--correlation-id",
            message["correlation_id"]
                .as_str()
                .expect("conversation correlation"),
            "--causation-id",
            message["id"].as_str().expect("conversation root"),
            "--json",
        ],
    );
    assert_eq!(
        host_message["deliveries"][0]["status"].as_str(),
        Some("delivered")
    );
    let host_inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--json",
        ],
    );
    assert_eq!(host_inbox.as_array().map(Vec::len), Some(1));
    let ack = team_run_json(
        &home,
        &project_id,
        &[
            "ack",
            "--id",
            &run_id,
            "--message-id",
            host_message["id"].as_str().expect("Host message id"),
            "--member-id",
            "host",
            "--json",
        ],
    );
    assert_eq!(
        ack["deliveries"][0]["status"].as_str(),
        Some("acknowledged")
    );
    let actionable_after_ack = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--json",
        ],
    );
    assert_eq!(actionable_after_ack.as_array().map(Vec::len), Some(0));
    let history_after_ack = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--all",
            "--json",
        ],
    );
    assert_eq!(history_after_ack.as_array().map(Vec::len), Some(1));

    // create --json: the full created bundle (run + member runs + Works).
    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Second run",
            "--member",
            "solo:worker:kimi#Complete the solo lane",
            "--json",
        ],
    );
    assert_eq!(created["team_run"]["status"].as_str(), Some("planning"));
    assert_eq!(
        created["member_runs"].as_array().map(Vec::len),
        Some(1),
        "member runs: {created:?}"
    );
    let works = created["works"].as_array().expect("Works");
    assert_eq!(works.len(), 1);
    assert_eq!(works[0]["status"].as_str(), Some("open"));
    assert!(works[0]["active_member_run_id"].as_str().is_some());
}

#[test]
fn team_run_cli_message_reuses_conversation_lineage_only_within_its_run() {
    let home = TempHome::new("team-run-cli-lineage");
    let project_id = init_project(&home, "alpha");
    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Correlate work",
            "--member",
            "lead:coordinator:kimi",
            "--member",
            "worker:implementer:kimi",
            "--json",
        ],
    );
    let run_id = created["team_run"]["id"].as_str().unwrap().to_string();
    let members = created["member_runs"].as_array().unwrap();
    let root = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            members[0]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "Please coordinate this conversation",
            "--informational",
            "--json",
        ],
    );
    let root_id = root["id"].as_str().unwrap();
    let correlation_id = root["correlation_id"].as_str().unwrap();

    let handoff = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "handoff",
            "--body",
            "handoff linked to the conversation",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            root_id,
            "--json",
        ],
    );
    assert_eq!(handoff["correlation_id"].as_str(), Some(correlation_id));
    assert_eq!(handoff["causation_id"].as_str(), Some(root_id));

    // A causation-only reply inherits its direct cause's correlation rather
    // than fabricating a fresh one.
    let reply = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            members[1]["id"].as_str().unwrap(),
            "--to",
            members[0]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "acknowledged",
            "--causation-id",
            handoff["id"].as_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(reply["correlation_id"].as_str(), Some(correlation_id));
    assert_eq!(reply["causation_id"].as_str(), handoff["id"].as_str());

    let foreign = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Separate team boundary",
            "--member",
            "outsider:implementer:kimi",
            "--json",
        ],
    );
    let foreign_member_id = foreign["member_runs"][0]["id"].as_str().unwrap();
    let messages_before_invalid = std::fs::read_to_string(
        home.spaces_dir()
            .join(&project_id)
            .join("team_messages.jsonl"),
    )
    .expect("read messages before invalid sends")
    .lines()
    .count();

    // A member from another TeamRun cannot impersonate a sender in this run,
    // even when it presents valid conversation lineage from the target run.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            foreign_member_id,
            "--to",
            members[0]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "cross-run impersonation",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            root_id,
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not belong to team run"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Recipient membership is checked before any message or event is written.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            "member-run-unknown",
            "--kind",
            "message",
            "--body",
            "unknown recipient",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            root_id,
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not belong to team run"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let messages_after_invalid = std::fs::read_to_string(
        home.spaces_dir()
            .join(&project_id)
            .join("team_messages.jsonl"),
    )
    .expect("read messages after invalid sends")
    .lines()
    .count();
    assert_eq!(messages_after_invalid, messages_before_invalid);

    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "unproven correlation",
            "--correlation-id",
            "corr-not-a-conversation",
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not identify a conversation"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let second_root = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "A separate conversation",
            "--json",
        ],
    );

    // A causation id from the same run must still agree with an explicitly
    // supplied correlation; the rejected send leaves the event stream intact.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "mismatched lineage",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            second_root["id"].as_str().unwrap(),
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("has correlation_id"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = team_run_json(&home, &project_id, &["events", "--id", &run_id, "--json"]);
    assert_eq!(events.as_array().map(Vec::len), Some(7));
}

#[test]
fn team_run_rejects_non_agent_team_wave_before_journaling_attempt() {
    let home = TempHome::new("team-run-wrong-executor");
    let project_id = init_project(&home, "alpha");
    seed_native_mission_wave(&home, &project_id);
    let wave_path = home.spaces_dir().join(&project_id).join("waves.jsonl");
    let wave = std::fs::read_to_string(&wave_path)
        .expect("read seeded wave")
        .replace("\"agent_team\"", "\"dynamic_workflow\"");
    std::fs::write(&wave_path, wave).expect("replace executor kind");

    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "must not start",
            "--wave-id",
            "wave-test",
            "--member",
            "worker:implementer:kimi",
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not agent_team"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !home
            .spaces_dir()
            .join(&project_id)
            .join("team_runs.jsonl")
            .exists(),
        "failed validation must not append a TeamRun"
    );
}

#[test]
fn mission_wave_cli_authoring_and_accepted_team_gate() {
    let home = TempHome::new("mission-wave-cli");
    let project_id = init_project(&home, "alpha");
    let mission = command_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-cli",
            "--title",
            "CLI Mission",
            "--objective",
            "Prove the native authoring surface",
            "--desired-outcome",
            "One accepted Wave",
            "--json",
        ],
    );
    assert_eq!(mission["id"].as_str(), Some("mission-cli"));
    let wave = command_json(
        &home,
        &project_id,
        &[
            "wave",
            "create",
            "--id",
            "wave-cli",
            "--mission-id",
            "mission-cli",
            "--title",
            "Reviewed TeamRun",
            "--objective",
            "Complete one assigned member attempt",
            "--executor-kind",
            "agent_team",
            "--json",
        ],
    );
    assert_eq!(wave["index"].as_u64(), Some(1));
    assert_eq!(wave["executor_kind"].as_str(), Some("agent_team"));

    let run = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "empty completion",
            "--mission-id",
            "mission-cli",
            "--wave-id",
            "wave-cli",
            "--member",
            "worker:implementer:kimi",
            "--json",
        ],
    );
    let run_id = run["team_run"]["id"].as_str().unwrap().to_string();
    let mut reviewing = run["team_run"].clone();
    reviewing["status"] = serde_json::json!("reviewing");
    reviewing["updated_at"] = serde_json::json!("unix-ms:review-ready");
    use std::io::Write as _;
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(home.spaces_dir().join(&project_id).join("team_runs.jsonl"))
        .expect("open team run ledger");
    writeln!(ledger, "{reviewing}").expect("append reviewing row");
    let completed = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "complete",
            "--id",
            &run_id,
        ],
    );
    assert!(
        completed.status.success(),
        "team completion failed: {}",
        String::from_utf8_lossy(&completed.stderr)
    );
    let waiting_wave = command_json(
        &home,
        &project_id,
        &["wave", "show", "--id", "wave-cli", "--json"],
    );
    assert_eq!(waiting_wave["status"].as_str(), Some("waiting"));
    let running_mission = command_json(
        &home,
        &project_id,
        &["mission", "show", "--id", "mission-cli", "--json"],
    );
    assert_eq!(running_mission["status"].as_str(), Some("running"));
    let gated = command_json(
        &home,
        &project_id,
        &[
            "wave",
            "gate",
            "--id",
            "wave-cli",
            "--status",
            "accepted",
            "--run-id",
            &run_id,
            "--accepted-by",
            "operator",
            "--note",
            "gate passed",
            "--outcome",
            "assigned run completed",
            "--artifact",
            "artifact:smoke",
            "--json",
        ],
    );
    assert_eq!(gated["gate_status"].as_str(), Some("accepted"));
    assert_eq!(gated["status"].as_str(), Some("completed"));
    assert_eq!(gated["accepted_run_id"].as_str(), Some(run_id.as_str()));
    assert_eq!(gated["accepted_by"].as_str(), Some("operator"));
    assert_eq!(
        gated["artifact_refs"],
        serde_json::json!(["artifact:smoke"])
    );

    let mission = command_json(
        &home,
        &project_id,
        &["mission", "show", "--id", "mission-cli", "--json"],
    );
    assert_eq!(mission["wave_ids"], serde_json::json!(["wave-cli"]));
}

#[test]
fn post_mission_wave_and_lightweight_gate() {
    let home = TempHome::new("mission-wave-http");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-http",
            "title": "HTTP Mission",
            "objective": "Author via API"
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["id"].as_str(), Some("mission-http"));
    let (status, body) = serve.post_json(
        "/v1/waves",
        &serde_json::json!({
            "id": "wave-http",
            "mission_id": "mission-http",
            "title": "HTTP Wave",
            "objective": "Gate without accepting",
            "executor_kind": "host"
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["index"].as_u64(), Some(1));
    assert!(body.get("snapshot").is_none());
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot["missions"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["waves"].as_array().map(Vec::len), Some(1));
    let (status, body) = serve.post_json(
        "/v1/waves/wave-http/gate",
        &serde_json::json!({"status": "revise", "note": "clarify scope"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["gate_status"].as_str(), Some("revise"));
    assert_eq!(body["result"]["status"].as_str(), Some("planned"));
    assert_eq!(body["result"]["gate_note"].as_str(), Some("clarify scope"));
}

#[test]
fn get_team_member_inbox_uses_actionable_latest_wins_projection() {
    let home = TempHome::new("team-inbox-http");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise member inbox",
            "members": [
                {"name": "member-a", "role": "builder", "provider": "codex"},
                {"name": "member-b", "role": "reviewer", "provider": "codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id");
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id");
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "Please review the shared Work board"
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    let message_id = sent["result"]["id"].as_str().expect("message id");

    let (status, inbox) =
        serve.get_json(&format!("/v1/team-runs/{run_id}/members/{member_id}/inbox"));
    assert_eq!(status, 200, "body: {inbox}");
    assert_eq!(
        inbox["messages"].as_array().map(Vec::len),
        Some(1),
        "queued ordinary message is actionable"
    );
    assert_eq!(inbox["messages"][0]["id"].as_str(), Some(message_id));
    let (status, all) = serve.get_json(&format!(
        "/v1/team-runs/{run_id}/members/{member_id}/inbox?all=true"
    ));
    assert_eq!(status, 200, "body: {all}");
    assert_eq!(all["messages"].as_array().map(Vec::len), Some(1));
}

#[test]
fn get_host_inbox_is_scoped_to_exact_native_thread() {
    let home = TempHome::new("host-inbox-http");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise native Host inbox",
            "host_surface": "codex-app",
            "host_thread_id": "codex-thread-http-a",
            "members": [
                {"name": "member-a", "role": "builder", "provider": "codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id");
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id");
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": member_id,
            "to_member_ids": ["host"],
            "kind": "message",
            "body": "QUESTION: choose A or B",
        }),
    );
    assert_eq!(status, 200, "body: {sent}");

    let (status, exact) =
        serve.get_json("/v1/team-runs/host-inbox?surface=codex-app&thread_id=codex-thread-http-a");
    assert_eq!(status, 200, "body: {exact}");
    assert_eq!(exact["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(exact["runs"][0]["team_run_id"].as_str(), Some(run_id));
    assert_eq!(
        exact["runs"][0]["messages"].as_array().map(Vec::len),
        Some(1)
    );

    let (status, other) =
        serve.get_json("/v1/team-runs/host-inbox?surface=codex-app&thread_id=another-thread");
    assert_eq!(status, 200, "body: {other}");
    assert_eq!(other["runs"].as_array().map(Vec::len), Some(0));
}

#[test]
fn linked_team_run_rejects_previous_attempt_from_another_wave() {
    let home = TempHome::new("team-run-previous-wave");
    let project_id = init_project(&home, "alpha");
    for (mission_id, wave_id) in [("mission-a", "wave-a"), ("mission-b", "wave-b")] {
        let _ = command_json(
            &home,
            &project_id,
            &[
                "mission",
                "create",
                "--id",
                mission_id,
                "--title",
                mission_id,
                "--objective",
                "test lineage",
                "--json",
            ],
        );
        let _ = command_json(
            &home,
            &project_id,
            &[
                "wave",
                "create",
                "--id",
                wave_id,
                "--mission-id",
                mission_id,
                "--title",
                wave_id,
                "--objective",
                "test lineage",
                "--executor-kind",
                "agent_team",
                "--json",
            ],
        );
    }
    let first = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "first",
            "--mission-id",
            "mission-a",
            "--wave-id",
            "wave-a",
            "--member",
            "worker-a:implementer:kimi",
            "--json",
        ],
    );
    let first_id = first["team_run"]["id"].as_str().unwrap();
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "invalid retry",
            "--mission-id",
            "mission-b",
            "--wave-id",
            "wave-b",
            "--previous",
            first_id,
            "--member",
            "worker-b:implementer:kimi",
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not an attempt of mission"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let runs = team_run_json(&home, &project_id, &["list", "--json"]);
    assert_eq!(runs.as_array().map(Vec::len), Some(1));
}

#[test]
fn post_team_run_creates_entities_and_get_snapshot_projects_them() {
    let home = TempHome::new("team-run-api");
    let project_id = init_project(&home, "alpha");
    let project_root =
        std::fs::canonicalize(home.base().join("alpha")).expect("canonical project root");
    seed_native_mission_wave(&home, &project_id);
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Ship v0",
            "mission_id": "mission-test",
            "wave_id": "wave-test",
            "execution_root": project_root,
            "budget_limit_usd": 5.0,
            "members": [
                {"name": "lead", "role": "coordinator", "provider": "kimi",
                 "initial_work": "Coordinate the delivery"},
                {"name": "worker-1", "role": "implementer", "provider": "codex",
                 "model": "gpt-5", "worktree_ref": project_root, "owned_paths": ["crates/a"],
                 "initial_work": "Implement and verify the change"},
            ],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(true), "body: {body}");

    // result: the created bundle (run + member runs + initial Works).
    let result = &body["result"];
    assert_eq!(result["team_run"]["objective"].as_str(), Some("Ship v0"));
    assert_eq!(result["team_run"]["status"].as_str(), Some("planning"));
    assert_eq!(
        result["team_run"]["mission_id"].as_str(),
        Some("mission-test")
    );
    assert_eq!(result["team_run"]["wave_id"].as_str(), Some("wave-test"));
    assert_eq!(
        result["team_run"]["execution_root"].as_str(),
        Some(project_root.to_str().expect("project root"))
    );
    assert_eq!(
        result["team_run"]["host_surface"].as_str(),
        Some("http"),
        "HTTP-created runs default host_surface to http"
    );
    assert_eq!(result["member_runs"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        result["member_runs"][1]["worktree_ref"].as_str(),
        Some(project_root.to_str().expect("project root"))
    );
    assert_eq!(result["works"].as_array().map(Vec::len), Some(2));
    let run_id = result["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();

    // Mutations stay bounded; the follow-up GET carries the projections.
    assert!(body.get("snapshot").is_none());
    let (snapshot_status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot_status, 200);
    let team_runs = snapshot["team_runs"].as_array().expect("team_runs");
    assert_eq!(team_runs.len(), 1, "team_runs: {team_runs:?}");
    assert_eq!(team_runs[0]["id"].as_str(), Some(run_id.as_str()));
    assert!(
        team_runs[0].get("wave_index").is_none(),
        "the persisted TeamRun has no second Wave ordering field"
    );
    assert_eq!(team_runs[0]["budget_limit_usd"].as_f64(), Some(5.0));
    assert_eq!(
        team_runs[0]["member_run_ids"].as_array().map(Vec::len),
        Some(2)
    );
    let waves = snapshot["waves"].as_array().expect("waves");
    assert_eq!(waves.len(), 1, "waves: {waves:?}");
    assert_eq!(waves[0]["id"].as_str(), Some("wave-test"));
    assert_eq!(
        waves[0]["executor_run_ids"],
        serde_json::json!([run_id]),
        "linked Wave owns the new AgentTeamRun attempt"
    );

    let member_runs = snapshot["member_runs"].as_array().expect("member_runs");
    assert_eq!(member_runs.len(), 2, "member_runs: {member_runs:?}");
    assert!(
        member_runs
            .iter()
            .all(|m| m["status"].as_str() == Some("idle")),
        "members start idle: {member_runs:?}"
    );

    let messages = snapshot["team_messages"].as_array().expect("team_messages");
    assert_eq!(
        messages.len(),
        0,
        "Work ownership must not create chat: {messages:?}"
    );
    let works = snapshot["works"].as_array().expect("Works");
    assert_eq!(works.len(), 2, "Works: {works:?}");
    assert!(
        works
            .iter()
            .all(|work| work["status"].as_str() == Some("open")
                && work["claim_mode"].as_str() == Some("host_assign")
                && work["active_member_run_id"].as_str().is_some()),
        "host-assigned initial Works: {works:?}"
    );

    // Folded events: 1 run + 2 member runs + 2 Works, seq 1..=5.
    let events = snapshot["team_run_events"]
        .as_array()
        .expect("team_run_events");
    assert_eq!(events.len(), 5, "events: {events:?}");
    let mut seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    assert!(
        events
            .iter()
            .all(|e| e["team_run_id"].as_str() == Some(run_id.as_str())
                && e["operation"].as_str() == Some("created")),
        "all events folded into the run: {events:?}"
    );

    assert_eq!(snapshot["team_runs"].as_array().map(Vec::len), Some(1));
}

#[test]
fn post_mutation_response_is_bounded_and_dashboard_can_refresh_from_get_snapshot() {
    let home = TempHome::new("bounded-mutation-response");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let large_context = "x".repeat(20_000);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "remain reachable from a deep link",
            "members": [{"name": "deep-link-member", "role": "auditor", "provider": "codex"}],
        }),
    );
    assert_eq!(status, 200, "created: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();

    for index in 0..80 {
        let (status, body) = serve.post_json(
            "/v1/missions",
            &serde_json::json!({
                "id": format!("mission-large-{index}"),
                "title": format!("Large mission {index}"),
                "objective": "inflate the durable read projection",
                "context": large_context,
            }),
        );
        assert_eq!(status, 200, "body: {body}");
        assert!(
            body.get("snapshot").is_none(),
            "mutation response leaked a full snapshot"
        );
        assert!(
            serde_json::to_vec(&body).unwrap().len() < 64 * 1024,
            "mutation response exceeded the bounded envelope"
        );
    }

    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200, "snapshot: {snapshot}");
    assert_eq!(
        snapshot["missions"].as_array().map(Vec::len),
        Some(80),
        "the Dashboard refresh GET must still expose every mutation"
    );
    assert!(
        serde_json::to_vec(&snapshot).unwrap().len() > 1_000_000,
        "fixture did not prove the POST response was bounded against a multi-megabyte projection"
    );
    let (status, scoped) = serve.get_json(&format!("/v1/team-runs/{run_id}/snapshot"));
    assert_eq!(status, 200, "scoped: {scoped}");
    assert_eq!(scoped["team_runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(scoped["member_runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(scoped["missions"].as_array().map(Vec::len), Some(0));
    assert!(
        serde_json::to_vec(&scoped).unwrap().len() < 64 * 1024,
        "Team deep-link projection must remain bounded despite a large historical store"
    );
}

#[test]
fn post_team_run_message_and_start_async() {
    let home = TempHome::new("team-run-msg");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
        ],
    );

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Route mail",
            "members": [
                {"name": "lead", "role": "coordinator", "provider": "kimi",
                 "initial_work": "Coordinate delivery"},
                {"name": "worker-1", "role": "implementer", "provider": "kimi",
                 "initial_work": "Implement the requested change"},
            ],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let run_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_ids: Vec<String> = body["result"]["member_runs"]
        .as_array()
        .expect("member runs")
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect();
    assert_eq!(member_ids.len(), 2);
    let worker_work_id = body["result"]["works"][1]["id"]
        .as_str()
        .expect("worker Work id")
        .to_string();

    // Route a handoff from the worker to the lead.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": member_ids[1],
            "to_member_ids": [member_ids[0]],
            "kind": "handoff",
            "body": "take over the review",
            "work_id": worker_work_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(true), "body: {body}");
    assert_eq!(body["result"]["kind"].as_str(), Some("handoff"));
    assert!(body["result"]["correlation_id"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(body["result"]["causation_id"].is_null());
    assert_eq!(
        body["result"]["team_run_id"].as_str(),
        Some(run_id.as_str())
    );
    assert_eq!(
        body["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    // Work ownership is separate from the one explicit conversation message.
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot["team_messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        snapshot["team_run_events"].as_array().map(Vec::len),
        Some(6)
    );

    // Unknown run id → 400, nothing journaled.
    let (status, body) = serve.post_json(
        "/v1/team-runs/team-run-nope/messages",
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_ids[0]],
            "kind": "control",
            "body": "ping",
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(false), "body: {body}");

    // HTTP start claims planning -> running synchronously, then drives the
    // provider work in the background.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(true), "body: {body}");
    assert_eq!(
        body["result"]["id"].as_str(),
        Some(run_id.as_str()),
        "body: {body}"
    );
    assert_eq!(body["result"]["status"].as_str(), Some("running"));

    let (status, host_notice) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": member_ids[1],
            "to_member_ids": ["host"],
            "kind": "message",
            "body": "The Work is ready for Host review",
            "work_id": worker_work_id,
        }),
    );
    assert_eq!(status, 200, "body: {host_notice}");
    let host_handoff_id = host_notice["result"]["id"]
        .as_str()
        .expect("Host notice id")
        .to_string();

    // Dashboard ACK can only acknowledge an actually delivered recipient row
    // and the URL TeamRun must own the message.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/wrong-run/messages/{host_handoff_id}/ack"),
        &serde_json::json!({"member_id": "host"}),
    );
    assert_eq!(status, 400, "body: {body}");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages/{host_handoff_id}/ack"),
        &serde_json::json!({"member_id": "host"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["deliveries"][0]["status"].as_str(),
        Some("acknowledged")
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let ack_event_count = snapshot["team_run_events"]
        .as_array()
        .expect("team run events")
        .iter()
        .filter(|event| {
            event["entity_type"].as_str() == Some("message")
                && event["entity_id"].as_str() == Some(host_handoff_id.as_str())
                && event["summary"].as_str() == Some("message acknowledged by host")
        })
        .count();
    assert_eq!(
        ack_event_count, 1,
        "first ACK must add one message ACK event"
    );
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages/{host_handoff_id}/ack"),
        &serde_json::json!({"member_id": "host"}),
    );
    assert_eq!(status, 200, "body: {body}");
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let repeated_ack_event_count = snapshot["team_run_events"]
        .as_array()
        .expect("team run events")
        .iter()
        .filter(|event| {
            event["entity_type"].as_str() == Some("message")
                && event["entity_id"].as_str() == Some(host_handoff_id.as_str())
                && event["summary"].as_str() == Some("message acknowledged by host")
        })
        .count();
    assert_eq!(
        repeated_ack_event_count, ack_event_count,
        "idempotent ACK must not add another message ACK event"
    );
}

#[test]
fn persistent_codex_supervisor_survives_handoffs_transport_loss_and_team_completion() {
    let home = TempHome::new("team-run-persistent-codex-supervisor");
    let project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-persistent-codex"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let name_marker = home.base().join("codex-thread-names.jsonl");
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_AUTO_COMPLETE", "1"),
            ("FAKE_CODEX_EXIT_AFTER_FIRST_TURN", "1"),
            // This test intentionally sends follow-up mail after observing
            // idle. Keep the test-only supervisor bound well above slow CI
            // HTTP/snapshot latency; explicit Close still ends both members.
            ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
            (
                "FAKE_CODEX_NAME_MARKER",
                name_marker.to_str().expect("name marker"),
            ),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise persistent supervisor semantics",
            "members": [
                {"name": "Builder", "role": "implementer", "provider": "codex",
                 "initial_work": "Build and report the result"},
                {"name": "Reviewer", "role": "reviewer", "provider": "codex",
                 "initial_work": "Review and report the result"}
            ]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let builder_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let reviewer_id = created["result"]["member_runs"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let builder_work_id = created["result"]["works"]
        .as_array()
        .expect("Works")
        .iter()
        .find(|work| work["active_member_run_id"].as_str() == Some(builder_id.as_str()))
        .and_then(|work| work["id"].as_str())
        .expect("Builder Work")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut recovered_idle = false;
    for _ in 0..200 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let builder = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(builder_id.as_str()));
        let disconnected = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(builder_id.as_str())
                    && action["action_type"].as_str() == Some("disconnected")
            });
        recovered_idle = builder.is_some_and(|member| {
            member["status"].as_str() == Some("idle")
                && member["native_session"]["native_session_id"].as_str()
                    == Some("thread_fake_codex_app_server")
        }) && disconnected;
        if recovered_idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        recovered_idle,
        "transport loss was not exposed and resumed on the same native session"
    );

    // A TeamRun cannot be completed while its durable Works remain unfinished.
    // Provider RESULT only ends a native turn; the members must explicitly
    // submit their Works and the Host must explicitly accept them.
    let (status, rejected_completion) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 400, "body: {rejected_completion}");
    assert!(
        rejected_completion
            .to_string()
            .contains("Works remain non-terminal"),
        "completion guard should explain the unfinished Works: {rejected_completion}"
    );

    let mut both_idle = false;
    for _ in 0..200 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        both_idle = [&builder_id, &reviewer_id].iter().all(|member_id| {
            snapshot["member_runs"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|member| {
                    member["id"].as_str() == Some(member_id.as_str())
                        && member["status"].as_str() == Some("idle")
                })
        });
        if both_idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        both_idle,
        "both members must be idle before explicit Work review"
    );

    let (_, before_review) = serve.get_json("/v1/snapshot");
    let owned_works = before_review["works"]
        .as_array()
        .expect("Works in snapshot")
        .iter()
        .filter(|work| work["team_run_id"].as_str() == Some(run_id.as_str()))
        .map(|work| {
            (
                work["id"].as_str().expect("Work id").to_string(),
                work["active_member_run_id"]
                    .as_str()
                    .expect("owned Work member")
                    .to_string(),
                work["version"].as_u64().expect("Work version"),
                work["status"].as_str().expect("Work status").to_string(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(owned_works.len(), 2, "expected one Work per member");
    for (work_id, member_run_id, version, status) in owned_works {
        let active_version = if status == "open" {
            let started = member_team_run_json(
                &home,
                &project_id,
                &run_id,
                &member_run_id,
                &[
                    "work",
                    "start",
                    "--team-run-id",
                    &run_id,
                    "--work-id",
                    &work_id,
                    "--expected-version",
                    &version.to_string(),
                    "--member-run-id",
                    &member_run_id,
                    "--json",
                ],
            );
            started["version"].as_u64().expect("started version")
        } else {
            assert_eq!(status, "in_progress", "unexpected Work state before submit");
            version
        };
        let submitted = member_team_run_json(
            &home,
            &project_id,
            &run_id,
            &member_run_id,
            &[
                "work",
                "submit",
                "--team-run-id",
                &run_id,
                "--work-id",
                &work_id,
                "--expected-version",
                &active_version.to_string(),
                "--member-run-id",
                &member_run_id,
                "--result",
                "native turn completed; explicit Work submitted for Host review",
                "--json",
            ],
        );
        let submitted_version = submitted["version"].as_u64().expect("submitted version");
        let accepted = team_run_json(
            &home,
            &project_id,
            &[
                "work",
                "accept",
                "--team-run-id",
                &run_id,
                "--work-id",
                &work_id,
                "--expected-version",
                &submitted_version.to_string(),
                "--summary",
                "Host accepted the explicit Work result",
                "--json",
            ],
        );
        assert_eq!(accepted["status"].as_str(), Some("done"));
    }

    // The TeamRun decision remains independent of persistent Member runtime
    // lifetime once all durable Works are accepted.
    let (status, completed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {completed}");
    assert_eq!(completed["result"]["status"].as_str(), Some("completed"));

    let (status, host_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [builder_id],
            "kind": "message",
            "body": "HOST FOLLOW-UP after TeamRun completion",
        }),
    );
    assert_eq!(status, 200, "body: {host_mail}");
    let host_message_id = host_mail["result"]["id"].as_str().unwrap().to_string();
    let conversation_correlation = host_mail["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, peer_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": reviewer_id,
            "to_member_ids": [builder_id],
            "kind": "message",
            "response_intent": "response_required",
            "body": "PEER FOLLOW-UP after TeamRun completion",
            "correlation_id": conversation_correlation,
            "causation_id": host_message_id,
        }),
    );
    assert_eq!(status, 200, "body: {peer_mail}");
    let peer_message_id = peer_mail["result"]["id"].as_str().unwrap().to_string();

    let mut delivered_once = false;
    for _ in 0..200 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let messages = snapshot["team_messages"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let delivered = |message_id: &str| {
            messages
                .iter()
                .find(|message| message["id"].as_str() == Some(message_id))
                .is_some_and(|message| {
                    message["deliveries"][0]["status"].as_str() == Some("delivered")
                        && message["deliveries"][0]["attempt"].as_u64() == Some(1)
                })
        };
        delivered_once = delivered(&host_message_id) && delivered(&peer_message_id);
        if delivered_once {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        delivered_once,
        "Host and peer conversation mail were not each delivered exactly once"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let builder_completed_rounds = snapshot["member_actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|action| {
            action["member_run_id"].as_str() == Some(builder_id.as_str())
                && action["action_type"].as_str() == Some("turn_completed")
        })
        .count();
    assert!(
        builder_completed_rounds >= 2,
        "initial Work and follow-up conversation should produce provider rounds without fabricating Handoff messages: {builder_completed_rounds}"
    );
    let builder_work = snapshot["works"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|work| work["id"].as_str() == Some(builder_work_id.as_str()))
        .expect("Builder Work in snapshot");
    assert_eq!(
        builder_work["status"].as_str(),
        Some("done"),
        "the provider RESULT alone did not close Work; the explicit member submit and Host accept above did: {builder_work}"
    );

    let native_names = std::fs::read_to_string(&name_marker).expect("thread/name/set requests");
    assert!(
        native_names.contains("\"name\":\"Agent Team · Builder\"")
            && native_names.contains("\"name\":\"Agent Team · Reviewer\""),
        "native Codex threads were not named from Member identity: {native_names}"
    );

    for member_id in [&builder_id, &reviewer_id] {
        let (status, closed) = serve.post_json(
            &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
            &serde_json::json!({"requested_by": "host", "reason": "dogfood lane accepted"}),
        );
        assert_eq!(status, 200, "body: {closed}");
    }
    let mut all_stopped = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        all_stopped = [&builder_id, &reviewer_id].iter().all(|member_id| {
            snapshot["member_runs"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|member| {
                    member["id"].as_str() == Some(member_id.as_str())
                        && member["status"].as_str() == Some("stopped")
                })
        });
        if all_stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        all_stopped,
        "explicit Host close did not stop both runtimes"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert!(
        snapshot["team_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|run| {
                run["id"].as_str() == Some(run_id.as_str())
                    && run["status"].as_str() == Some("completed")
            }),
        "Member close must not rewrite the TeamRun decision"
    );
}

#[test]
fn stale_supervisor_quiesces_and_successor_resumes_mail_once() {
    let home = TempHome::new("team-run-stale-supervisor-quiescence");
    let project_id = init_project_selector_clean(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let first_prompt_ready = home.base().join("stale-first-prompt-ready");
    let first_prompt_release = home.base().join("stale-first-prompt-release");
    let prompt_marker = home.base().join("stale-kimi-prompts.jsonl");
    let attach_marker = home.base().join("stale-kimi-attach.log");
    let first_prompt_ready_value = first_prompt_ready.display().to_string();
    let first_prompt_release_value = first_prompt_release.display().to_string();
    let prompt_marker_value = prompt_marker.display().to_string();
    let attach_marker_value = attach_marker.display().to_string();
    let mut serve_env = vec![
        ("KIMI_CODE_BIN", fake_kimi.as_str()),
        ("FAKE_KIMI_RESULT", "done"),
        (
            "FAKE_KIMI_FIRST_PROMPT_READY",
            first_prompt_ready_value.as_str(),
        ),
        (
            "FAKE_KIMI_FIRST_PROMPT_RELEASE",
            first_prompt_release_value.as_str(),
        ),
        ("FAKE_KIMI_PROMPT_MARKER", prompt_marker_value.as_str()),
        ("FAKE_KIMI_ATTACH_MARKER", attach_marker_value.as_str()),
        ("HARNESS_TEAM_SUPERVISOR_LEASE_MS", "300"),
        ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "300"),
    ];
    serve_env.extend(NATIVE_SELECTOR_CLEAN_ENV.iter().copied());
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &serve_env);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Prove stale Supervisor quiescence",
            "members": [{
                "name": "kimi-lease-fence",
                "role": "runtime_reliability",
                "provider": "kimi",
                "initial_work": "Exercise lease fencing"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    for _ in 0..300 {
        if first_prompt_ready.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        first_prompt_ready.exists(),
        "stale generation never reached its first provider prompt"
    );

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let old_lease = store
        .latest_team_supervisor_lease(&run_id)
        .expect("read old lease")
        .expect("old lease");
    assert_eq!(old_lease.generation, 1);
    let initial_session = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .and_then(|member| member.native_session)
        .map(|session| session.native_session_id)
        .expect("initial native session");

    let (status, queued_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "QUEUED_FOR_SUCCESSOR",
        }),
    );
    assert_eq!(status, 200, "body: {queued_mail}");
    let queued_id = queued_mail["result"]["id"]
        .as_str()
        .expect("queued id")
        .to_string();
    let correlation = queued_mail["result"]["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();
    let (status, accepted_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "PROVIDER_ACCEPTED_BEFORE_LOSS",
            "correlation_id": correlation,
            "causation_id": queued_id,
        }),
    );
    assert_eq!(status, 200, "body: {accepted_mail}");
    let accepted_id = accepted_mail["result"]["id"]
        .as_str()
        .expect("accepted id")
        .to_string();
    let (status, uncertain_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "CLAIMED_WITHOUT_RECEIPT_BEFORE_LOSS",
            "correlation_id": correlation,
            "causation_id": accepted_id,
        }),
    );
    assert_eq!(status, 200, "body: {uncertain_mail}");
    let uncertain_id = uncertain_mail["result"]["id"]
        .as_str()
        .expect("uncertain id")
        .to_string();

    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis()
            .min(u64::MAX as u128) as u64
    };
    let claim_id = "claim-with-provider-receipt-before-lease-loss";
    let claimed = store
        .claim_team_message_delivery(
            &run_id,
            &accepted_id,
            &member_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            claim_id,
            now_ms(),
            15_000,
            "unix-ms:test-claimed",
        )
        .expect("claim accepted boundary");
    assert!(
        matches!(
            claimed,
            harness_store::TeamMessageDeliveryClaimResult::Claimed(_)
        ),
        "accepted boundary must be claimed exactly once"
    );
    store
        .complete_team_message_delivery_claim(
            &run_id,
            &accepted_id,
            &member_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            claim_id,
            "native-receipt-before-lease-loss",
            now_ms(),
            "unix-ms:test-delivered",
        )
        .expect("complete accepted boundary");
    let uncertain_claim_id = "claim-without-provider-receipt-before-lease-loss";
    let uncertain_claimed = store
        .claim_team_message_delivery(
            &run_id,
            &uncertain_id,
            &member_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            uncertain_claim_id,
            now_ms(),
            15_000,
            "unix-ms:test-uncertain-claimed",
        )
        .expect("claim uncertain boundary");
    assert!(
        matches!(
            uncertain_claimed,
            harness_store::TeamMessageDeliveryClaimResult::Claimed(_)
        ),
        "uncertain boundary must be claimed exactly once"
    );

    let prompt_count = || {
        std::fs::read_to_string(&prompt_marker)
            .unwrap_or_default()
            .lines()
            .count()
    };
    assert_eq!(
        prompt_count(),
        1,
        "only the blocked stale prompt may have reached the provider"
    );

    // Supersede generation 1 while its first prompt is blocked. uncertain_mail
    // is the claimed-without-receipt boundary, accepted_mail is the
    // claimed+receipt boundary, and queued_mail remains successor-owned work.
    store
        .release_team_supervisor_lease(
            &run_id,
            &old_lease.supervisor_id,
            old_lease.generation,
            now_ms(),
        )
        .expect("release stale generation");
    let fencing_lease = store
        .acquire_team_supervisor_lease(
            &run_id,
            "test-fencing-supervisor",
            std::process::id(),
            "tcp://127.0.0.1:1",
            now_ms(),
            10_000,
        )
        .expect("acquire fencing generation");
    assert_eq!(fencing_lease.generation, 2);

    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        prompt_count(),
        1,
        "stale generation must not start, resume, or prompt after lease loss"
    );
    let (control_status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({
            "reason": "must be fenced",
            "requested_by": "test"
        }),
    );
    assert_ne!(
        control_status, 200,
        "live control must reject the stale generation"
    );
    assert_eq!(
        prompt_count(),
        1,
        "rejected stale live control must not touch the provider"
    );
    let disconnected_actions = store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .filter(|action| action.member_run_id == member_id && action.action_type == "disconnected")
        .count();
    assert_eq!(
        disconnected_actions, 0,
        "lease loss must not enter the retrying starting-disconnected loop"
    );

    // Every fake ACP process treats its own first prompt as prompt #1. The
    // stale process is already quiesced, so release future successor prompts.
    std::fs::write(&first_prompt_release, b"release successor")
        .expect("release successor fake prompt");
    store
        .release_team_supervisor_lease(
            &run_id,
            &fencing_lease.supervisor_id,
            fencing_lease.generation,
            now_ms(),
        )
        .expect("release fencing generation");
    let mut successor_started = false;
    for _ in 0..200 {
        let (status, _) = serve.post_json(
            &format!("/v1/team-runs/{run_id}/start"),
            &serde_json::json!({}),
        );
        if status == 202 {
            successor_started = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        successor_started,
        "successor could not attach after stale generation quiesced"
    );

    let mut converged = None;
    let mut last_snapshot = None;
    for _ in 0..400 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let messages = snapshot["team_messages"]
            .as_array()
            .expect("snapshot messages");
        let delivery = |id: &str| {
            messages
                .iter()
                .find(|message| message["id"].as_str() == Some(id))
                .map(|message| message["deliveries"][0].clone())
        };
        let queued_delivery = delivery(&queued_id);
        let accepted_delivery = delivery(&accepted_id);
        let uncertain_delivery = delivery(&uncertain_id);
        let member = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()));
        // The successor emits one batched mail prompt after the stale Work
        // prompt. It must not invent a third prompt by replaying the initial
        // Work whose old-generation claim has no provider receipt; that
        // uncertainty remains durable until explicit reconciliation.
        let ready = uncertain_delivery.as_ref().is_some_and(|delivery| {
            delivery["status"] == "claimed"
                && delivery["attempt"] == 1
                && delivery["claimed_generation"] == 1
                && delivery["provider_receipt_id"].is_null()
        }) && queued_delivery.as_ref().is_some_and(|delivery| {
            delivery["status"] == "delivered"
                && delivery["attempt"] == 1
                && delivery["claimed_generation"] == 3
                && delivery["provider_receipt_id"]
                    .as_str()
                    .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:"))
        }) && accepted_delivery.as_ref().is_some_and(|delivery| {
            delivery["status"] == "delivered"
                && delivery["attempt"] == 1
                && delivery["claimed_generation"] == 1
                && delivery["provider_receipt_id"] == "native-receipt-before-lease-loss"
        }) && member.is_some_and(|member| {
            member["status"] == "idle"
                && member["native_session"]["native_session_id"] == initial_session
        }) && prompt_count() == 2;
        if ready {
            converged = Some(snapshot);
            break;
        }
        last_snapshot = Some(snapshot);
        std::thread::sleep(Duration::from_millis(20));
    }
    let snapshot = converged.unwrap_or_else(|| {
        panic!(
            "successor did not converge all delivery boundaries; prompts={}; attach={:?}; snapshot={}",
            prompt_count(),
            std::fs::read_to_string(&attach_marker),
            last_snapshot.unwrap_or_default(),
        )
    });
    let attach_log = std::fs::read_to_string(&attach_marker).expect("successor attach log");
    assert_eq!(
        attach_log.lines().count(),
        1,
        "successor must resume the same native session exactly once: {attach_log}"
    );
    assert!(
        attach_log.contains(&initial_session),
        "successor resumed a different native session: {attach_log}"
    );
    let handoffs = snapshot["team_messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| {
            message["kind"] == "handoff"
                && message["from_member_id"].as_str() == Some(member_id.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handoffs.len(),
        0,
        "the unresolved claimed delivery must continue fencing semantic handoff"
    );
    let prompt_log = std::fs::read_to_string(&prompt_marker).expect("successor prompt log");
    assert_eq!(
        prompt_log.matches("PROVIDER_ACCEPTED_BEFORE_LOSS").count(),
        0,
        "provider-accepted mail is already present in the resumed native session and must not be replayed: {prompt_log}"
    );
    assert_eq!(
        prompt_log.matches("QUEUED_FOR_SUCCESSOR").count(),
        1,
        "queued mail must reach exactly one successor prompt: {prompt_log}"
    );
    assert_eq!(
        prompt_log
            .matches("CLAIMED_WITHOUT_RECEIPT_BEFORE_LOSS")
            .count(),
        0,
        "claimed-without-receipt mail must remain uncertain and unreplayed: {prompt_log}"
    );
    assert_eq!(
        snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| action["action_type"] == "runtime_recovery")
            .count(),
        0,
        "provider-accepted mail lives in the resumed native session and must not invent a Work runtime-recovery action"
    );
}

#[test]
fn codex_terminal_frame_is_fenced_before_stale_semantic_writes() {
    let home = TempHome::new("team-run-codex-terminal-fence");
    let project_id = init_project_selector_clean(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ready = home.base().join("codex-terminal-received");
    let release = home.base().join("codex-terminal-release");
    let ready_value = ready.display().to_string();
    let release_value = release.display().to_string();
    let mut serve_env = vec![
        ("PATH", path.as_str()),
        ("FAKE_CODEX_AUTO_COMPLETE", "1"),
        (
            "HARNESS_TEST_CODEX_TERMINAL_RECEIVED_READY",
            ready_value.as_str(),
        ),
        (
            "HARNESS_TEST_CODEX_TERMINAL_RECEIVED_RELEASE",
            release_value.as_str(),
        ),
        ("HARNESS_TEAM_SUPERVISOR_LEASE_MS", "10000"),
        ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
    ];
    serve_env.extend(NATIVE_SELECTOR_CLEAN_ENV.iter().copied());
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &serve_env);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Fence a Codex terminal frame",
            "members": [{
                "name": "codex-terminal-fence",
                "role": "runtime_reliability",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise terminal fencing"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_file(&ready, "Codex terminal receive barrier");

    let store = HarnessStore::new(home.spaces_dir().join(project_id));
    let before = member_semantic_row_counts(&store, &member_id);
    assert_eq!(before.2, 0, "terminal frame was processed before barrier");
    replace_supervisor_lease(&store, &run_id);
    std::fs::write(&release, b"release stale terminal").expect("release Codex terminal");
    std::thread::sleep(Duration::from_millis(300));

    assert_eq!(
        member_semantic_row_counts(&store, &member_id),
        before,
        "stale Codex terminal result wrote native-session/member/action/Handoff state"
    );
}

#[test]
fn kimi_terminal_frame_is_fenced_before_stale_semantic_writes() {
    let home = TempHome::new("team-run-kimi-terminal-fence");
    let project_id = init_project_selector_clean(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let ready = home.base().join("kimi-terminal-received");
    let release = home.base().join("kimi-terminal-release");
    let ready_value = ready.display().to_string();
    let release_value = release.display().to_string();
    let mut serve_env = vec![
        ("KIMI_CODE_BIN", fake_kimi.as_str()),
        ("FAKE_KIMI_RESULT", "done"),
        (
            "HARNESS_TEST_KIMI_TERMINAL_RECEIVED_READY",
            ready_value.as_str(),
        ),
        (
            "HARNESS_TEST_KIMI_TERMINAL_RECEIVED_RELEASE",
            release_value.as_str(),
        ),
        ("HARNESS_TEAM_SUPERVISOR_LEASE_MS", "10000"),
        ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
    ];
    serve_env.extend(NATIVE_SELECTOR_CLEAN_ENV.iter().copied());
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &serve_env);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Fence a Kimi terminal frame",
            "members": [{
                "name": "kimi-terminal-fence",
                "role": "runtime_reliability",
                "provider": "kimi",
                "initial_work": "Exercise terminal fencing"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_file(&ready, "Kimi terminal receive barrier");

    let store = HarnessStore::new(home.spaces_dir().join(project_id));
    let before = member_semantic_row_counts(&store, &member_id);
    assert_eq!(before.2, 0, "terminal frame was processed before barrier");
    replace_supervisor_lease(&store, &run_id);
    std::fs::write(&release, b"release stale terminal").expect("release Kimi terminal");
    std::thread::sleep(Duration::from_millis(300));

    assert_eq!(
        member_semantic_row_counts(&store, &member_id),
        before,
        "stale Kimi terminal result wrote native-session/member/action/Handoff state"
    );
}

#[test]
fn heartbeat_failure_latch_rejects_close_while_durable_lease_is_current() {
    let home = TempHome::new("team-run-heartbeat-latch-control-fence");
    let project_id = init_project_selector_clean(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let prompt_ready = home.base().join("kimi-prompt-ready");
    let prompt_release = home.base().join("kimi-prompt-release");
    let heartbeat_failed = home.base().join("heartbeat-failed");
    let cancel_marker = home.base().join("kimi-cancel");
    let prompt_ready_value = prompt_ready.display().to_string();
    let prompt_release_value = prompt_release.display().to_string();
    let heartbeat_failed_value = heartbeat_failed.display().to_string();
    let cancel_marker_value = cancel_marker.display().to_string();
    let mut serve_env = vec![
        ("KIMI_CODE_BIN", fake_kimi.as_str()),
        ("FAKE_KIMI_FIRST_PROMPT_READY", prompt_ready_value.as_str()),
        (
            "FAKE_KIMI_FIRST_PROMPT_RELEASE",
            prompt_release_value.as_str(),
        ),
        ("FAKE_KIMI_CANCEL_MARKER", cancel_marker_value.as_str()),
        (
            "HARNESS_TEST_SUPERVISOR_HEARTBEAT_FAIL_READY",
            heartbeat_failed_value.as_str(),
        ),
        ("HARNESS_TEAM_SUPERVISOR_LEASE_MS", "10000"),
        ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
    ];
    serve_env.extend(NATIVE_SELECTOR_CLEAN_ENV.iter().copied());
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &serve_env);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Fence control after heartbeat failure",
            "members": [{
                "name": "kimi-control-fence",
                "role": "runtime_reliability",
                "provider": "kimi",
                "initial_work": "Exercise heartbeat fencing"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_file(&prompt_ready, "live Kimi prompt");
    wait_for_file(&heartbeat_failed, "injected heartbeat failure");

    let store = HarnessStore::new(home.spaces_dir().join(project_id));
    let lease = store
        .latest_team_supervisor_lease(&run_id)
        .expect("read durable lease")
        .expect("durable lease");
    assert_eq!(
        lease.status,
        harness_core::TeamSupervisorLeaseStatus::Active,
        "failure injection must leave the durable row apparently current"
    );
    assert!(
        lease.expires_unix_ms > current_unix_ms(),
        "durable lease expired before the local-latch assertion"
    );

    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({
            "reason": "must be rejected by local supervisor latch",
            "requested_by": "test"
        }),
    );
    assert_ne!(status, 200, "locally invalid Supervisor accepted Close");
    assert!(
        store
            .team_member_close_requests()
            .expect("close requests")
            .into_iter()
            .all(|request| request.member_run_id != member_id),
        "rejected Close persisted a durable close request"
    );
    assert!(
        !cancel_marker.exists(),
        "rejected Close reached the provider control transport"
    );
}

#[test]
fn codex_app_server_member_can_be_steered_in_place() {
    let home = TempHome::new("team-run-codex-app-server");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-app"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &[("PATH", path.as_str())]);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise live Codex control",
            "members": [{
                "name": "codex-live",
                "role": "implementer",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise live Codex control"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut live = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        live = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some("thread_fake_codex_app_server")
            });
        if live {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(live, "app-server member never became live");

    // Control the provider through a second Harness service process. The
    // durable lease routes this request to the Supervisor process that owns
    // the physical app-server connection; no process-local registry shortcut
    // is available to this client.
    let control_client = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, steered) = control_client.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({"content": "finish with the requested report", "requested_by": "operator"}),
    );
    assert_eq!(status, 200, "body: {steered}");
    assert_eq!(
        steered["result"]["control"]["delivery"].as_str(),
        Some("steered")
    );
    assert_eq!(
        steered["result"]["message"]["deliveries"][0]["policy"].as_str(),
        Some("inject")
    );

    let mut idle = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(idle, "steered member did not return to persistent idle");
}

#[test]
fn codex_app_server_post_handoff_steer_is_independent_and_converges_before_follow_up_round() {
    let home = TempHome::new("team-run-codex-post-handoff-steer");
    let project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-post-handoff"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_AUTO_COMPLETE_AFTER_STEER", "1"),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Converge one native turn after an explicit Handoff",
            "members": [{
                "name": "codex-convergence",
                "role": "implementer",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise same-turn convergence"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut live = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let member_running = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some("thread_fake_codex_app_server")
            });
        let work_delivered = snapshot["work_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()))
            .is_some_and(|delivery| {
                matches!(
                    delivery["status"].as_str(),
                    Some("claimed" | "provider_received")
                )
            });
        live = member_running && work_delivered;
        if live {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(live, "app-server member never became live");

    let (status, explicit_handoff) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_kind": "member_run",
            "sender_id": member_id,
            "to_member_ids": ["host"],
            "kind": "handoff",
            "body": "## RESULT\ndone\n## SUMMARY\nexplicit same-turn handoff",
            "work_id": work_id,
        }),
    );
    assert_eq!(status, 200, "body: {explicit_handoff}");
    let explicit_handoff_id = explicit_handoff["result"]["id"]
        .as_str()
        .expect("handoff id")
        .to_string();
    let conversation_correlation = explicit_handoff["result"]["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();

    let control_client = ServeHandle::spawn(&home, home.base(), &[]);
    let descendant_client = ServeHandle::spawn(&home, home.base(), &[]);
    let observer_barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let observer_ready = std::sync::Arc::clone(&observer_barrier);
    let observer = std::thread::spawn(move || {
        observer_ready.wait();
        for _ in 0..200 {
            let (_, snapshot) = descendant_client.get_json("/v1/snapshot");
            let control = snapshot["team_messages"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|message| {
                    message["kind"].as_str() == Some("control")
                        && message["body"].as_str()
                            == Some("incorporate the correction before ending this turn")
                });
            if let Some(control) = control {
                let control_id = control["id"].as_str().expect("Control id").to_string();
                let correlation_id = control["correlation_id"]
                    .as_str()
                    .expect("Control correlation")
                    .to_string();
                let observed_delivery = control["deliveries"][0].clone();
                return (control_id, correlation_id, observed_delivery);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("concurrent observer never saw the Steer Control")
    });
    observer_barrier.wait();
    let (status, steered) = control_client.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({
            "content": "incorporate the correction before ending this turn",
            "requested_by": "operator"
        }),
    );
    assert_eq!(status, 200, "body: {steered}");
    let steer_correlation = steered["result"]["message"]["correlation_id"]
        .as_str()
        .expect("Steer correlation")
        .to_string();
    assert_ne!(
        steer_correlation, conversation_correlation,
        "live control must not infer Work or conversation ownership from a prior Handoff"
    );
    assert!(steered["result"]["message"]["causation_id"].is_null());
    let steer_message_id = steered["result"]["message"]["id"]
        .as_str()
        .expect("Steer control message")
        .to_string();
    let (observed_control_id, observed_correlation, observed_delivery) =
        observer.join().expect("concurrent Control observer");
    assert_eq!(observed_control_id, steer_message_id);
    assert_eq!(observed_correlation, steer_correlation);
    assert_eq!(observed_delivery["policy"], "inject");
    assert_eq!(observed_delivery["status"], "delivered");
    let physical_control_rows = std::fs::read_to_string(
        home.spaces_dir()
            .join(&project_id)
            .join("team_messages.jsonl"),
    )
    .expect("read physical TeamMessage rows")
    .lines()
    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    .filter(|message| message["id"].as_str() == Some(steer_message_id.as_str()))
    .collect::<Vec<_>>();
    assert_eq!(
        physical_control_rows.len(),
        1,
        "Steer Control must be published exactly once: {physical_control_rows:?}"
    );
    assert_eq!(
        physical_control_rows[0]["deliveries"][0]["policy"],
        "inject"
    );
    assert_eq!(
        physical_control_rows[0]["deliveries"][0]["status"],
        "delivered"
    );

    let mut converged = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        let handoffs = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| {
                message["from_member_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
            })
            .collect::<Vec<_>>();
        converged = idle
            && handoffs.len() == 1
            && handoffs[0]["id"].as_str() == Some(explicit_handoff_id.as_str());
        if converged {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        converged,
        "same-turn Steer must not append a sibling fallback Handoff"
    );

    let (status, follow_up) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "OPEN NEXT ROUND after idle",
            "correlation_id": conversation_correlation,
            "causation_id": explicit_handoff_id,
        }),
    );
    assert_eq!(status, 200, "body: {follow_up}");
    let follow_up_id = follow_up["result"]["id"]
        .as_str()
        .expect("follow-up id")
        .to_string();

    let completed_before = {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count()
    };
    let mut next_round = false;
    for _ in 0..150 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let follow_up_delivered = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(follow_up_id.as_str()))
            .is_some_and(|message| message["deliveries"][0]["status"] == "delivered");
        let completed_after = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count();
        next_round = follow_up_delivered && completed_after > completed_before;
        if next_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        next_round,
        "ordinary post-idle correlated follow-up must open a new provider round without fabricating a Handoff"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let work = snapshot["works"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|work| work["id"].as_str() == Some(work_id.as_str()))
        .expect("Work in snapshot");
    assert_eq!(
        work["status"].as_str(),
        Some("open"),
        "provider receipt, conversation Handoff, and provider RESULT must not infer Work start/submission/completion: {work}"
    );
}

#[test]
fn codex_app_server_member_interrupt_waits_for_provider_terminal_event() {
    let home = TempHome::new("team-run-codex-interrupt");
    let _project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-interrupt"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &[("PATH", path.as_str())]);
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Codex interruption",
            "members": [{"name": "codex-stop", "role": "observer", "provider": "codex", "execution_mode": "codex_app_server", "initial_work": "Exercise Codex interruption"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut running = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        running = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
            });
        if running {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(running, "Codex app-server member never became live");
    let (status, result) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({"requested_by": "operator", "reason": "stop deterministic turn"}),
    );
    assert_eq!(status, 200, "body: {result}");
    assert_eq!(
        result["result"]["status"].as_str(),
        Some("interrupt_requested")
    );
    let mut idle = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(idle, "Codex interrupt did not stop only the active turn");
    let (status, follow_up) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "continue after interrupt"
        }),
    );
    assert_eq!(status, 200, "body: {follow_up}");
    let mut resumed = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        resumed = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some("thread_fake_codex_app_server")
            });
        if resumed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(resumed, "queued mail did not wake the interrupted Member");
    let (status, steered) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({"content": "finish resumed turn", "requested_by": "host"}),
    );
    assert_eq!(status, 200, "body: {steered}");
    let mut idle_after_resume = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle_after_resume = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle_after_resume {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        idle_after_resume,
        "interrupted Member did not finish a later turn on the same session"
    );
}

#[test]
fn host_can_explicitly_close_a_live_codex_member() {
    let home = TempHome::new("team-run-codex-close");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-close"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let resume_marker = home.base().join("codex-close-resume.log");
    let resume_marker_value = resume_marker.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_RESUME_MARKER", resume_marker_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise explicit Host close",
            "members": [{"name": "codex-close", "role": "observer", "provider": "codex", "initial_work": "Exercise Codex close"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut running = false;
    let mut native_session_id = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        running = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .is_some_and(|member| {
                native_session_id = member["native_session"]["native_session_id"]
                    .as_str()
                    .map(str::to_string);
                member["status"].as_str() == Some("running") && native_session_id.is_some()
            });
        if running {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(running, "Codex member never became live");
    let native_session_id = native_session_id.expect("Codex native session before close");

    let (status, result) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "lane accepted"}),
    );
    assert_eq!(status, 200, "body: {result}");
    assert_eq!(result["result"]["status"].as_str(), Some("close_requested"));
    assert_eq!(
        result["result"]["provider_ack"].as_str(),
        Some("turn_interrupt_accepted")
    );
    let mut stopped = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        stopped = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("stopped")
                    && member["coordination_status"].as_str() == Some("closed")
            });
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(stopped, "Codex member did not terminate after Host close");

    let (status, reopened) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/reopen"),
        &serde_json::json!({"reopened_by": "host", "reason": "continue same conversation"}),
    );
    assert_eq!(status, 202, "body: {reopened}");
    assert_eq!(
        reopened["result"]["member_run"]["id"].as_str(),
        Some(member_id.as_str())
    );
    assert_eq!(
        reopened["result"]["member_run"]["runtime_generation"].as_u64(),
        Some(2)
    );
    assert_eq!(
        reopened["result"]["member_run"]["native_session"]["native_session_id"].as_str(),
        Some(native_session_id.as_str())
    );

    let mut resumed = false;
    for _ in 0..150 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        resumed = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .is_some_and(|member| {
                matches!(member["status"].as_str(), Some("running" | "idle"))
                    && member["coordination_status"].as_str() == Some("active")
                    && member["runtime_generation"].as_u64() == Some(2)
                    && member["native_session"]["native_session_id"].as_str()
                        == Some(native_session_id.as_str())
            });
        if resumed && resume_marker.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(resumed, "reopened Codex member did not run generation 2");
    let resume_log = std::fs::read_to_string(&resume_marker).expect("Codex resume marker");
    assert!(
        resume_log.contains(&native_session_id),
        "reopen did not call thread/resume with the preserved session: {resume_log}"
    );

    let (status, result) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "reopen acceptance complete"}),
    );
    assert_eq!(status, 200, "body: {result}");
}

#[test]
fn codex_provider_reported_interruption_is_not_attributed_to_harness() {
    let home = TempHome::new("team-run-codex-provider-interrupt");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(
        &home.base().join("fakebin-codex-provider-interrupt"),
    );
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_INTERRUPT_WITHOUT_REQUEST", "1"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise provider-reported interruption",
            "members": [{"name": "codex-provider-stop", "role": "observer", "provider": "codex", "execution_mode": "codex_app_server", "initial_work": "Exercise provider interruption"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);

    let mut final_snapshot = serde_json::Value::Null;
    let mut interruption_summary = String::new();
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        interruption_summary = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| action["member_run_id"].as_str() == Some(member_id.as_str()))
            .filter_map(|action| action["summary"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if idle && interruption_summary.contains("without a Harness control request") {
            final_snapshot = snapshot;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(
        final_snapshot,
        serde_json::Value::Null,
        "provider-reported interruption did not return the member to idle with honest attribution"
    );
    assert!(
        interruption_summary.contains("without a Harness control request"),
        "missing honest provider interruption attribution: {interruption_summary}"
    );
    assert!(
        !interruption_summary.contains("operator or Lead interrupted"),
        "provider interruption was falsely attributed to Harness: {interruption_summary}"
    );
}

#[test]
fn kimi_acp_member_can_be_cancelled_cooperatively() {
    let home = TempHome::new("team-run-kimi-cancel");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let cancel_marker = home.base().join("kimi-cancel-marker.log");
    let cancel_marker_value = cancel_marker.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.31.0"),
            ("FAKE_KIMI_WAIT", "1"),
            ("FAKE_KIMI_CANCEL_MARKER", cancel_marker_value.as_str()),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Kimi cancellation",
            "members": [{"name": "kimi-live", "role": "observer", "provider": "kimi", "model": "k2.5", "initial_work": "Exercise Kimi cancellation"}]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    let mut live = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        live = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
            });
        if live {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(live, "Kimi ACP member never became live");
    let (status, interrupted) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({"requested_by": "operator", "reason": "stop this observation"}),
    );
    assert_eq!(status, 200, "body: {interrupted}");
    assert_eq!(
        interrupted["result"]["status"].as_str(),
        Some("cancel_requested")
    );
    let mut idle = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        idle,
        "Kimi ACP interrupt stopped the Member instead of only its turn"
    );
    let cancel_frame = std::fs::read_to_string(&cancel_marker).expect("cancel notification");
    assert!(cancel_frame.contains(r#""method":"session/cancel""#));
    assert!(
        !cancel_frame.contains(r#""id":"#),
        "ACP session/cancel must be a notification without a request id: {cancel_frame}"
    );
}

#[test]
fn host_close_terminates_kimi_0310_runtime_without_conflating_interrupt() {
    let home = TempHome::new("team-run-kimi-0310-close");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let cancel_marker = home.base().join("kimi-close-cancel-marker.log");
    let cancel_marker_value = cancel_marker.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.31.0"),
            ("FAKE_KIMI_WAIT", "1"),
            ("FAKE_KIMI_CANCEL_MARKER", cancel_marker_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Close a Kimi runtime without claiming native cancel",
            "members": [{"name": "kimi-close", "role": "observer", "provider": "kimi", "model": "k2.5", "initial_work": "Exercise Kimi close"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    let mut running = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        running = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
            });
        if running {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(running, "Kimi 0.31.0 member never became live");

    let (status, closed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "lane accepted"}),
    );
    assert_eq!(status, 200, "body: {closed}");
    assert_eq!(
        closed["result"]["provider_ack"].as_str(),
        Some("harness_runtime_termination_requested")
    );
    let mut stopped = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        stopped = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("stopped")
            });
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(stopped, "Host close did not stop Kimi 0.31.0 runtime");
    assert!(
        !cancel_marker.exists(),
        "Host close must terminate the owned runtime directly, not masquerade as Interrupt"
    );
}

#[test]
fn idle_kimi_member_consumes_late_mail_on_the_same_native_session() {
    let home = TempHome::new("team-run-kimi-late-mail");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise persistent Kimi mailbox",
            "members": [{"name": "kimi-idle", "role": "implementer", "provider": "kimi", "initial_work": "Exercise persistent Kimi mailbox"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut first_session = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        first_session = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            })
            .and_then(|member| {
                member["native_session"]["native_session_id"]
                    .as_str()
                    .map(str::to_string)
            });
        let first_round_completed = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            });
        if first_session.is_some() && first_round_completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let first_session = first_session.expect("Kimi idle native session");
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "late Kimi follow-up",
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    assert!(
        sent["result"]["response_intent"].is_null(),
        "bare Host follow-up carries no explicit intent yet still wakes the idle Kimi member \
         via the sender-aware default: {sent}"
    );
    let message_id = sent["result"]["id"].as_str().unwrap().to_string();

    let mut second_round = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let delivered = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(message_id.as_str()))
            .is_some_and(|message| {
                message["deliveries"][0]["status"].as_str() == Some("delivered")
                    && message["deliveries"][0]["attempt"].as_u64() == Some(1)
            });
        let same_session = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some(first_session.as_str())
            });
        let completed_rounds = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count();
        second_round = delivered && same_session && completed_rounds >= 2;
        if second_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        second_round,
        "late Kimi mail was not delivered exactly once on the same native session"
    );
}

#[test]
fn busy_kimi_member_batches_mail_in_order_and_withholds_stale_handoff() {
    let home = TempHome::new("team-run-kimi-busy-mail");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let ready = home.base().join("kimi-first-prompt-ready");
    let release = home.base().join("kimi-first-prompt-release");
    let prompts = home.base().join("kimi-prompts.jsonl");
    let ready_value = ready.display().to_string();
    let release_value = release.display().to_string();
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_FIRST_PROMPT_READY", ready_value.as_str()),
            ("FAKE_KIMI_FIRST_PROMPT_RELEASE", release_value.as_str()),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Kimi safe-boundary batching",
            "members": [{"name": "kimi-busy", "role": "implementer", "provider": "kimi", "initial_work": "Exercise safe-boundary batching"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    for _ in 0..200 {
        if ready.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "first Kimi prompt did not enter busy state");

    let (status, first) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "BUSY_CORRECTION_ONE",
        }),
    );
    assert_eq!(status, 200, "body: {first}");
    let first_id = first["result"]["id"].as_str().unwrap().to_string();
    let correlation = first["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, second) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "BUSY_CORRECTION_TWO",
            "correlation_id": correlation,
            "causation_id": first_id,
        }),
    );
    assert_eq!(status, 200, "body: {second}");
    let second_id = second["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        first["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    assert_eq!(
        second["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    std::fs::write(&release, b"release").expect("release first Kimi prompt");

    let mut accepted = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let messages = snapshot["team_messages"].as_array().unwrap();
        let delivery = |id: &str| {
            messages
                .iter()
                .find(|message| message["id"].as_str() == Some(id))
                .map(|message| &message["deliveries"][0])
        };
        let first_delivery = delivery(&first_id);
        let second_delivery = delivery(&second_id);
        let receipts_match =
            first_delivery
                .zip(second_delivery)
                .is_some_and(|(first_delivery, second_delivery)| {
                    first_delivery["status"].as_str() == Some("delivered")
                        && second_delivery["status"].as_str() == Some("delivered")
                        && first_delivery["attempt"].as_u64() == Some(1)
                        && second_delivery["attempt"].as_u64() == Some(1)
                        && first_delivery["provider_receipt_id"].as_str()
                            == second_delivery["provider_receipt_id"].as_str()
                });
        accepted = receipts_match
            && snapshot["member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|action| {
                    action["member_run_id"].as_str() == Some(member_id.as_str())
                        && action["action_type"].as_str() == Some("turn_completed")
                });
        if accepted {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        accepted,
        "Kimi busy-turn messages were not batched exactly once"
    );
    let prompt_log = std::fs::read_to_string(&prompts).expect("Kimi prompt log");
    let first_position = prompt_log
        .find("BUSY_CORRECTION_ONE")
        .expect("first correction in provider prompt");
    let second_position = prompt_log
        .find("BUSY_CORRECTION_TWO")
        .expect("second correction in provider prompt");
    assert!(
        first_position < second_position,
        "safe-boundary mail order changed"
    );
}

#[test]
fn crashed_kimi_transport_resumes_same_session_without_replaying_work_delivery() {
    let home = TempHome::new("team-run-kimi-crash-recovery");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let crash_once = home.base().join("kimi-crashed-once");
    let attach = home.base().join("kimi-attach.log");
    let prompts = home.base().join("kimi-recovery-prompts.jsonl");
    let crash_value = crash_once.display().to_string();
    let attach_value = attach.display().to_string();
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.31.0"),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_CRASH_ONCE_MARKER", crash_value.as_str()),
            ("FAKE_KIMI_ATTACH_MARKER", attach_value.as_str()),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Recover a Kimi member after provider transport loss",
            "members": [{"name": "kimi-recover", "role": "implementer", "provider": "kimi", "initial_work": "Exercise Kimi recovery"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut recovered = false;
    for _ in 0..400 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let member_idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
                    && member["native_session"]["native_session_id"]
                        .as_str()
                        .is_some_and(|session| session.starts_with("session_fake_"))
            });
        let work_once = snapshot["work_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()))
            .is_some_and(|delivery| {
                delivery["status"].as_str() == Some("provider_received")
                    && delivery["attempt"].as_u64() == Some(1)
            });
        let completed = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            });
        let disconnected = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("disconnected")
            });
        let runtime_recovery = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("runtime_recovery")
            });
        recovered = member_idle && work_once && completed && disconnected && runtime_recovery;
        if recovered {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        recovered,
        "Kimi runtime generation did not recover the accepted Work; snapshot={}; attach={:?}; prompts={:?}",
        serve.get_json("/v1/snapshot").1,
        std::fs::read_to_string(&attach),
        std::fs::read_to_string(&prompts),
    );
    let attach_log = std::fs::read_to_string(&attach).expect("attach log");
    assert!(
        attach_log
            .lines()
            .any(|line| line.starts_with("resume session_fake_")),
        "0.31.0 recovery did not use lightweight session/resume: {attach_log}"
    );
    assert!(
        !attach_log.lines().any(|line| line.starts_with("load ")),
        "0.31.0 unexpectedly replayed native history"
    );
    let prompt_log = std::fs::read_to_string(&prompts).expect("prompt log");
    assert!(
        prompt_log.contains("RUNTIME RECOVERY"),
        "restarted adapter did not ask Kimi to inspect and continue safely"
    );
}

#[test]
fn codex_app_server_question_routes_to_lead_and_resumes_same_turn() {
    let home = TempHome::new("team-run-codex-question");
    let _project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-question"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("PATH", path.as_str()), ("FAKE_CODEX_ASK", "1")],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Codex reverse input",
            "members": [{"name": "codex-question", "role": "implementer", "provider": "codex", "execution_mode": "codex_app_server", "initial_work": "Exercise provider question routing"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut interaction_id = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        interaction_id = snapshot["pending_interactions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|interaction| {
                interaction["member_run_id"].as_str() == Some(member_id.as_str())
                    && interaction["status"].as_str() == Some("pending")
            })
            .and_then(|interaction| interaction["id"].as_str().map(str::to_string));
        if interaction_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let interaction_id = interaction_id.expect("Codex PendingInteraction");
    let (status, resolved) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/interactions/{interaction_id}/resolve"),
        &serde_json::json!({"option_id": "implementation::0", "resolved_by": "host"}),
    );
    assert_eq!(status, 200, "body: {resolved}");
    assert_eq!(resolved["result"]["status"].as_str(), Some("answered"));
    let mut idle = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        idle,
        "Codex did not resume after Lead answer and return idle"
    );
}

#[test]
fn interrupt_cancels_pending_interaction_before_kimi_prompt() {
    let home = TempHome::new("team-run-kimi-waiting-cancel");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.31.0"),
            ("FAKE_KIMI_ASK", "1"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Wait for Lead, then be interrupted",
            "members": [{"name": "kimi-waiting", "role": "observer", "provider": "kimi", "model": "k2.5", "initial_work": "Exercise pending interaction cancellation"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut waiting = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        waiting = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("waiting")
            });
        if waiting {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        waiting,
        "Kimi never entered provider-interaction waiting state"
    );
    let (status, interrupted) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({"reason": "cancel while waiting", "requested_by": "operator"}),
    );
    assert_eq!(status, 200, "body: {interrupted}");
    let mut idle_with_cancelled_interaction = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        let cancelled = snapshot["pending_interactions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|interaction| {
                interaction["member_run_id"].as_str() == Some(member_id.as_str())
                    && interaction["status"].as_str() == Some("cancelled")
            });
        idle_with_cancelled_interaction = idle && cancelled;
        if idle_with_cancelled_interaction {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        idle_with_cancelled_interaction,
        "interrupt did not cancel the waiting interaction and return the Member to idle"
    );
}

#[test]
fn post_team_run_transition_and_compatibility_lineage() {
    let home = TempHome::new("team-run-transition");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Unlinked compatibility attempt 1 (planning).
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Compatibility attempt one",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let wave1_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();

    // Unlinked legacy runs retain previous_run_id as compatibility lineage.
    // Native Mission/Wave attempts are covered separately and only link retries
    // inside one Wave.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Compatibility attempt two",
            "previous_run_id": wave1_id,
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let runs = snapshot["team_runs"].as_array().expect("team_runs");
    assert_eq!(
        runs.iter()
            .find(|run| run["objective"].as_str() == Some("Compatibility attempt two"))
            .and_then(|run| run["previous_run_id"].as_str()),
        Some(wave1_id.as_str()),
        "compatibility attempt lineage: {runs:?}"
    );

    // An unknown previous run id is rejected, nothing journaled.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Dangling compatibility attempt",
            "previous_run_id": "team-run-nope",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(false), "body: {body}");

    // Illegal attempt move: planning → completed; an attempt must reach
    // reviewing before it can become completion-eligible for a Wave gate.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave1_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("invalid team-run transition"),
        "body: {body}"
    );

    // Legal: planning → cancelled, folded into the run row + event log.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave1_id}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("cancelled"));
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let runs = snapshot["team_runs"].as_array().expect("team_runs");
    assert_eq!(
        runs.iter()
            .find(|run| run["id"].as_str() == Some(wave1_id.as_str()))
            .and_then(|run| run["status"].as_str()),
        Some("cancelled"),
        "latest-wins projection shows the cancellation: {runs:?}"
    );
    let events = snapshot["team_run_events"]
        .as_array()
        .expect("team_run_events");
    assert!(
        events.iter().any(|event| {
            event["entity_id"].as_str() == Some(wave1_id.as_str())
                && event["operation"].as_str() == Some("updated")
                && event["summary"]
                    .as_str()
                    .unwrap_or("")
                    .contains("cancelled")
        }),
        "a cancellation event was folded: {events:?}"
    );

    // A terminal run cannot transition anywhere.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave1_id}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 400, "body: {body}");

    // Flip compatibility attempt 2 to reviewing by appending the row directly
    // (the store is an append-only latest-wins ledger), then complete it.
    let wave2_id = runs
        .iter()
        .find(|run| run["objective"].as_str() == Some("Compatibility attempt two"))
        .and_then(|run| run["id"].as_str())
        .expect("wave 2 id")
        .to_string();
    let store_root = home.spaces_dir().join(&project_id);
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(store_root.join("team_runs.jsonl"))
        .expect("open team_runs.jsonl");
    use std::io::Write as _;
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": wave2_id,
            "host_surface": "http",
            "objective": "Compatibility attempt two",
            "status": "reviewing",
            "previous_run_id": wave1_id,
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:2",
        })
    )
    .expect("append reviewing row");

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave2_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("completed"));
    assert!(
        body["result"]["completed_at"].as_str().is_some(),
        "completed_at stamped on attempt completion: {body:?}"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let events = snapshot["team_run_events"]
        .as_array()
        .expect("team_run_events");
    assert!(
        events.iter().any(|event| {
            event["entity_id"].as_str() == Some(wave2_id.as_str())
                && event["operation"].as_str() == Some("completed")
                && event["summary"]
                    .as_str()
                    .unwrap_or("")
                    .contains("team-run attempt completed")
        }),
        "the attempt-completion event was folded: {events:?}"
    );

    // The CLI arms share the same lifecycle: completing an already-completed run is
    // a usage error, and cancelling a planning run succeeds.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "complete",
            "--id",
            &wave2_id,
        ],
    );
    assert!(
        !out.status.success(),
        "completing a completed run must fail: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("invalid team-run transition"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Compatibility attempt three",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let wave3_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let cancelled = team_run_json(&home, &project_id, &["cancel", "--id", &wave3_id, "--json"]);
    assert_eq!(cancelled["status"].as_str(), Some("cancelled"));

    // A status-only cancellation must not lie about stopping active provider
    // work. Until cooperative interruption exists, running -> cancelled is
    // rejected by the shared CLI/HTTP transition contract.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Active compatibility attempt",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let running_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("running attempt id")
        .to_string();
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(store_root.join("team_runs.jsonl"))
        .expect("open team_runs.jsonl");
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": running_id,
            "host_surface": "http",
            "objective": "Active compatibility attempt",
            "status": "running",
            "created_at": "unix-ms:3",
            "updated_at": "unix-ms:4",
        })
    )
    .expect("append running row");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{running_id}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("running cancellation requires provider interruption"),
        "body: {body}"
    );

    // Once an operator has independently stopped every provider process, the
    // explicit recovery path terminates the stale attempt and its members with
    // an auditable reason. It is deliberately separate from status-only cancel.
    let recovered = team_run_json(
        &home,
        &project_id,
        &[
            "cancel",
            "--id",
            &running_id,
            "--confirm-provider-stopped",
            "--reason",
            "foreground orchestrator was interrupted",
            "--cancelled-by",
            "test-operator",
            "--json",
        ],
    );
    assert_eq!(recovered["status"].as_str(), Some("cancelled"));
    let recovered_status = team_run_json(
        &home,
        &project_id,
        &["status", "--id", &running_id, "--json"],
    );
    assert_eq!(
        recovered_status["members"][0]["member_run"]["status"].as_str(),
        Some("stopped")
    );
    assert_eq!(
        recovered_status["members"][0]["latest_action"]["action_type"].as_str(),
        Some("interrupted")
    );
    assert_eq!(
        recovered_status["members"][0]["latest_action"]["status"].as_str(),
        Some("cancelled")
    );
}

#[test]
fn sse_streams_team_run_events() {
    let home = TempHome::new("team-run-sse");
    let project_id = init_project(&home, "alpha");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let mut sse = serve.open_sse("");

    // Create a run AFTER the stream is live: the watcher tails
    // team_run_events.jsonl and broadcasts each folded event.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Stream me",
            "--member",
            "solo:worker:kimi",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // Native row frames are now multiplexed alongside the folded event rows,
    // so collect the complete create burst rather than stopping after the first
    // three typed projections.
    let frames = collect_sse_data(&mut sse, Duration::from_secs(6), 6);
    assert!(
        frames.iter().any(|frame| {
            frame["entity_type"].as_str() == Some("team_run")
                && frame["operation"].as_str() == Some("created")
                && frame["team_run_id"].as_str() == Some(run_id.as_str())
        }),
        "expected a team_run created frame for {run_id}; got: {frames:?}"
    );
}

#[test]
fn two_peer_ack_only_mail_converges_without_extra_rounds_and_batches_on_next_trigger() {
    let home = TempHome::new("team-run-two-peer-convergence");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let prompts = home.base().join("kimi-prompts.jsonl");
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
            // Keep idle members inside their wake loop for the whole scenario;
            // the default 250ms test grace would retire them before the later
            // response-required triggers arrive.
            ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Two-peer bounded convergence on acknowledgement-only mail",
            "members": [
                {"name": "peer-a", "role": "implementer", "provider": "kimi",
                 "initial_work": "Complete peer A lane"},
                {"name": "peer-b", "role": "reviewer", "provider": "kimi",
                 "initial_work": "Complete peer B lane"}
            ]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_a = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_b = created["result"]["member_runs"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let snapshot_messages = |serve: &ServeHandle| -> Vec<serde_json::Value> {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["team_messages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    let member_status = |serve: &ServeHandle, member_id: &str| -> Option<String> {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id))
            .and_then(|member| member["status"].as_str().map(str::to_string))
    };
    let completed_rounds = |serve: &ServeHandle, member_id: &str| -> usize {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id)
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count()
    };
    let follow_up_rounds = |prompts: &std::path::Path| -> usize {
        std::fs::read_to_string(prompts)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("FOLLOW-UP MESSAGES"))
            .count()
    };

    let mut round_one = false;
    for _ in 0..300 {
        round_one = completed_rounds(&serve, &member_a) >= 1
            && completed_rounds(&serve, &member_b) >= 1
            && member_status(&serve, &member_a).as_deref() == Some("idle")
            && member_status(&serve, &member_b).as_deref() == Some("idle");
        if round_one {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(round_one, "both peers must finish round one and go idle");

    // Ack-only PEER mail must NOT wake an idle peer into a provider round
    // (ADR 0046 §4); the delivery stays durable and queued. This is the
    // sender-aware default: no explicit intent is set on the wire, only
    // explicit member provenance.
    let (status, fyi) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_kind": "member_run",
            "sender_id": member_b,
            "from_member_id": member_b,
            "to_member_ids": [member_a],
            "kind": "message",
            "body": "ACK: your lane note landed; no reply needed",
        }),
    );
    assert_eq!(status, 200, "body: {fyi}");
    let fyi_id = fyi["result"]["id"].as_str().unwrap().to_string();
    let correlation_a = fyi["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(fyi["result"]["response_intent"].is_null());

    // Host mail is response-required by DEFAULT (Host questions, revisions,
    // and acceptance decisions all ride on `message`), so an FYI-only Host
    // note must say so explicitly. That explicit override is also non-waking.
    let (status, host_fyi) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_a],
            "kind": "message",
            "response_intent": "informational",
            "body": "FYI: the wave advanced; no reply needed",
            "correlation_id": correlation_a,
        }),
    );
    assert_eq!(status, 200, "body: {host_fyi}");
    let host_fyi_id = host_fyi["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        host_fyi["result"]["response_intent"].as_str(),
        Some("informational")
    );

    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        follow_up_rounds(&prompts),
        0,
        "informational mail must not start a provider round: {}",
        std::fs::read_to_string(&prompts).unwrap_or_default()
    );
    assert_eq!(
        member_status(&serve, &member_a).as_deref(),
        Some("idle"),
        "informational mail must not even mark the member busy"
    );
    for queued_id in [&fyi_id, &host_fyi_id] {
        let delivery = snapshot_messages(&serve)
            .into_iter()
            .find(|message| message["id"].as_str() == Some(queued_id.as_str()))
            .and_then(|message| message["deliveries"][0].clone().into());
        let delivery: serde_json::Value = delivery.expect("informational delivery row");
        assert_eq!(delivery["status"].as_str(), Some("queued"), "{queued_id}");
        assert_eq!(delivery["attempt"].as_u64(), Some(0), "{queued_id}");
    }

    // A response-required question wakes peer A. During that round the
    // scripted provider answers with acknowledgement-only mail to peer B.
    let (status, question) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_a],
            "kind": "message",
            "response_intent": "response_required",
            "body": "QUESTION: confirm your lane state",
            "correlation_id": correlation_a,
        }),
    );
    assert_eq!(status, 200, "body: {question}");
    let question_id = question["result"]["id"].as_str().unwrap().to_string();

    let mut a_second_round = false;
    for _ in 0..300 {
        let messages = snapshot_messages(&serve);
        let question_delivered = messages
            .iter()
            .find(|message| message["id"].as_str() == Some(question_id.as_str()))
            .is_some_and(|message| message["deliveries"][0]["status"] == "delivered");
        a_second_round = question_delivered
            && completed_rounds(&serve, &member_a) >= 2
            && member_status(&serve, &member_a).as_deref() == Some("idle");
        if a_second_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        a_second_round,
        "response-required question must drive exactly one follow-up round on peer A"
    );
    let (status, peer_ack) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_kind": "member_run",
            "sender_id": member_a,
            "from_member_id": member_a,
            "to_member_ids": [member_b],
            "kind": "message",
            "response_intent": "informational",
            "body": "ACK: noted, no reply needed",
            "correlation_id": correlation_a,
            "causation_id": question_id,
        }),
    );
    assert_eq!(status, 200, "body: {peer_ack}");
    // Both earlier informational notes (the bare peer ack and the explicitly
    // informational Host FYI) rode along with the triggered round and were
    // delivered exactly once with that round's receipt.
    let messages = snapshot_messages(&serve);
    for queued_id in [&fyi_id, &host_fyi_id] {
        let delivery = messages
            .iter()
            .find(|message| message["id"].as_str() == Some(queued_id.as_str()))
            .map(|message| message["deliveries"][0].clone())
            .expect("informational delivery");
        assert_eq!(
            delivery["status"].as_str(),
            Some("delivered"),
            "{queued_id}"
        );
        assert_eq!(delivery["attempt"].as_u64(), Some(1), "{queued_id}");
        assert!(
            delivery["provider_receipt_id"]
                .as_str()
                .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:")),
            "{queued_id}"
        );
    }

    // Bounded convergence: peer B must NOT start a round for the ack-only
    // mail. Wait long enough for any erroneous round to begin.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        follow_up_rounds(&prompts),
        1,
        "ack-only peer mail must not trigger another provider round: {}",
        std::fs::read_to_string(&prompts).unwrap_or_default()
    );
    let messages = snapshot_messages(&serve);
    let ack_message = messages
        .iter()
        .find(|message| {
            message["from_member_id"].as_str() == Some(member_a.as_str())
                && message["to_member_ids"][0].as_str() == Some(member_b.as_str())
        })
        .expect("peer ack message")
        .clone();
    assert_eq!(
        ack_message["deliveries"][0]["status"].as_str(),
        Some("queued"),
        "ack-only mail stays durable and queued without a round"
    );
    assert_eq!(member_status(&serve, &member_b).as_deref(), Some("idle"));

    // An ordinary Host message now triggers peer B on the sender-aware
    // default alone (no explicit intent on the wire); the queued ack batches
    // into that round and both are delivered exactly once with the same
    // provider receipt.
    let (status, b_trigger) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_b],
            "kind": "message",
            "body": "Start your reviewed lane now",
        }),
    );
    assert_eq!(status, 200, "body: {b_trigger}");
    let b_trigger_id = b_trigger["result"]["id"].as_str().unwrap().to_string();
    let mut b_second_round = false;
    for _ in 0..300 {
        let messages = snapshot_messages(&serve);
        let trigger_delivered = messages
            .iter()
            .find(|message| message["id"].as_str() == Some(b_trigger_id.as_str()))
            .is_some_and(|message| message["deliveries"][0]["status"] == "delivered");
        b_second_round = trigger_delivered && completed_rounds(&serve, &member_b) >= 2;
        if b_second_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        b_second_round,
        "ordinary Host mail must drive peer B's follow-up round on the sender-aware default"
    );
    let messages = snapshot_messages(&serve);
    let delivery_of = |message_id: &str| -> serde_json::Value {
        messages
            .iter()
            .find(|message| message["id"].as_str() == Some(message_id))
            .map(|message| message["deliveries"][0].clone())
            .expect("delivery row")
    };
    let ack_delivery = delivery_of(ack_message["id"].as_str().unwrap());
    let trigger_delivery = delivery_of(&b_trigger_id);
    assert_eq!(ack_delivery["status"].as_str(), Some("delivered"));
    assert_eq!(ack_delivery["attempt"].as_u64(), Some(1));
    assert_eq!(trigger_delivery["status"].as_str(), Some("delivered"));
    assert_eq!(trigger_delivery["attempt"].as_u64(), Some(1));
    assert_eq!(
        ack_delivery["provider_receipt_id"].as_str(),
        trigger_delivery["provider_receipt_id"].as_str(),
        "queued informational mail batches into the triggered round"
    );
    // Exactly two follow-up rounds happened in the whole team (A then B):
    // convergence is bounded, no acknowledgement ping-pong.
    assert_eq!(follow_up_rounds(&prompts), 2);
    let prompt_log = std::fs::read_to_string(&prompts).expect("prompt log");
    let b_round_line = prompt_log
        .lines()
        .filter(|line| line.contains("FOLLOW-UP MESSAGES"))
        .find(|line| line.contains("Start your reviewed lane now"))
        .expect("peer B follow-up prompt");
    let ack_position = b_round_line
        .find("ACK: noted, no reply needed")
        .expect("ack batched first");
    let trigger_position = b_round_line
        .find("Start your reviewed lane now")
        .expect("trigger batched second");
    assert!(
        ack_position < trigger_position,
        "batched mail preserves append order: {b_round_line}"
    );
}

#[test]
fn kimi_provider_error_round_records_failure_without_fabricated_handoff_and_recovers() {
    let home = TempHome::new("team-run-kimi-provider-error");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let error_once = home.base().join("kimi-prompt-error-once");
    let error_once_value = error_once.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            (
                "FAKE_KIMI_PROMPT_ERROR_ONCE_MARKER",
                error_once_value.as_str(),
            ),
            ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Kimi provider failure parity",
            "members": [{"name": "kimi-fail", "role": "implementer", "provider": "kimi", "initial_work": "Exercise provider failure parity"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut provider_error_recorded = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let messages = snapshot["team_messages"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let handoffs = messages
            .iter()
            .filter(|message| {
                message["from_member_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
            })
            .count();
        let provider_error = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("provider_error")
                    && action["status"].as_str() == Some("failed")
            });
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        assert_eq!(
            handoffs, 0,
            "a provider-failed turn must never fabricate a handoff"
        );
        provider_error_recorded = provider_error && idle;
        if provider_error_recorded {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        provider_error_recorded,
        "non-retryable Kimi provider failure must record a failed provider_error round and stay idle"
    );
    assert!(error_once.exists(), "the scripted provider error fired");
    // The provider did accept the prompt before failing the turn: the
    // Work delivery keeps its honest receipt and is never replayed.
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let work_delivery = snapshot["work_deliveries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()))
        .cloned()
        .expect("Work delivery");
    assert_eq!(work_delivery["status"].as_str(), Some("provider_received"));
    assert_eq!(work_delivery["attempt"].as_u64(), Some(1));
    assert!(work_delivery["provider_receipt_id"]
        .as_str()
        .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:")));

    // The member stays usable: the next response-required message runs a new
    // round on the same member without fabricating a Handoff message.
    let (status, follow_up) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "response_intent": "response_required",
            "body": "Retry the lane after the provider outage",
            "work_id": work_id,
        }),
    );
    assert_eq!(status, 200, "body: {follow_up}");
    let follow_up_id = follow_up["result"]["id"].as_str().unwrap().to_string();
    let mut recovered = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let completed = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
                    && action["status"].as_str() == Some("succeeded")
            });
        let delivered = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(follow_up_id.as_str()))
            .is_some_and(|message| message["deliveries"][0]["status"] == "delivered");
        recovered = delivered && completed;
        if recovered {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        recovered,
        "the recovered round must consume the retry conversation without fabricating Handoff"
    );
}

/// A prompt the provider rejects BEFORE any session/update was never accepted.
/// Publishing a provider receipt for it would complete the Work delivery and
/// burn the work before the provider accepted responsibility.
#[test]
fn kimi_prompt_rejected_before_any_update_never_burns_the_work() {
    let home = TempHome::new("team-run-kimi-reject-before-update");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let reject_once = home.base().join("kimi-reject-before-update-once");
    let reject_once_value = reject_once.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            (
                "FAKE_KIMI_REJECT_BEFORE_UPDATE_MARKER",
                reject_once_value.as_str(),
            ),
            ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Kimi immediate rejection must not burn the Work",
            "members": [{"name": "kimi-reject", "role": "implementer", "provider": "kimi", "initial_work": "Exercise immediate rejection recovery"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut rejected = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let provider_error = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("provider_error")
                    && action["status"].as_str() == Some("failed")
            });
        let handoffs = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| {
                message["from_member_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
            })
            .count();
        assert_eq!(
            handoffs, 0,
            "a rejected prompt must never fabricate a handoff"
        );
        rejected = provider_error;
        if rejected {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        rejected,
        "an immediately rejected Kimi prompt must record a failed provider_error round"
    );
    assert!(reject_once.exists(), "the scripted rejection fired");

    // The core contract: no receipt was published for a turn the provider
    // never accepted, so the Work delivery is not completed and stays
    // replayable rather than silently burned.
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let work_delivery = snapshot["work_deliveries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()))
        .cloned()
        .expect("Work delivery");
    assert_ne!(
        work_delivery["status"].as_str(),
        Some("provider_received"),
        "a rejected prompt must not complete the Work delivery: {work_delivery}"
    );
    assert!(
        work_delivery["provider_receipt_id"].is_null(),
        "a rejected prompt must publish no provider receipt: {work_delivery}"
    );
    assert!(
        !snapshot["team_run_events"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|event| {
                event["entity_id"].as_str() == Some(work_id.as_str())
                    && event["summary"]
                        .as_str()
                        .is_some_and(|summary| summary.contains("accepted by provider"))
            }),
        "a rejected prompt must not journal `Work accepted by provider`: {}",
        snapshot["team_run_events"]
    );
}

/// JSON-RPC servers that serialize every field return `"error": null` on
/// success. `frame.get("error").is_some()` is true for that key, so a naive
/// check turns every successful round into a provider failure and loses the
/// member's entire output.
#[test]
fn kimi_null_error_key_on_a_successful_response_is_not_a_provider_error() {
    let home = TempHome::new("team-run-kimi-null-error-key");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_NULL_ERROR_KEY", "1"),
            ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Null error key is still a successful round",
            "members": [{"name": "kimi-null-error", "role": "implementer", "provider": "kimi", "initial_work": "Exercise null error response"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut completed = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        assert!(
            !snapshot["member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|action| {
                    action["member_run_id"].as_str() == Some(member_id.as_str())
                        && action["action_type"].as_str() == Some("provider_error")
                }),
            "`error: null` is an empty key, not a provider failure"
        );
        completed = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
                    && action["status"].as_str() == Some("succeeded")
            });
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        completed,
        "a successful round carrying `error: null` must record a successful provider turn"
    );
}

/// `max_tokens`, `refusal`, and `max_turn_requests` all stop the turn before
/// the provider finished its turn successfully. Recording them as
/// turn_completed/succeeded would conflate transport termination with a valid
/// terminal round, so they must record a failed provider round instead. Work
/// state remains owned exclusively by explicit Work operations in either case.
#[test]
fn kimi_incomplete_stop_reason_records_failure_without_a_fabricated_handoff() {
    for stop_reason in ["max_tokens", "refusal", "max_turn_requests"] {
        let home = TempHome::new(&format!("team-run-kimi-stop-{stop_reason}"));
        let _project_id = init_project(&home, "alpha");
        let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
        let fake_kimi = fake_bin.join("kimi").display().to_string();
        let serve = ServeHandle::spawn_with_env(
            &home,
            home.base(),
            &[],
            &[
                ("KIMI_CODE_BIN", fake_kimi.as_str()),
                ("FAKE_KIMI_RESULT", "done"),
                ("FAKE_KIMI_STOP_REASON", stop_reason),
                ("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
            ],
        );
        let (_, created) = serve.post_json(
            "/v1/team-runs",
            &serde_json::json!({
                "objective": format!("Kimi {stop_reason} must not read as success"),
                "members": [{"name": "kimi-stop", "role": "implementer", "provider": "kimi", "initial_work": "Exercise incomplete stop reason"}]
            }),
        );
        let run_id = created["result"]["team_run"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let member_id = created["result"]["member_runs"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let (status, started) = serve.post_json(
            &format!("/v1/team-runs/{run_id}/start"),
            &serde_json::json!({}),
        );
        assert_eq!(status, 202, "body: {started}");

        let mut failed = false;
        for _ in 0..300 {
            let (_, snapshot) = serve.get_json("/v1/snapshot");
            let actions: Vec<&serde_json::Value> = snapshot["member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|action| action["member_run_id"].as_str() == Some(member_id.as_str()))
                .collect();
            assert!(
                !actions.iter().any(|action| {
                    action["action_type"].as_str() == Some("turn_completed")
                        && action["status"].as_str() == Some("succeeded")
                }),
                "{stop_reason} must never be recorded as a succeeded completion"
            );
            let handoffs = snapshot["team_messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|message| {
                    message["from_member_id"].as_str() == Some(member_id.as_str())
                        && message["kind"].as_str() == Some("handoff")
                })
                .count();
            assert_eq!(handoffs, 0, "{stop_reason} must never fabricate a handoff");
            failed = actions.iter().any(|action| {
                action["action_type"].as_str() == Some("provider_error")
                    && action["status"].as_str() == Some("failed")
            });
            if failed {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            failed,
            "stopReason {stop_reason} must record a failed provider round"
        );
    }
}

#[test]
fn external_interactive_member_joins_and_exchanges_mail() {
    let home = TempHome::new("team-run-external-interactive");
    let project_id = init_project(&home, "alpha");

    // A declared external interactive member may use an arbitrary provider
    // label because Harness never executes it or claims adapter capability.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "custom external provider",
            "--member",
            "custom-reviewer:reviewer:local-agent/external_interactive",
        ],
    );
    assert!(
        out.status.success(),
        "custom external provider must be accepted: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let custom_run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let custom = team_run_json(
        &home,
        &project_id,
        &["status", "--id", &custom_run_id, "--json"],
    );
    assert_eq!(
        custom["members"][0]["member_run"]["provider"],
        "local-agent"
    );
    assert_eq!(
        custom["members"][0]["member_run"]["provider_profile"]["execution_driver"],
        "user_driven"
    );

    // Create a run whose only member is the user's own external interactive
    // session; Harness spawns nothing for it.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Review the external lane",
            "--member",
            "ext-reviewer:reviewer:kimi/external_interactive#Review the external lane",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    let members = status["members"].as_array().expect("members");
    assert_eq!(members.len(), 1, "members: {members:?}");
    let ext = &members[0]["member_run"];
    let ext_id = ext["id"].as_str().expect("external member id").to_string();
    assert_eq!(ext["status"].as_str(), Some("idle"));
    assert_eq!(
        ext["provider_profile"]["execution_mode"].as_str(),
        Some("external_interactive")
    );
    assert_eq!(
        ext["provider_profile"]["execution_driver"].as_str(),
        Some("user_driven")
    );
    assert!(
        ext["native_session"].is_null(),
        "external members have no native session record: {ext}"
    );
    assert!(
        ext["workspace_snapshot"].is_null(),
        "external members get no Harness workspace snapshot: {ext}"
    );

    // add-member accepts the same mode on an active run and records optional
    // initial Work without duplicating ownership into chat.
    let added = team_run_json(
        &home,
        &project_id,
        &[
            "add-member",
            "--id",
            &run_id,
            "--member",
            "ext-helper:helper:codex/external_interactive",
            "--initial-work",
            "Pair on the review",
        ],
    );
    let helper_id = added["member_run"]["id"]
        .as_str()
        .expect("helper member id")
        .to_string();
    assert_eq!(
        added["member_run"]["provider_profile"]["execution_mode"].as_str(),
        Some("external_interactive")
    );
    assert_eq!(
        added["work"]["active_member_run_id"].as_str(),
        Some(helper_id.as_str()),
        "initial Work: {added}"
    );

    // The Supervisor starts the run without spawning an adapter for external
    // members: no adapter error, no Failed status, and start returns promptly
    // because there is nothing to drive.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
    );
    assert!(
        out.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        !stdout.contains("adapter not implemented"),
        "start output: {stdout}"
    );
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(status["team_run"]["status"].as_str(), Some("running"));
    for entry in status["members"].as_array().expect("members") {
        let member_status = entry["member_run"]["status"]
            .as_str()
            .expect("member status");
        assert!(
            !matches!(member_status, "failed" | "disconnected"),
            "external member must not be marked {member_status}: {entry}"
        );
    }

    // Host → external member: the delivery stays queued until the session
    // polls its inbox itself.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            &ext_id,
            "--kind",
            "message",
            "--body",
            "Please review crates/harness-core",
        ],
    );
    assert!(
        out.status.success(),
        "host send failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let host_message_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let all_mail = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--all",
            "--json",
        ],
    );
    let correlation = all_mail
        .as_array()
        .expect("external inbox history")
        .iter()
        .find(|message| message["id"].as_str() == Some(host_message_id.as_str()))
        .and_then(|message| message["correlation_id"].as_str())
        .expect("conversation correlation")
        .to_string();

    // The external session polls ordinary mail and acks what it consumed.
    let inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--json",
        ],
    );
    let inbox_ids: Vec<&str> = inbox
        .as_array()
        .expect("inbox")
        .iter()
        .filter_map(|message| message["id"].as_str())
        .collect();
    assert_eq!(inbox_ids, vec![host_message_id.as_str()]);
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "ack",
            "--id",
            &run_id,
            "--member-id",
            &ext_id,
            "--message-id",
            &host_message_id,
        ],
    );
    assert!(
        out.status.success(),
        "external ack failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--json",
        ],
    );
    assert_eq!(
        inbox.as_array().map(Vec::len),
        Some(0),
        "acked mail leaves the actionable inbox: {inbox}"
    );

    // External member → Host reply keeps the conversation correlation and names
    // its direct cause.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            &ext_id,
            "--to",
            "host",
            "--kind",
            "message",
            "--body",
            "Review done: no defects found",
            "--correlation-id",
            &correlation,
            "--causation-id",
            &host_message_id,
        ],
    );
    assert!(
        out.status.success(),
        "external reply failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let reply_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let host_inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--json",
        ],
    );
    let reply = host_inbox
        .as_array()
        .expect("host inbox")
        .iter()
        .find(|message| message["id"].as_str() == Some(reply_id.as_str()))
        .expect("reply in host inbox");
    assert_eq!(reply["from_member_id"].as_str(), Some(ext_id.as_str()));
    assert_eq!(reply["correlation_id"].as_str(), Some(correlation.as_str()));
    assert_eq!(
        reply["causation_id"].as_str(),
        Some(host_message_id.as_str())
    );

    // Closing an external member freezes its Harness coordination; there is
    // no provider runtime or native session under Harness control to clean up.
    let closed = team_run_json(
        &home,
        &project_id,
        &[
            "close-member",
            "--id",
            &run_id,
            "--member-run-id",
            &helper_id,
            "--reason",
            "review pair no longer needed",
        ],
    );
    assert_eq!(
        closed["status"].as_str(),
        Some("stopped"),
        "close: {closed}"
    );
    assert_eq!(closed["runtime"].as_str(), Some("external_unmanaged"));
    assert_eq!(closed["runtime_effect"].as_str(), Some("none"));
    assert_eq!(
        closed["coordination_effect"].as_str(),
        Some("member_closed")
    );
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let helper = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .rev()
        .find(|member| member.id == helper_id)
        .expect("helper member row");
    assert_eq!(helper.status, harness_core::MemberRunStatus::Stopped);
    assert_eq!(
        helper.coordination_status,
        harness_core::MemberCoordinationStatus::Closed
    );
    assert!(
        store
            .latest_team_member_close_request(&helper_id)
            .expect("close request")
            .is_some_and(|close| close.status == harness_core::TeamMemberCloseStatus::Applied),
        "close request must be applied without a supervisor"
    );

    // An external-only TeamRun remains Host-controlled: after a correlated
    // Handoff the Host may close the coordination binding and explicitly
    // complete the run without claiming that any external process was stopped.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            &ext_id,
            "--to",
            "host",
            "--kind",
            "handoff",
            "--body",
            "External review handoff: checks reported by the user-driven member",
            "--correlation-id",
            &correlation,
            "--causation-id",
            &host_message_id,
        ],
    );
    assert!(
        out.status.success(),
        "external handoff failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Leave one message queued so Close can prove that the frozen coordination
    // binding cannot send, receive, or ACK until explicit Reopen.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            &ext_id,
            "--kind",
            "message",
            "--body",
            "Queued before coordination close",
            "--correlation-id",
            &correlation,
        ],
    );
    assert!(
        out.status.success(),
        "pre-close send failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let queued_before_close_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let reviewer_closed = team_run_json(
        &home,
        &project_id,
        &[
            "close-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--reason",
            "Host accepted external review",
        ],
    );
    assert_eq!(reviewer_closed["runtime_effect"].as_str(), Some("none"));

    for args in [
        vec![
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            &ext_id,
            "--to",
            "host",
            "--kind",
            "message",
            "--body",
            "must not send after close",
            "--correlation-id",
            &correlation,
        ],
        vec![
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            &ext_id,
            "--kind",
            "message",
            "--body",
            "must not queue after close",
            "--correlation-id",
            &correlation,
        ],
        vec![
            "--project",
            &project_id,
            "team-run",
            "ack",
            "--id",
            &run_id,
            "--member-id",
            &ext_id,
            "--message-id",
            &queued_before_close_id,
        ],
    ] {
        let out = run_harness(&home, home.base(), &args);
        assert!(
            !out.status.success()
                && String::from_utf8_lossy(&out.stderr).contains("coordination is closed"),
            "closed external coordination must reject {args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let reopened = team_run_json(
        &home,
        &project_id,
        &[
            "reopen-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--reason",
            "continue the same external review",
        ],
    );
    assert_eq!(reopened["member_run"]["id"].as_str(), Some(ext_id.as_str()));
    assert_eq!(
        reopened["member_run"]["coordination_status"].as_str(),
        Some("active")
    );
    assert_eq!(
        reopened["member_run"]["runtime_generation"].as_u64(),
        Some(2)
    );
    assert_eq!(
        reopened["runtime_activation"].as_str(),
        Some("external_user_driven")
    );
    let reopened_inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--json",
        ],
    );
    assert!(
        reopened_inbox.as_array().is_some_and(|messages| messages
            .iter()
            .any(|message| { message["id"].as_str() == Some(queued_before_close_id.as_str()) })),
        "mail queued before close must thaw after reopen: {reopened_inbox}"
    );
    let ack = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "ack",
            "--id",
            &run_id,
            "--member-id",
            &ext_id,
            "--message-id",
            &queued_before_close_id,
        ],
    );
    assert!(
        ack.status.success(),
        "reopened external member must ACK frozen mail: {}",
        String::from_utf8_lossy(&ack.stderr)
    );

    let retired = team_run_json(
        &home,
        &project_id,
        &[
            "deactivate-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--reason",
            "external reviewer retired",
        ],
    );
    assert_eq!(retired["coordination_status"].as_str(), Some("retired"));
    let reopen_retired = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "reopen-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
        ],
    );
    assert!(
        !reopen_retired.status.success()
            && String::from_utf8_lossy(&reopen_retired.stderr).contains("is retired"),
        "retired member must not reopen: {}",
        String::from_utf8_lossy(&reopen_retired.stderr)
    );

    // TeamRun completion is not Work acceptance. This scenario exercised
    // external coordination only, so the Host explicitly cancels its two
    // untouched Works before ending the run.
    let works = team_run_json(
        &home,
        &project_id,
        &["work", "list", "--team-run-id", &run_id],
    );
    for work in works.as_array().expect("Work list") {
        let work_id = work["id"].as_str().expect("Work id");
        let version = work["version"].as_u64().expect("Work version").to_string();
        team_run_json(
            &home,
            &project_id,
            &[
                "work",
                "cancel",
                "--work-id",
                work_id,
                "--expected-version",
                &version,
                "--reason",
                "external coordination scenario ended without execution",
            ],
        );
    }
    let completed = team_run_json(&home, &project_id, &["complete", "--id", &run_id, "--json"]);
    assert_eq!(completed["status"].as_str(), Some("completed"));
}
