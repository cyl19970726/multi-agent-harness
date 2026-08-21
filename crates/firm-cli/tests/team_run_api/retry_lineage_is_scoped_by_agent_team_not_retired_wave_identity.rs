use super::*;

#[test]
fn retry_lineage_is_scoped_by_agent_team_not_retired_wave_identity() {
    let home = TempHome::new("team-run-previous-wave");
    let project_id = init_project(&home, "alpha");
    let first = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "first",
            "--agent-team-id",
            FIXTURE_TEAM_ID,
            "--member",
            "worker-a:implementer:kimi",
            "--json",
        ],
    );
    let first_id = first["team_run"]["id"].as_str().unwrap();
    assert!(first["team_run"].get("wave_id").is_none());
    let second = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "same Team retry",
            "--previous",
            first_id,
            "--member",
            "worker-b:implementer:kimi",
            "--json",
        ],
    );
    assert_eq!(
        second["team_run"]["previous_run_id"].as_str(),
        Some(first_id)
    );
    assert_eq!(
        second["team_run"]["agent_team_id"].as_str(),
        Some(FIXTURE_TEAM_ID)
    );
    let runs = team_run_json(&home, &project_id, &["list", "--json"]);
    assert_eq!(runs.as_array().map(Vec::len), Some(2));
}
