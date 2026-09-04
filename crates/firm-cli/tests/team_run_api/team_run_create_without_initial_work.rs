use super::*;

#[test]
fn team_run_create_without_initial_work_accepts_the_first_host_assignment() {
    let home = TempHome::new("team-run-no-initial-work");
    let project_id = init_project(&home, "alpha");
    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--agent-team-id",
            FIXTURE_TEAM_ID,
            "--objective",
            "Assign the real Work after creating the TeamRun",
            "--member",
            "agent-runtime-host:host:kimi#Coordinate the bootstrap",
            "--member",
            "worker:implementer:kimi#Implement the bootstrap",
            "--no-initial-work",
            "--json",
        ],
    );
    let run_id = created["team_run"]["id"].as_str().expect("TeamRun id");
    assert_eq!(created["works"], serde_json::json!([]));

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    assert!(
        store
            .latest_works()
            .expect("Works")
            .into_iter()
            .all(|work| work.team_run_id != run_id),
        "--no-initial-work must not persist bootstrap Work"
    );
    let worker_run_id = created["member_runs"]
        .as_array()
        .expect("MemberRuns")
        .iter()
        .find(|member| member["agent_member_id"].as_str() == Some("worker"))
        .and_then(|member| member["id"].as_str())
        .expect("worker MemberRun")
        .to_string();

    let work_id = create_fixture_work(
        &home,
        &project_id,
        run_id,
        "First Host-assigned Work",
        Some(&worker_run_id),
    );
    let started = member_team_run_json(
        &home,
        &project_id,
        run_id,
        &worker_run_id,
        &[
            "work",
            "start",
            "--team-run-id",
            run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &worker_run_id,
        ],
    );
    assert_eq!(started["phase"].as_str(), Some("active"), "{started}");
    assert_eq!(started["version"].as_u64(), Some(3), "{started}");
}
