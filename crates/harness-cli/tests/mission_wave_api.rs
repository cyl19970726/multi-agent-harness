//! End-to-end acceptance for the additive Mission/Wave control plane.
//!
//! This deliberately exercises the public CLI and HTTP surfaces rather than
//! constructing core objects directly: Wave attempt registration, the gate,
//! and snapshot projections must agree across the surfaces a Host uses.

use std::time::{Duration, Instant};

mod fake_provider;
mod harness_env;
use harness_env::{collect_sse_data, current_project_id, run_harness, ServeHandle, TempHome};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

fn run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_harness(home, home.base(), &full);
    assert!(
        out.status.success(),
        "harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|error| panic!("harness {args:?} stdout was not JSON ({error})"))
}

fn force_team_run_reviewing(
    home: &TempHome,
    project_id: &str,
    run_id: &str,
    mission_id: &str,
    wave_id: &str,
) {
    use std::io::Write as _;

    let path = home.projects_dir().join(project_id).join("team_runs.jsonl");
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open team run ledger");
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": run_id,
            "mission_id": mission_id,
            "wave_id": wave_id,
            "host_surface": "test",
            "objective": "accepted attempt",
            "status": "reviewing",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:2",
        })
    )
    .expect("append reviewing team run");
}

#[test]
fn host_plan_waves_keep_one_mission_team_and_member_sessions_alive() {
    let home = TempHome::new("host-plan-mission-team");
    let project_id = init_project(&home, "host-plan");

    for (id, name, role, provider) in [
        ("agent-build", "PrimaryBuilder", "primary builder", "codex"),
        ("agent-review", "ReviewPartner", "reviewer", "kimi"),
        ("agent-repair", "RepairFixer", "repair specialist", "codex"),
    ] {
        run_json(
            &home,
            &project_id,
            &[
                "agent",
                "create",
                "--id",
                id,
                "--name",
                name,
                "--role",
                role,
                "--provider",
                provider,
            ],
        );
    }
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-platform",
            "--name",
            "Platform Team",
            "--description",
            "Long-lived Mission team",
            "--lead",
            "host",
            "--member",
            "agent-build",
            "--member",
            "agent-review",
        ],
    );
    let mission = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-host-plan",
            "--title",
            "Ship host plan",
            "--objective",
            "Prove members can continue across Waves",
            "--context",
            "# Mission context\n\nKeep provider-native sessions.",
            "--json",
        ],
    );
    assert!(mission["context"]
        .as_str()
        .is_some_and(|context| context.contains("provider-native")));
    let linked = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "link-team",
            "--id",
            "mission-host-plan",
            "--team-id",
            "team-platform",
        ],
    );
    assert_eq!(
        linked["agent_team_ids"],
        serde_json::json!(["team-platform"])
    );
    let wave_1 = run_json(
        &home,
        &project_id,
        &[
            "wave",
            "create",
            "--id",
            "wave-plan-1",
            "--mission-id",
            "mission-host-plan",
            "--title",
            "Baseline",
            "--objective",
            "Start concurrent lanes",
            "--context",
            "# Wave 1\n\nTwo lanes start; review may carry forward.",
            "--json",
        ],
    );
    assert_eq!(wave_1["executor_kind"].as_str(), Some("host"));
    assert_eq!(wave_1["revision"].as_u64(), Some(1));

    let created = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "create",
            "--objective",
            "Work across Host plan revisions",
            "--agent-team-id",
            "team-platform",
            "--mission-id",
            "mission-host-plan",
            "--resume-member",
            "PrimaryBuilder:codex-session-1",
            "--json",
        ],
    );
    assert_eq!(
        created["team_run"]["agent_team_id"].as_str(),
        Some("team-platform")
    );
    assert_eq!(
        created["team_run"]["mission_id"].as_str(),
        Some("mission-host-plan")
    );
    assert!(created["team_run"]["wave_id"].is_null());
    let team_run_id = created["team_run"]["id"].as_str().unwrap();
    let builder_member_id = created["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        created["member_runs"][0]["native_session"]["native_session_id"].as_str(),
        Some("codex-session-1")
    );

    run_json(
        &home,
        &project_id,
        &[
            "wave",
            "advance",
            "--id",
            "wave-plan-1",
            "--outcome",
            "Baseline lane is ready; review continues",
        ],
    );
    let wave_2 = run_json(
        &home,
        &project_id,
        &[
            "wave",
            "create",
            "--id",
            "wave-plan-2",
            "--mission-id",
            "mission-host-plan",
            "--title",
            "Repair if needed",
            "--objective",
            "Integrate completed work while review continues",
            "--context",
            "# Wave 2\n\nCarry ReviewPartner forward and add RepairFixer.",
            "--json",
        ],
    );
    assert_eq!(wave_2["index"].as_u64(), Some(2));
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "add-member",
            "--id",
            "team-platform",
            "--member",
            "agent-repair",
        ],
    );
    let joined = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "add-member",
            "--id",
            team_run_id,
            "--member",
            "RepairFixer:repair specialist:codex",
            "--assignment",
            "Repair any issue found by the review lane",
            "--origin-wave-id",
            "wave-plan-2",
        ],
    );
    assert_eq!(
        joined["assignment"]["origin_wave_id"].as_str(),
        Some("wave-plan-2")
    );
    assert_eq!(
        joined["team_run"]["member_run_ids"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let repair_member_id = joined["member_run"]["id"].as_str().unwrap().to_string();
    let renamed = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "rename-member",
            "--id",
            team_run_id,
            "--member-run-id",
            &repair_member_id,
            "--name",
            "TargetedRepair",
        ],
    );
    assert_eq!(renamed["name"].as_str(), Some("TargetedRepair"));
    let deactivated = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "deactivate-member",
            "--id",
            team_run_id,
            "--member-run-id",
            &repair_member_id,
            "--reason",
            "No reproducible defect remained after review",
        ],
    );
    assert_eq!(deactivated["status"].as_str(), Some("stopped"));

    let status = run_json(
        &home,
        &project_id,
        &["team-run", "status", "--id", team_run_id, "--json"],
    );
    let builder = status["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["member_run"]["id"].as_str() == Some(&builder_member_id))
        .unwrap();
    assert_eq!(
        builder["member_run"]["native_session"]["native_session_id"].as_str(),
        Some("codex-session-1"),
        "Wave advance must not replace the MemberRun or provider-native session"
    );

    // Explicit retry lineage cannot jump to another stable Team or Mission.
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-other",
            "--name",
            "Other Team",
            "--description",
            "Retry isolation fixture",
            "--lead",
            "host",
            "--member",
            "agent-build",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "link-team",
            "--id",
            "mission-host-plan",
            "--team-id",
            "team-other",
        ],
    );
    let cross_team_retry = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "invalid cross-team retry",
            "--agent-team-id",
            "team-other",
            "--mission-id",
            "mission-host-plan",
            "--previous",
            team_run_id,
        ],
    );
    assert!(!cross_team_retry.status.success());
    assert!(
        String::from_utf8_lossy(&cross_team_retry.stderr).contains("not for the same agent team")
    );

    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-other",
            "--title",
            "Other Mission",
            "--objective",
            "Retry isolation fixture",
            "--json",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "wave",
            "create",
            "--id",
            "wave-other",
            "--mission-id",
            "mission-other",
            "--title",
            "Other Mission plan",
            "--objective",
            "Prove origin references cannot cross Missions",
            "--json",
        ],
    );
    let cross_mission_origin = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            team_run_id,
            "--from",
            "host",
            "--to",
            &builder_member_id,
            "--kind",
            "assignment",
            "--body",
            "invalid cross-Mission origin",
            "--origin-wave-id",
            "wave-other",
        ],
    );
    assert!(!cross_mission_origin.status.success());
    assert!(String::from_utf8_lossy(&cross_mission_origin.stderr)
        .contains("not TeamRun Mission mission-host-plan"));
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "link-team",
            "--id",
            "mission-other",
            "--team-id",
            "team-platform",
        ],
    );
    let cross_mission_retry = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "invalid cross-mission retry",
            "--agent-team-id",
            "team-platform",
            "--mission-id",
            "mission-other",
            "--previous",
            team_run_id,
        ],
    );
    assert!(!cross_mission_retry.status.success());
    assert!(
        String::from_utf8_lossy(&cross_mission_retry.stderr).contains("not in the same mission")
    );

    run_json(
        &home,
        &project_id,
        &[
            "wave",
            "advance",
            "--id",
            "wave-plan-2",
            "--outcome",
            "Host plan complete",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "close",
            "--id",
            "mission-host-plan",
            "--outcome",
            "Mission completed while the reusable team remains independent",
            "--json",
        ],
    );
    let team = run_json(
        &home,
        &project_id,
        &["team", "show", "--id", "team-platform"],
    );
    assert_eq!(team["status"].as_str(), Some("active"));
    let run = run_json(
        &home,
        &project_id,
        &["team-run", "status", "--id", team_run_id, "--json"],
    );
    assert_eq!(run["team_run"]["status"].as_str(), Some("planning"));
    let cancelled = run_json(
        &home,
        &project_id,
        &["team-run", "cancel", "--id", team_run_id, "--json"],
    );
    assert_eq!(
        cancelled["status"].as_str(),
        Some("cancelled"),
        "Mission closeout must not prevent the independent TeamRun from settling"
    );
}

#[test]
fn host_wave_accepts_direct_outcome_without_fake_run() {
    let home = TempHome::new("host-wave-gate");
    let project_id = init_project(&home, "host-wave");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-host",
            "--title",
            "Direct host work",
            "--objective",
            "Record an honest Host outcome",
            "--json",
        ],
    );
    let (status, body) = serve.post_json(
        "/v1/missions/mission-host/close",
        &serde_json::json!({"outcome": "too early"}),
    );
    assert_eq!(status, 400, "body: {body}");
    run_json(
        &home,
        &project_id,
        &[
            "wave",
            "create",
            "--id",
            "wave-host",
            "--mission-id",
            "mission-host",
            "--title",
            "Host slice",
            "--objective",
            "Finish without a fake executor run",
            "--executor-kind",
            "host",
            "--json",
        ],
    );
    let (status, body) = serve.post_json(
        "/v1/missions/mission-host/close",
        &serde_json::json!({"outcome": "still too early"}),
    );
    assert_eq!(status, 400, "body: {body}");
    let accepted = run_json(
        &home,
        &project_id,
        &[
            "wave",
            "gate",
            "--id",
            "wave-host",
            "--status",
            "accepted",
            "--accepted-by",
            "host",
            "--outcome",
            "Direct work verified",
            "--artifact",
            "check:host",
            "--json",
        ],
    );
    assert_eq!(accepted["gate_status"].as_str(), Some("accepted"));
    assert_eq!(accepted["status"].as_str(), Some("completed"));
    assert!(accepted["accepted_run_id"].is_null());

    // Host acceptance remains immutable even though its honest accepted run
    // id is null.
    let out = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "wave",
            "gate",
            "--id",
            "wave-host",
            "--status",
            "blocked",
        ],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("already accepted"));

    let (status, body) = serve.post_json(
        "/v1/missions/mission-host/close",
        &serde_json::json!({
            "outcome": "Mission intent satisfied",
            "completed_by": "dashboard-host"
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("completed"));
    assert_eq!(
        body["result"]["completed_by"].as_str(),
        Some("dashboard-host")
    );
    assert!(body["result"]["completed_at"].is_string());

    // Identical closeout is idempotent; a conflicting actor/outcome and any
    // new Wave after completion are rejected.
    let repeated = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "close",
            "--id",
            "mission-host",
            "--outcome",
            "Mission intent satisfied",
            "--completed-by",
            "dashboard-host",
            "--json",
        ],
    );
    assert_eq!(repeated["status"].as_str(), Some("completed"));
    let conflict = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "close",
            "--id",
            "mission-host",
            "--outcome",
            "different",
            "--completed-by",
            "another-host",
        ],
    );
    assert!(!conflict.status.success());
    let late_wave = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "wave",
            "create",
            "--mission-id",
            "mission-host",
            "--title",
            "Too late",
            "--objective",
            "Must be rejected",
            "--executor-kind",
            "host",
        ],
    );
    assert!(!late_wave.status.success());
}

#[test]
fn mission_wave_attempt_retry_gate_and_snapshot_contract() {
    let home = TempHome::new("mission-wave-api");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Compatibility projection ids are a read-only namespace and cannot be
    // shadowed by native authoring.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "compat-goal:spoof",
            "title": "Invalid",
            "objective": "must not shadow a Goal projection",
        }),
    );
    assert_eq!(status, 400, "body: {body}");

    // Public JSON parsing and domain validation reject malformed TeamRuns
    // before any run/member/message/event row is appended.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "obsolete wave index",
            "wave_index": 2,
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("was retired"),
        "body: {body}"
    );

    for invalid in [
        serde_json::json!({
            "objective": "no executable member",
            "members": [],
        }),
        serde_json::json!({
            "objective": "incomplete native linkage",
            "mission_id": "mission-alpha",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    ] {
        let (status, body) = serve.post_json("/v1/team-runs", &invalid);
        assert_eq!(status, 400, "body: {body}");
    }
    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert_eq!(snapshot["team_runs"].as_array().map(Vec::len), Some(0));
    assert_eq!(snapshot["member_runs"].as_array().map(Vec::len), Some(0));
    assert_eq!(snapshot["team_messages"].as_array().map(Vec::len), Some(0));

    // HTTP authoring: a native Mission and Wave appear in the product snapshot;
    // no Goal or Task graph is created as a side effect.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-alpha",
            "title": "Ship agent team retry semantics",
            "objective": "Prove a Wave owns its execution attempts",
            "desired_outcome": "One accepted team attempt",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["id"].as_str(), Some("mission-alpha"));

    let (status, body) = serve.post_json(
        "/v1/waves",
        &serde_json::json!({
            "id": "wave-invalid-index",
            "mission_id": "mission-alpha",
            "index": "not-a-number",
            "title": "Invalid",
            "objective": "must not be appended",
            "executor_kind": "agent_team",
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert_eq!(snapshot["waves"].as_array().map(Vec::len), Some(0));

    let (status, body) = serve.post_json(
        "/v1/waves",
        &serde_json::json!({
            "id": "wave-alpha",
            "mission_id": "mission-alpha",
            "title": "Run and accept the team",
            "objective": "Create two team attempts and accept the second",
            "executor_kind": "agent_team",
            "exit_criteria": "A completed attempt is accepted",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["index"].as_u64(), Some(1));
    assert_eq!(
        body["snapshot"]["missions"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(body["snapshot"]["waves"].as_array().map(Vec::len), Some(1));

    // CLI list returns native Mission rows and carries ordered Wave membership.
    let missions = run_json(&home, &project_id, &["mission", "list"]);
    let native = missions
        .as_array()
        .expect("mission list")
        .iter()
        .find(|mission| mission["id"].as_str() == Some("mission-alpha"))
        .expect("native mission");
    assert_eq!(native["wave_ids"], serde_json::json!(["wave-alpha"]));
    let waves = run_json(
        &home,
        &project_id,
        &["wave", "list", "--mission-id", "mission-alpha"],
    );
    assert_eq!(waves.as_array().map(Vec::len), Some(1));

    // Reject a TeamRun that tries to bind a Wave from another Mission. The
    // request must be atomic: no run is recorded on either Mission/Wave.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({"id": "mission-beta", "title": "Other", "objective": "isolation"}),
    );
    assert_eq!(status, 200, "body: {body}");
    let (status, body) = serve.post_json(
        "/v1/waves",
        &serde_json::json!({
            "id": "wave-beta",
            "mission_id": "mission-beta",
            "title": "Other team wave",
            "objective": "must remain isolated",
            "executor_kind": "agent_team",
        }),
    );
    assert_eq!(status, 200, "body: {body}");

    // Explicitly inserted indexes remain product-ordered in both Wave reads
    // and the owning Mission's membership list.
    for (id, index) in [("wave-beta-later", 3), ("wave-beta-middle", 2)] {
        let (status, body) = serve.post_json(
            "/v1/waves",
            &serde_json::json!({
                "id": id,
                "mission_id": "mission-beta",
                "index": index,
                "title": id,
                "objective": "ordered membership",
                "executor_kind": "host",
            }),
        );
        assert_eq!(status, 200, "body: {body}");
    }
    let mission_beta = run_json(
        &home,
        &project_id,
        &["mission", "show", "--id", "mission-beta"],
    );
    assert_eq!(
        mission_beta["wave_ids"],
        serde_json::json!(["wave-beta", "wave-beta-middle", "wave-beta-later"])
    );
    let beta_waves = run_json(
        &home,
        &project_id,
        &["wave", "list", "--mission-id", "mission-beta"],
    );
    assert_eq!(
        beta_waves
            .as_array()
            .unwrap()
            .iter()
            .map(|wave| wave["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "invalid cross join",
            "mission_id": "mission-alpha",
            "wave_id": "wave-beta",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert_eq!(snapshot["team_runs"].as_array().map(Vec::len), Some(0));

    // A non-AgentTeam Wave cannot be used as an AgentTeamRun executor target.
    let (status, body) = serve.post_json(
        "/v1/waves",
        &serde_json::json!({
            "id": "wave-host",
            "mission_id": "mission-alpha",
            "title": "Host-only step",
            "objective": "prove executor boundary",
            "executor_kind": "host",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "invalid executor",
            "wave_id": "wave-host",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");

    // Attempt A is cancelled. Attempt B is a retry in the same Wave; `previous`
    // is only attempt lineage, while Wave.executor_run_ids is the canonical list.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "first attempt",
            "mission_id": "mission-alpha",
            "wave_id": "wave-alpha",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let attempt_a = body["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        body["result"]["team_run"]["mission_id"].as_str(),
        Some("mission-alpha")
    );
    assert_eq!(
        body["result"]["team_run"]["wave_id"].as_str(),
        Some("wave-alpha")
    );
    assert!(body["result"]["team_run"].get("task_ids").is_none());
    let assignment_id = body["result"]["assignment_messages"][0]["id"]
        .as_str()
        .expect("assignment id")
        .to_string();
    let assignment_correlation = body["result"]["assignment_messages"][0]["correlation_id"]
        .as_str()
        .expect("assignment correlation")
        .to_string();
    let member_id = body["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();

    // Assignment-message correlation is the ownership path: an explicit reply
    // preserves both references, while a causation-only reply inherits its
    // direct cause's correlation without involving a legacy Task.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "from_member_id": member_id,
            "to_member_ids": ["host"],
            "kind": "handoff",
            "body": "implementation handoff",
            "correlation_id": assignment_correlation,
            "causation_id": assignment_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["correlation_id"].as_str(),
        Some(assignment_correlation.as_str())
    );
    assert_eq!(
        body["result"]["causation_id"].as_str(),
        Some(assignment_id.as_str())
    );
    let handoff_id = body["result"]["id"]
        .as_str()
        .expect("handoff id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "from_member_id": "host",
            "to_member_ids": [member_id],
            "kind": "review_result",
            "body": "accepted",
            "causation_id": handoff_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["correlation_id"].as_str(),
        Some(assignment_correlation.as_str()),
        "causation-only reply inherits its cause correlation"
    );

    // Provider/member failure settles at reviewing. It must remain active for
    // gate purposes, but can be explicitly cancelled so a truthful retry can
    // be created without marking the failed attempt completed.
    force_team_run_reviewing(
        &home,
        &project_id,
        &attempt_a,
        "mission-alpha",
        "wave-alpha",
    );

    // A gate is only meaningful once every attempt is settled. In particular,
    // blocked/revise cannot race a later TeamRun transition and leave
    // Wave.status disagreeing with gate_status.
    for gate_status in ["blocked", "revise"] {
        let (status, body) = serve.post_json(
            "/v1/waves/wave-alpha/gate",
            &serde_json::json!({"status": gate_status, "note": "too early"}),
        );
        assert_eq!(status, 400, "body: {body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| error.contains("active attempt")),
            "body: {body}"
        );
    }
    let unsettled_wave = run_json(&home, &project_id, &["wave", "show", "--id", "wave-alpha"]);
    assert_eq!(unsettled_wave["gate_status"].as_str(), Some("pending"));

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("cancelled"));

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "replacement attempt",
            "mission_id": "mission-alpha",
            "wave_id": "wave-alpha",
            "previous_run_id": attempt_a,
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let attempt_b = body["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        body["snapshot"]["waves"]
            .as_array()
            .unwrap()
            .iter()
            .find(|wave| wave["id"].as_str() == Some("wave-alpha"))
            .unwrap()["executor_run_ids"],
        serde_json::json!([attempt_a, attempt_b]),
    );

    // A cancelled attempt is not gate-eligible. Complete B through the public
    // team transition before accepting it through the public Wave gate.
    let (status, body) = serve.post_json(
        "/v1/waves/wave-alpha/gate",
        &serde_json::json!({"status": "accepted", "run_id": attempt_a}),
    );
    assert_eq!(status, 400, "body: {body}");
    let (status, body) = serve.post_json(
        "/v1/waves/wave-alpha/gate",
        &serde_json::json!({"status": "accepted", "run_id": "team-run-not-an-attempt"}),
    );
    assert_eq!(status, 400, "body: {body}");
    force_team_run_reviewing(
        &home,
        &project_id,
        &attempt_b,
        "mission-alpha",
        "wave-alpha",
    );
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_b}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {body}");

    let (status, body) = serve.post_json(
        "/v1/waves/wave-alpha/gate",
        &serde_json::json!({
            "status": "accepted",
            "run_id": attempt_b,
            "accepted_by": "operator",
            "note": "integration verified",
            "outcome": "retry accepted",
            "artifact_refs": ["check:team-run"],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let wave = &body["result"];
    assert_eq!(wave["accepted_run_id"].as_str(), Some(attempt_b.as_str()));
    assert_eq!(wave["gate_status"].as_str(), Some("accepted"));
    assert_eq!(wave["accepted_by"].as_str(), Some("operator"));
    assert_eq!(wave["artifact_refs"], serde_json::json!(["check:team-run"]));

    // An accepted Wave is immutable with respect to a conflicting attempt.
    let (status, body) = serve.post_json(
        "/v1/waves/wave-alpha/gate",
        &serde_json::json!({"status": "accepted", "run_id": attempt_a}),
    );
    assert_eq!(status, 400, "body: {body}");

    // Acceptance freezes the Wave's attempt set. A later retry must be made
    // explicit by revising before acceptance or by creating a later Wave.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "too late",
            "wave_id": "wave-alpha",
            "members": [{"name": "late", "role": "worker", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    let frozen_wave = run_json(&home, &project_id, &["wave", "show", "--id", "wave-alpha"]);
    assert_eq!(
        frozen_wave["executor_run_ids"],
        serde_json::json!([attempt_a, attempt_b])
    );

    // Historical reasoning remains in JSONL, but the new snapshot must not
    // project it as product state or evidence.
    use std::io::Write as _;
    let action_path = home
        .projects_dir()
        .join(&project_id)
        .join("member_actions.jsonl");
    let mut actions = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&action_path)
        .expect("open action ledger");
    writeln!(
        actions,
        "{}",
        serde_json::json!({
            "id": "legacy-thinking",
            "seq": 999,
            "team_run_id": attempt_b,
            "member_run_id": "legacy-member",
            "action_type": "thinking",
            "status": "succeeded",
            "title": "legacy reasoning",
            "summary": "must stay historical",
            "started_at": "unix-ms:1",
        })
    )
    .expect("append legacy thinking");
    assert!(std::fs::read_to_string(&action_path)
        .unwrap()
        .contains("legacy reasoning"));
    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert!(
        snapshot["member_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["action_type"].as_str() != Some("thinking")),
        "thinking leaked into snapshot: {:?}",
        snapshot["member_actions"]
    );
}

#[test]
fn http_console_starts_native_team_wave_and_streams_transient_thinking() {
    let home = TempHome::new("mission-wave-console-start");
    let project_id = init_project(&home, "alpha");
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
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-console",
            "title": "Console-native Agent Team",
            "objective": "Run the complete Mission/Wave journey",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let (status, body) = serve.post_json(
        "/v1/waves",
        &serde_json::json!({
            "id": "wave-console",
            "mission_id": "mission-console",
            "title": "Execute one team",
            "objective": "Let the fake member complete",
            "executor_kind": "agent_team",
            "exit_criteria": "The completed attempt is accepted",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Complete through the Console start endpoint",
            "mission_id": "mission-console",
            "wave_id": "wave-console",
            "members": [{"name": "worker", "role": "implementer", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let run_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = body["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();

    let mut sse = serve.open_sse(&format!("?project={project_id}"));
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({"max_concurrency": 1, "idle_timeout_s": 10}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("running"));

    // The synchronous reservation makes duplicate starts fail even while the
    // provider driver is still booting (or after it has already completed).
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 400, "body: {body}");

    let frames = collect_sse_data(&mut sse, Duration::from_secs(8), 30);
    assert!(
        frames.iter().any(|frame| {
            frame["kind"].as_str() == Some("thinking")
                && frame["team_run_id"].as_str() == Some(run_id.as_str())
                && frame["member_run_id"].as_str() == Some(member_id.as_str())
                && frame["preview"].as_str() == Some("hidden reasoning")
        }),
        "transient activity frame missing: {frames:?}"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let snapshot = loop {
        let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
        assert_eq!(status, 200);
        let completed = snapshot["team_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|run| {
                run["id"].as_str() == Some(run_id.as_str())
                    && run["status"].as_str() == Some("completed")
            });
        if completed {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "team run did not complete: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(
        snapshot["member_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["action_type"].as_str() != Some("thinking")),
        "thinking became durable: {}",
        snapshot["member_actions"]
    );
    assert!(
        !snapshot.to_string().contains("hidden reasoning"),
        "thinking leaked into snapshot"
    );

    // The external provider ingress applies the same lifecycle boundary and
    // refuses previews once the attempt is terminal.
    let (status, body) = serve.post_json(
        &format!("/v1/live/member-activity?project={project_id}"),
        &serde_json::json!({
            "team_run_id": run_id,
            "member_run_id": member_id,
            "preview": "too late",
        }),
    );
    assert_eq!(status, 400, "body: {body}");

    let (status, body) = serve.post_json(
        &format!("/v1/waves/wave-console/gate?project={project_id}"),
        &serde_json::json!({
            "status": "accepted",
            "run_id": run_id,
            "accepted_by": "console-host",
            "outcome": "deterministic provider completed",
            "artifact_refs": ["check:http-console"],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["gate_status"].as_str(), Some("accepted"));
    assert_eq!(
        body["result"]["accepted_run_id"].as_str(),
        Some(run_id.as_str())
    );
}
