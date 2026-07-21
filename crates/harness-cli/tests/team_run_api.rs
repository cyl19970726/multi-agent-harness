//! Integration coverage for the Agent Team v0 surface (team-run task):
//!   - `harness team-run create|list|status|send|events` CLI smoke against an
//!     isolated HOME (temp store, real binary),
//!   - `POST /v1/team-runs` creates the run + member runs + assignment
//!     messages + folded events, and the response snapshot carries the six new
//!     ledger projections,
//!   - `POST /v1/team-runs/{id}/messages` routes a message (400 on unknown
//!     run), `POST /v1/team-runs/{id}/start` accepts asynchronous execution,
//!   - `GET /team-console` serves the console page as text/html,
//!   - SSE `/v1/events` streams `team_run_event` frames for appended rows.

use std::time::Duration;

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
    let store = home.projects_dir().join(project_id);
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
            "--member",
            "lead:coordinator:kimi",
            "--member",
            "worker-1:implementer:codex:gpt-5@crates/a,docs",
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
        members[1]["member_run"]["owned_paths"],
        serde_json::json!(["crates/a", "docs"]),
        "owned_paths parsed from @path1,path2"
    );
    assert!(
        members.iter().all(|m| m["latest_action"].is_null()),
        "no member actions journaled yet: {members:?}"
    );
    assert_eq!(status["unacked_messages"].as_u64(), Some(2));

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
            "blocker",
            "--body",
            "blocked on API design",
            "--json",
        ],
    );
    assert_eq!(message["kind"].as_str(), Some("blocker"));
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
            "answer",
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
        home.projects_dir()
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
            "progress",
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
            "progress",
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
        home.projects_dir()
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
            "progress",
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
            "progress",
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
    let wave_path = home.projects_dir().join(&project_id).join("waves.jsonl");
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
            .projects_dir()
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
        .open(
            home.projects_dir()
                .join(&project_id)
                .join("team_runs.jsonl"),
        )
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
    assert_eq!(
        body["snapshot"]["missions"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(body["snapshot"]["waves"].as_array().map(Vec::len), Some(1));
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
fn post_team_run_creates_entities_and_snapshot() {
    let home = TempHome::new("team-run-api");
    let project_id = init_project(&home, "alpha");
    seed_native_mission_wave(&home, &project_id);
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Ship v0",
            "mission_id": "mission-test",
            "wave_id": "wave-test",
            "budget_limit_usd": 5.0,
            "members": [
                {"name": "lead", "role": "coordinator", "provider": "kimi"},
                {"name": "worker-1", "role": "implementer", "provider": "codex",
                 "model": "gpt-5", "owned_paths": ["crates/a"]},
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
        result["team_run"]["host_surface"].as_str(),
        Some("http"),
        "HTTP-created runs default host_surface to http"
    );
    assert_eq!(result["member_runs"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        result["assignment_messages"].as_array().map(Vec::len),
        Some(2)
    );
    let run_id = result["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();

    // snapshot: the six new projections carry the journaled rows.
    let snapshot = &body["snapshot"];
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

    // The same rows are visible via the plain GET snapshot route.
    let (get_status, get_snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(get_status, 200);
    assert_eq!(
        get_snapshot["team_runs"].as_array().map(Vec::len),
        Some(1),
        "GET /v1/snapshot team_runs"
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
    assert_eq!(
        body["snapshot"]["team_messages"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(
        body["snapshot"]["team_run_events"].as_array().map(Vec::len),
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
    let ack_event_count = body["snapshot"]["team_run_events"]
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
    let repeated_ack_event_count = body["snapshot"]["team_run_events"]
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

    let (status, steered) = serve.post_json(
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

    let mut completed = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        completed = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("completed")
            });
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(completed, "steered member did not complete");
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
    assert!(
        stopped,
        "Codex member was marked terminal before/without provider interruption acknowledgement"
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
    assert!(stopped, "Kimi ACP member did not acknowledge cancellation");
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
    let mut completed = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        completed = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("completed")
            });
        if completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(completed, "Codex did not resume after Lead answer");
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
    let mut stopped_with_cancelled_interaction = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let stopped = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("stopped")
            });
        let cancelled = snapshot["pending_interactions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|interaction| {
                interaction["member_run_id"].as_str() == Some(member_id.as_str())
                    && interaction["status"].as_str() == Some("cancelled")
            });
        stopped_with_cancelled_interaction = stopped && cancelled;
        if stopped_with_cancelled_interaction {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        stopped_with_cancelled_interaction,
        "interrupt did not close both waiting interaction and prompt"
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
    let runs = body["snapshot"]["team_runs"].as_array().expect("team_runs");
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
    let runs = body["snapshot"]["team_runs"].as_array().expect("team_runs");
    assert_eq!(
        runs.iter()
            .find(|run| run["id"].as_str() == Some(wave1_id.as_str()))
            .and_then(|run| run["status"].as_str()),
        Some("cancelled"),
        "latest-wins projection shows the cancellation: {runs:?}"
    );
    let events = body["snapshot"]["team_run_events"]
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
    let store_root = home.projects_dir().join(&project_id);
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
    let events = body["snapshot"]["team_run_events"]
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
