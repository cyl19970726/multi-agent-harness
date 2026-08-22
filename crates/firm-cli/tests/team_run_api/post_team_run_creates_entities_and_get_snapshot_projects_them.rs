use super::*;

#[test]
fn post_team_run_creates_entities_and_get_snapshot_projects_them() {
    let home = TempHome::new("team-run-api");
    let project_id = init_project(&home, "alpha");
    let project_root =
        std::fs::canonicalize(home.base().join("alpha")).expect("canonical project root");
    seed_mission_with_legacy_wave(&home, &project_id);
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Ship v0",
            "agent_team_id": FIXTURE_TEAM_ID,
            "execution_root": project_root,
            "budget_limit_usd": 5.0,
            "members": [
                {"name": "lead", "role": "coordinator", "provider": "kimi",
                 "initial_work": "Coordinate the delivery"},
                {"name": "worker-1", "role": "implementer", "provider": "codex",
                 "model": "gpt-5", "provider_cwd_hint": project_root, "owned_paths": ["crates/a"],
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
        result["team_run"]["agent_team_id"].as_str(),
        Some(FIXTURE_TEAM_ID)
    );
    assert!(result["team_run"].get("mission_id").is_none());
    assert!(result["team_run"].get("wave_id").is_none());
    assert_eq!(
        result["team_run"]["execution_root"].as_str(),
        Some(project_root.to_str().expect("project root"))
    );
    assert_eq!(
        result["team_run"]["host_surface"].as_str(),
        Some("http"),
        "HTTP-created runs default host_surface to http"
    );
    assert_eq!(result["member_runs"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        result["member_runs"][1]["provider_cwd_hint"].as_str(),
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
        Some(3)
    );
    let waves = snapshot["legacy_waves"].as_array().expect("legacy_waves");
    assert_eq!(waves.len(), 1, "waves: {waves:?}");
    assert_eq!(waves[0]["id"].as_str(), Some("wave-test"));
    assert_eq!(waves[0]["executor_run_ids"], serde_json::json!([]));

    let member_runs = snapshot["member_runs"].as_array().expect("member_runs");
    assert_eq!(member_runs.len(), 3, "member_runs: {member_runs:?}");
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
            .all(|work| work["phase"].as_str() == Some("open")
                && work["claim_mode"].as_str() == Some("host_assign")
                && work["active_member_run_id"].as_str().is_some()),
        "host-assigned initial Works: {works:?}"
    );

    // Folded events: 1 run + 3 member runs (including Host) + 2 Works.
    let events = snapshot["team_run_events"]
        .as_array()
        .expect("team_run_events");
    assert_eq!(events.len(), 6, "events: {events:?}");
    let mut seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6]);
    assert!(
        events
            .iter()
            .all(|e| e["team_run_id"].as_str() == Some(run_id.as_str())
                && e["operation"].as_str() == Some("created")),
        "all events folded into the run: {events:?}"
    );

    assert_eq!(snapshot["team_runs"].as_array().map(Vec::len), Some(1));
}
