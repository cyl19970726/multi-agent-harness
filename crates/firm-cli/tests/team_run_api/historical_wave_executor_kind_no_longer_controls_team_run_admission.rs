use super::*;

#[test]
fn historical_wave_executor_kind_no_longer_controls_team_run_admission() {
    let home = TempHome::new("team-run-wrong-executor");
    let project_id = init_project(&home, "alpha");
    seed_mission_with_legacy_wave(&home, &project_id);
    let wave_path = home.spaces_dir().join(&project_id).join("waves.jsonl");
    let wave = std::fs::read_to_string(&wave_path)
        .expect("read seeded wave")
        .replace("\"agent_team\"", "\"dynamic_workflow\"");
    std::fs::write(&wave_path, wave).expect("replace executor kind");

    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            FIXTURE_TEAM_ID,
            "--objective",
            "must not start",
            "--member",
            "worker:implementer:kimi",
        ],
    );
    assert!(out.status.success(), "create failed: {out:?}");
    let runs = team_run_json(&home, &project_id, &["list", "--json"]);
    assert_eq!(runs.as_array().map(Vec::len), Some(1));
    assert_eq!(runs[0]["agent_team_id"].as_str(), Some(FIXTURE_TEAM_ID));
    assert!(runs[0].get("wave_id").is_none());
}
