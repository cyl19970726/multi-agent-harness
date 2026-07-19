//! End-to-end acceptance for the additive Mission/Wave control plane.
//!
//! This deliberately exercises the public CLI and HTTP surfaces rather than
//! constructing core objects directly: Wave attempt registration, the gate,
//! and snapshot projections must agree across the surfaces a Host uses.

mod harness_env;
use harness_env::{current_project_id, run_harness, ServeHandle, TempHome};

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
    for invalid in [
        serde_json::json!({
            "objective": "no executable member",
            "members": [],
        }),
        serde_json::json!({
            "objective": "bad wave index",
            "wave_index": "not-a-number",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
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
    assert_eq!(body["snapshot"]["tasks"].as_array().map(Vec::len), Some(0));

    // CLI list is the same native projection and carries the ordered membership.
    let missions = run_json(&home, &project_id, &["mission", "list"]);
    let native = missions
        .as_array()
        .expect("mission list")
        .iter()
        .find(|projection| projection["mission"]["id"].as_str() == Some("mission-alpha"))
        .expect("native mission projection");
    assert_eq!(native["source"].as_str(), Some("native"));
    assert_eq!(
        native["mission"]["wave_ids"],
        serde_json::json!(["wave-alpha"])
    );
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
        mission_beta["mission"]["wave_ids"],
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
    assert_eq!(
        body["result"]["team_run"]["task_ids"],
        serde_json::json!([])
    );
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
fn legacy_goal_projects_to_mission_without_rewriting_history() {
    let home = TempHome::new("mission-wave-compat");
    let project_id = init_project(&home, "alpha");
    let store_root = home.projects_dir().join(&project_id);

    let goal = run_json(
        &home,
        &project_id,
        &[
            "goal",
            "create",
            "--id",
            "legacy-goal",
            "--title",
            "Historical Goal",
            "--owner",
            "lead",
            "--description",
            "Keep the old ledger readable",
        ],
    );
    assert_eq!(goal["id"].as_str(), Some("legacy-goal"));
    let goals_before = std::fs::read_to_string(store_root.join("goals.jsonl")).unwrap();
    assert!(!store_root.join("missions.jsonl").exists());
    assert!(!store_root.join("waves.jsonl").exists());

    let projections = run_json(&home, &project_id, &["mission", "list"]);
    let legacy = projections
        .as_array()
        .unwrap()
        .iter()
        .find(|projection| projection["source_id"].as_str() == Some("legacy-goal"))
        .expect("Goal compatibility projection");
    assert_eq!(legacy["source"].as_str(), Some("goal_compatibility"));
    assert_eq!(
        legacy["mission"]["id"].as_str(),
        Some("compat-goal:legacy-goal")
    );
    assert_eq!(
        legacy["mission"]["objective"].as_str(),
        Some("Keep the old ledger readable")
    );

    assert_eq!(
        std::fs::read_to_string(store_root.join("goals.jsonl")).unwrap(),
        goals_before
    );
    assert!(!store_root.join("missions.jsonl").exists());
    assert!(!store_root.join("waves.jsonl").exists());
}
