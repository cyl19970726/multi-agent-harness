//! Integration coverage for the Agent Team v0 surface (team-run task):
//!   - `harness team-run create|list|status|inbox|ack|send|events` CLI smoke against an
//!     isolated HOME (temp store, real binary),
//!   - `POST /v1/team-runs` creates the run + member runs + assignment
//!     messages + folded events, and the response snapshot carries the six new
//!     ledger projections,
//!   - `POST /v1/team-runs/{id}/messages` routes a message (400 on unknown
//!     run), `POST /v1/team-runs/{id}/start` accepts asynchronous execution,
//!   - `GET /team-console` serves the console page as text/html,
//!   - SSE `/v1/events` streams `team_run_event` frames for appended rows.

use std::time::Duration;

use harness_core::{TeamDeliveryPolicy, TeamDeliveryStatus};
use harness_store::HarnessStore;

mod fake_provider;
mod harness_env;
use harness_env::{collect_sse_data, current_project_id, run_harness, ServeHandle, TempHome};

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
            "lead:coordinator:kimi",
            "--member",
            "worker-1:implementer:codex:gpt-5@crates/a,docs",
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

    // status --json: members + no actions yet + both assignments un-acked.
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(status["team_run"]["id"].as_str(), Some(run_id.as_str()));
    let members = status["members"].as_array().expect("members");
    assert_eq!(members.len(), 2, "members: {members:?}");
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
    assert!(
        member_detail["assignment"]["correlation_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "member detail preserves the Assignment correlation"
    );
    assert_eq!(
        member_detail["mailbox"]["inbox"].as_array().map(Vec::len),
        Some(1),
        "member detail includes its Assignment inbox"
    );

    // The compatibility field counts only manual_ack deliveries that actually
    // reached delivered: a queued manual ACK remains non-actionable.
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let mut assignments = store.team_messages().expect("assignment messages");
    assignments.sort_by(|left, right| left.id.cmp(&right.id));
    assignments[0].deliveries[0].policy = TeamDeliveryPolicy::ManualAck;
    assignments[0].deliveries[0].status = TeamDeliveryStatus::Queued;
    assignments[1].deliveries[0].policy = TeamDeliveryPolicy::ManualAck;
    assignments[1].deliveries[0].status = TeamDeliveryStatus::Delivered;
    store
        .append_team_message(&assignments[0])
        .expect("append queued manual ACK delivery");
    store
        .append_team_message(&assignments[1])
        .expect("append delivered manual ACK delivery");
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(
        status["unacked_messages"].as_u64(),
        Some(1),
        "only delivered manual ACKs are actionable"
    );
    assignments[1].deliveries[0].status = TeamDeliveryStatus::Failed;
    store
        .append_team_message(&assignments[1])
        .expect("append failed manual ACK delivery");
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(
        status["unacked_messages"].as_u64(),
        Some(0),
        "failed manual ACK deliveries are terminal rather than actionable"
    );
    assignments[1].deliveries[0].status = TeamDeliveryStatus::Expired;
    store
        .append_team_message(&assignments[1])
        .expect("append expired manual ACK delivery");
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(
        status["unacked_messages"].as_u64(),
        Some(0),
        "expired manual ACK deliveries are terminal rather than actionable"
    );

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

    // events --json: 5 create-time events + 1 send event, seq 1..=6 in order.
    let events = team_run_json(&home, &project_id, &["events", "--id", &run_id, "--json"]);
    let events = events.as_array().expect("events array");
    assert_eq!(events.len(), 6, "events: {events:?}");
    let seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6], "seq strictly increasing");
    assert_eq!(events[0]["entity_type"].as_str(), Some("team_run"));
    assert_eq!(events[0]["operation"].as_str(), Some("created"));
    assert_eq!(events[0]["source_kind"].as_str(), Some("host"));
    // The send folded a member-sourced message event (v0: no member status flip).
    let last = &events[5];
    assert_eq!(last["entity_type"].as_str(), Some("message"));
    assert_eq!(last["source_kind"].as_str(), Some("member"));
    assert_eq!(last["member_run_id"].as_str(), Some(member_ids[1]));

    // events --after-seq 5: only the send event remains.
    let tail = team_run_json(
        &home,
        &project_id,
        &["events", "--id", &run_id, "--after-seq", "5", "--json"],
    );
    let tail = tail.as_array().expect("tail array");
    assert_eq!(tail.len(), 1, "tail: {tail:?}");
    assert_eq!(tail[0]["seq"].as_u64(), Some(6));

    // Member-to-Host mail is actionable immediately; CLI ACK is the complete
    // control-plane path and removes it from the default Inbox without erasing
    // the latest historical projection.
    assignments[1].deliveries[0].status = TeamDeliveryStatus::Delivered;
    store
        .append_team_message(&assignments[1])
        .expect("restore delivered Assignment ownership before handoff");
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
            "--correlation-id",
            &assignments[1].correlation_id,
            "--causation-id",
            &assignments[1].id,
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

    // create --json: the full created bundle (run + member runs + assignments).
    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Second run",
            "--member",
            "solo:worker:kimi",
            "--json",
        ],
    );
    assert_eq!(created["team_run"]["status"].as_str(), Some("planning"));
    assert_eq!(
        created["member_runs"].as_array().map(Vec::len),
        Some(1),
        "member runs: {created:?}"
    );
    let assignments = created["assignment_messages"]
        .as_array()
        .expect("assignment messages");
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0]["kind"].as_str(), Some("assignment"));
    assert_eq!(assignments[0]["from_member_id"].as_str(), Some("host"));
    assert_eq!(
        assignments[0]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
}

#[test]
fn team_run_cli_message_reuses_assignment_lineage_only_within_its_run() {
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
    let assignment = &created["assignment_messages"][0];
    let assignment_id = assignment["id"].as_str().unwrap();
    let correlation_id = assignment["correlation_id"].as_str().unwrap();
    let members = created["member_runs"].as_array().unwrap();
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let mut delivered_assignment = store
        .team_messages()
        .expect("assignment messages")
        .into_iter()
        .find(|message| message.id == assignment_id)
        .expect("lead assignment");
    delivered_assignment.deliveries[0].status = TeamDeliveryStatus::Delivered;
    store
        .append_team_message(&delivered_assignment)
        .expect("deliver lead Assignment before handoff");
    let wrong_owner = run_harness(
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
            members[1]["id"].as_str().unwrap(),
            "--to",
            "host",
            "--kind",
            "handoff",
            "--body",
            "must not reuse another member's assignment",
            "--correlation-id",
            correlation_id,
        ],
    );
    assert!(!wrong_owner.status.success());
    assert!(
        String::from_utf8_lossy(&wrong_owner.stderr)
            .contains("is not an Assignment delivered to MemberRun"),
        "stderr: {}",
        String::from_utf8_lossy(&wrong_owner.stderr)
    );

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
            "handoff linked to assignment",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            assignment_id,
            "--json",
        ],
    );
    assert_eq!(handoff["correlation_id"].as_str(), Some(correlation_id));
    assert_eq!(handoff["causation_id"].as_str(), Some(assignment_id));

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
    // even when it presents valid assignment lineage from the target run.
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
            assignment_id,
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
            assignment_id,
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
            "corr-not-an-assignment",
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not identify an assignment"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
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
            created["assignment_messages"][1]["id"].as_str().unwrap(),
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
    let assignment_id = created["result"]["assignment_messages"][0]["id"]
        .as_str()
        .expect("assignment id");

    let (status, inbox) =
        serve.get_json(&format!("/v1/team-runs/{run_id}/members/{member_id}/inbox"));
    assert_eq!(status, 200, "body: {inbox}");
    assert_eq!(
        inbox["messages"].as_array().map(Vec::len),
        Some(1),
        "queued assignment is actionable"
    );
    assert_eq!(inbox["messages"][0]["id"].as_str(), Some(assignment_id));
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
    let assignment = &created["result"]["assignment_messages"][0];
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": member_id,
            "to_member_ids": ["host"],
            "kind": "message",
            "body": "QUESTION: choose A or B",
            "correlation_id": assignment["correlation_id"],
            "causation_id": assignment["id"],
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
                {"name": "lead", "role": "coordinator", "provider": "kimi"},
                {"name": "worker-1", "role": "implementer", "provider": "codex",
                 "model": "gpt-5", "worktree_ref": project_root, "owned_paths": ["crates/a"]},
            ],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(true), "body: {body}");

    // result: the created bundle (run + member runs + assignment messages).
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
    assert_eq!(
        result["assignment_messages"].as_array().map(Vec::len),
        Some(2)
    );
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
    assert_eq!(messages.len(), 2, "team_messages: {messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m["kind"].as_str() == Some("assignment")
                && m["from_member_id"].as_str() == Some("host")
                && m["deliveries"][0]["policy"].as_str() == Some("queue")
                && m["deliveries"][0]["status"].as_str() == Some("queued")),
        "queued host assignments: {messages:?}"
    );

    // Folded events: 1 run + 2 member runs + 2 messages, seq 1..=5.
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
                {"name": "lead", "role": "coordinator", "provider": "kimi"},
                {"name": "worker-1", "role": "implementer", "provider": "kimi"},
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
    let assignment_id = body["result"]["assignment_messages"][0]["id"]
        .as_str()
        .expect("assignment id")
        .to_string();
    let assignment_correlation = body["result"]["assignment_messages"][0]["correlation_id"]
        .as_str()
        .expect("assignment correlation")
        .to_string();

    // Route a handoff from the worker to the lead.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": member_ids[1],
            "to_member_ids": [member_ids[0]],
            "kind": "handoff",
            "body": "take over the review",
            "correlation_id": assignment_correlation,
            "causation_id": assignment_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(true), "body: {body}");
    assert_eq!(body["result"]["kind"].as_str(), Some("handoff"));
    assert_eq!(
        body["result"]["correlation_id"].as_str(),
        Some(assignment_correlation.as_str())
    );
    assert_eq!(
        body["result"]["causation_id"].as_str(),
        Some(assignment_id.as_str())
    );
    assert_eq!(
        body["result"]["team_run_id"].as_str(),
        Some(run_id.as_str())
    );
    assert_eq!(
        body["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    // 2 assignment messages + this one; the send folded one more event (6 total).
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot["team_messages"].as_array().map(Vec::len), Some(3));
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

    let mut host_handoff_id = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        host_handoff_id = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| {
                message["team_run_id"].as_str() == Some(run_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
                    && message["deliveries"].as_array().is_some_and(|deliveries| {
                        deliveries.iter().any(|delivery| {
                            delivery["member_id"].as_str() == Some("host")
                                && delivery["status"].as_str() == Some("delivered")
                        })
                    })
            })
            .and_then(|message| message["id"].as_str().map(str::to_string));
        if host_handoff_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let host_handoff_id = host_handoff_id.expect("provider handoff to host");

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
    let _project_id = init_project(&home, "alpha");
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
                {"name": "Builder", "role": "implementer", "provider": "codex"},
                {"name": "Reviewer", "role": "reviewer", "provider": "codex"}
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
    let assignment_id = created["result"]["assignment_messages"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let assignment_correlation = created["result"]["assignment_messages"][0]["correlation_id"]
        .as_str()
        .unwrap()
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

    // A TeamRun decision is independent of persistent Member runtime lifetime.
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
            "correlation_id": assignment_correlation,
            "causation_id": assignment_id,
        }),
    );
    assert_eq!(status, 200, "body: {host_mail}");
    let host_message_id = host_mail["result"]["id"].as_str().unwrap().to_string();
    let (status, peer_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": reviewer_id,
            "to_member_ids": [builder_id],
            "kind": "message",
            "body": "PEER FOLLOW-UP after TeamRun completion",
            "correlation_id": assignment_correlation,
            "causation_id": assignment_id,
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
        let peer_handoff = messages.iter().any(|message| {
            message["from_member_id"].as_str() == Some(builder_id.as_str())
                && message["kind"].as_str() == Some("handoff")
                && message["causation_id"].as_str() == Some(peer_message_id.as_str())
        });
        delivered_once = delivered(&host_message_id) && delivered(&peer_message_id) && peer_handoff;
        if delivered_once {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        delivered_once,
        "Host and peer mail were not each delivered exactly once and reflected by the follow-up handoff"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let builder_handoffs = snapshot["team_messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| {
            message["from_member_id"].as_str() == Some(builder_id.as_str())
                && message["kind"].as_str() == Some("handoff")
        })
        .collect::<Vec<_>>();
    assert!(
        (2..=3).contains(&builder_handoffs.len()),
        "sequential Host and peer writes may be accepted in one batch or two consecutive rounds, but each round must produce one authoritative handoff: {}",
        builder_handoffs.len()
    );
    assert!(
        builder_handoffs.iter().any(|message| {
            message["causation_id"].as_str() == Some(peer_message_id.as_str())
                && message["correlation_id"].as_str() == Some(assignment_correlation.as_str())
        }),
        "the follow-up handoff must keep Assignment correlation and point to the latest consumed message"
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
                "execution_mode": "codex_app_server"
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
            "members": [{"name": "codex-stop", "role": "observer", "provider": "codex", "execution_mode": "codex_app_server"}]
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
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &[("PATH", path.as_str())]);
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise explicit Host close",
            "members": [{"name": "codex-close", "role": "observer", "provider": "codex"}]
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
    assert!(running, "Codex member never became live");

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
            });
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(stopped, "Codex member did not terminate after Host close");
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
            "members": [{"name": "codex-provider-stop", "role": "observer", "provider": "codex", "execution_mode": "codex_app_server"}]
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
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_WAIT", "1"),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Kimi cancellation",
            "members": [{"name": "kimi-live", "role": "observer", "provider": "kimi", "model": "k2.5"}]
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
            "members": [{"name": "kimi-idle", "role": "implementer", "provider": "kimi"}]
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
    let assignment_correlation = created["result"]["assignment_messages"][0]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut first_session = None;
    let mut first_handoff_id = None;
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
        first_handoff_id = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| {
                message["from_member_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
            })
            .and_then(|message| message["id"].as_str().map(str::to_string));
        if first_session.is_some() && first_handoff_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let first_session = first_session.expect("Kimi idle native session");
    let first_handoff_id = first_handoff_id.expect("Kimi first handoff");
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "message",
            "body": "late Kimi follow-up",
            "correlation_id": assignment_correlation,
            "causation_id": first_handoff_id,
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    let message_id = sent["result"]["id"].as_str().unwrap().to_string();

    let mut second_round = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let handoffs = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| {
                message["from_member_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
            })
            .collect::<Vec<_>>();
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
        let second_handoff_has_exact_lineage = handoffs.iter().any(|message| {
            message["causation_id"].as_str() == Some(message_id.as_str())
                && message["correlation_id"].as_str() == Some(assignment_correlation.as_str())
        });
        second_round =
            handoffs.len() == 2 && delivered && same_session && second_handoff_has_exact_lineage;
        if second_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        second_round,
        "late Kimi mail was not delivered exactly once with exact round lineage on the same session"
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
            "members": [{"name": "codex-question", "role": "implementer", "provider": "codex", "execution_mode": "codex_app_server"}]
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
            ("FAKE_KIMI_ASK", "1"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Wait for Lead, then be interrupted",
            "members": [{"name": "kimi-waiting", "role": "observer", "provider": "kimi", "model": "k2.5"}]
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
fn kimi_0291_interrupt_fails_before_false_cancel_ack_or_interaction_mutation() {
    let home = TempHome::new("team-run-kimi-0291-no-cancel");
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
            ("FAKE_KIMI_VERSION", "0.29.1"),
            ("FAKE_KIMI_ASK", "1"),
            ("FAKE_KIMI_CANCEL_MARKER", cancel_marker_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Prove unsupported Kimi cancellation fails closed",
            "members": [{"name": "kimi-0291", "role": "observer", "provider": "kimi", "model": "k2.5"}]
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

    let mut waiting_snapshot = serde_json::Value::Null;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let waiting = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("waiting")
                    && member["provider_profile"]["provider_version"].as_str() == Some("0.29.1")
                    && member["provider_profile"]["supports_cancel"].as_bool() == Some(false)
            });
        if waiting {
            waiting_snapshot = snapshot;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(
        waiting_snapshot,
        serde_json::Value::Null,
        "Kimi 0.29.1 never exposed its fail-closed capability profile"
    );

    let (status, interrupted) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({"reason": "must fail closed", "requested_by": "operator"}),
    );
    assert_eq!(status, 400, "body: {interrupted}");
    assert!(
        interrupted["error"]
            .as_str()
            .is_some_and(|error| error.contains("does not support provider-native cancellation")),
        "body: {interrupted}"
    );

    let (_, after) = serve.get_json("/v1/snapshot");
    assert!(
        after["pending_interactions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|interaction| {
                interaction["member_run_id"].as_str() == Some(member_id.as_str())
                    && interaction["status"].as_str() == Some("pending")
            }),
        "unsupported Interrupt mutated the pending interaction: {after}"
    );
    assert!(
        !after["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("interrupt_requested")
            }),
        "unsupported Interrupt recorded a false cancel request: {after}"
    );
    assert!(
        !cancel_marker.exists(),
        "unsupported Interrupt reached ACP session/cancel"
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
