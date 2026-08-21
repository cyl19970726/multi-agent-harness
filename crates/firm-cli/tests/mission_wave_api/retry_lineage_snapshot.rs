use super::*;

// Historical Wave/TeamRun retry umbrella. Its middle section exercises the
// retired run-addressed HTTP message writer without a current NodeDaemon;
// current RoleAction/Message fabric and explicit 410 route inventory have
// independent executable coverage.
#[test]
fn mission_team_run_retry_lineage_wave_retirement_and_snapshot_contract() {
    let home = TempHome::new("mission-wave-api");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Public JSON parsing and domain validation reject malformed TeamRuns
    // before any run/member/message/event row is appended. Unaffected by
    // ADR 0051: `wave_index` was already retired compatibility, separately
    // from the Wave-write retirement this test now exercises below.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "obsolete wave index",
            "wave_index": 2,
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi", "initial_work": "Integrate the attempt and provide evidence."}],
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
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi", "initial_work": "Integrate the attempt and provide evidence."}],
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

    // HTTP authoring: a native Mission appears in the product snapshot; no
    // Goal or Task graph is created as a side effect. Wave no longer owns
    // execution attempts (ADR 0051): the Host records judgment as a Mission
    // Log entry instead, and nothing populates the Wave ledger for a fresh
    // Mission anymore.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-alpha",
            "title": "Ship agent team retry semantics",
            "objective": "Prove TeamRun retry lineage survives the Mission Log cutover",
            "desired_outcome": "A completed retry attempt",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["id"].as_str(), Some("mission-alpha"));
    let host = create_canonical_agent_member(
        &home,
        home.base(),
        &project_id,
        "agent-alpha-host",
        "Alpha Host",
        "host",
        "codex",
        &[("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
    );
    assert!(
        host.status.success(),
        "canonical host create failed: {host:?}"
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();
    run_json(
        &home,
        &project_id,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-alpha",
            "--name",
            "Alpha Team",
            "--description",
            "Flat retry Team",
            "--mission-id",
            "mission-alpha",
            "--host-agent-id",
            "agent-alpha-host",
            "--node-id",
            &node_id,
            "--member",
            "agent-alpha-host",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-alpha",
            "--kind",
            "judgment",
            "--body",
            "Two lanes will run concurrently; integration follows the first completed attempt.",
            "--json",
        ],
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot["missions"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        snapshot["legacy_waves"].as_array().map(Vec::len),
        Some(0),
        "wave create is retired: nothing populates this ledger for a new Mission"
    );
    assert_eq!(snapshot["mission_log"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        snapshot["mission_log"][0]["kind"].as_str(),
        Some("judgment")
    );

    // CLI list returns native Mission rows without advertising the empty
    // Legacy `wave_ids` compatibility field.
    let missions = run_json(&home, &project_id, &["mission", "list"]);
    let native = missions
        .as_array()
        .expect("mission list")
        .iter()
        .find(|mission| mission["id"].as_str() == Some("mission-alpha"))
        .expect("native mission");
    assert!(native.get("wave_ids").is_none());

    // Historical Wave rows remain readable (ADR 0051): seeded directly
    // (never through `wave create`, which is retired), they still project
    // through the explicit Legacy read surface in index order. Current
    // TeamRun creation does not resolve or bind these rows.
    seed_historical_wave(
        &home,
        &project_id,
        "wave-alpha",
        "mission-alpha",
        1,
        "agent_team",
    );
    seed_historical_wave(
        &home,
        &project_id,
        "wave-alpha-later",
        "mission-alpha",
        2,
        "agent_team",
    );
    let waves = run_json(
        &home,
        &project_id,
        &["legacy", "wave", "list", "--mission-id", "mission-alpha"],
    );
    assert_eq!(
        waves
            .as_array()
            .unwrap()
            .iter()
            .map(|wave| wave["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2],
        "wave list still orders historical rows by index"
    );

    // Reject any TeamRun request that tries to bind a Legacy Wave. The
    // request must be atomic: no run is recorded.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({"id": "mission-beta", "title": "Other", "objective": "isolation"}),
    );
    assert_eq!(status, 200, "body: {body}");
    seed_historical_wave(
        &home,
        &project_id,
        "wave-beta",
        "mission-beta",
        1,
        "agent_team",
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

    // The rejection is independent of historical executor metadata: no
    // Legacy Wave can become the current TeamRun executor target.
    seed_historical_wave(&home, &project_id, "wave-host", "mission-alpha", 3, "host");
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "invalid executor",
            "wave_id": "wave-host",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");

    // Attempt A is cancelled. Attempt B is a retry via `previous_run_id`.
    // Mission-only (no wave_id) is the primary TeamRun creation path now
    // that Wave no longer owns execution attempts (ADR 0034/0051).
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "first attempt",
            "agent_team_id": "team-alpha",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi", "initial_work": "Integrate the first attempt and submit evidence for Host review."}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let attempt_a = body["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(body["result"]["team_run"]["agent_team_id"], "team-alpha");
    assert!(body["result"]["team_run"]["wave_id"].is_null());
    assert!(body["result"]["team_run"].get("task_ids").is_none());
    let member_id = body["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "agent-alpha-host",
            "sender_kind": "host",
            "sender_id": "agent-alpha-host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "Please execute the assigned Work.",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let request_id = body["result"]["id"]
        .as_str()
        .expect("request id")
        .to_string();
    let conversation_correlation = body["result"]["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();

    // Work is the ownership path. Conversation correlation remains useful for
    // replies, but it does not create or transfer responsibility.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "operator-test",
            "sender_kind": "operator",
            "sender_id": "operator-test",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "implementation handoff",
            "correlation_id": conversation_correlation,
            "causation_id": request_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["correlation_id"].as_str(),
        Some(conversation_correlation.as_str())
    );
    assert_eq!(
        body["result"]["causation_id"].as_str(),
        Some(request_id.as_str())
    );
    let handoff_id = body["result"]["id"]
        .as_str()
        .expect("handoff id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "accepted",
            "causation_id": handoff_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["correlation_id"].as_str(),
        Some(conversation_correlation.as_str()),
        "causation-only reply inherits its cause correlation"
    );

    // Provider/member failure settles at reviewing; it can be explicitly
    // cancelled so a truthful retry can be created without marking the
    // failed attempt completed. Unrelated to any Wave gate now -- there is
    // no gate left to race.
    force_team_run_reviewing(&home, &project_id, &attempt_a, "mission-alpha");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("cancelled"));

    // Log-before-act (ADR 0051): the Host records why it is retrying before
    // creating the replacement attempt, not as after-the-fact narration.
    let replan = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-alpha",
            "--kind",
            "replan",
            "--body",
            "First attempt failed in review; retry with a fresh ProviderRuntimeProjection.",
            "--json",
        ],
    );
    assert_eq!(replan["revision"].as_u64(), Some(2));

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "replacement attempt",
            "agent_team_id": "team-alpha",
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
        body["result"]["team_run"]["previous_run_id"].as_str(),
        Some(attempt_a.as_str())
    );

    force_team_run_reviewing(&home, &project_id, &attempt_b, "mission-alpha");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_b}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("completed"));

    // The Host records closeout evidence in the Mission Log instead of a
    // Wave gate accepting the retry -- an append-only log has nothing
    // analogous to a gate to accept, revise, or block (ADR 0051).
    let closeout = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-alpha",
            "--kind",
            "closeout_evidence",
            "--body",
            "Retry attempt completed and reviewed.",
            "--json",
        ],
    );
    assert_eq!(closeout["revision"].as_u64(), Some(3));
    let entries = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-alpha",
            "--json",
        ],
    );
    assert_eq!(
        entries
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["revision"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    // The Wave gate that used to accept a retry attempt is retired on every
    // surface, regardless of which attempt or Wave id is named.
    let (status, body) = serve.post_json(
        "/v1/waves/wave-alpha/gate",
        &serde_json::json!({"status": "accepted", "run_id": attempt_b}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("retired"),
        "body: {body}"
    );
    let cli_gate = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "wave",
            "gate",
            "--id",
            "wave-alpha",
            "--status",
            "accepted",
            "--run-id",
            &attempt_b,
        ],
    );
    assert!(!cli_gate.status.success());
    assert!(String::from_utf8_lossy(&cli_gate.stderr).contains("retired"));

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
