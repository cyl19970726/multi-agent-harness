use super::*;

#[test]
fn work_list_since_returns_only_works_changed_after_cursor() {
    let fixture = seed_board_read_fixture("work-since");

    let snapshot = team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "list",
            "--team-run-id",
            &fixture.run_id,
            "--since",
            "0",
        ],
    );
    assert_eq!(snapshot["since"].as_u64(), Some(0));
    assert_eq!(
        snapshot["works"].as_array().map(Vec::len),
        Some(6),
        "since=0 returns every Work: {snapshot}"
    );
    let baseline_next_since = snapshot["next_since"].as_u64().expect("next_since");

    // One more mutation after the snapshot: the Host resumes the blocked Work.
    let resumed = team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "resume",
            "--team-run-id",
            &fixture.run_id,
            "--work-id",
            &fixture.work_blocked_id,
            "--expected-version",
            "4",
            "--resolution",
            "dependency resolved",
        ],
    );
    assert_eq!(resumed["phase"].as_str(), Some("active"));
    assert_eq!(resumed["condition"].as_str(), Some("normal"));
    assert_eq!(resumed["version"].as_u64(), Some(5));

    let delta = team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "list",
            "--team-run-id",
            &fixture.run_id,
            "--since",
            &baseline_next_since.to_string(),
        ],
    );
    let delta_works = delta["works"].as_array().expect("delta works");
    assert_eq!(
        delta_works.len(),
        1,
        "only the Work that changed after the cursor comes back: {delta}"
    );
    assert_eq!(
        delta_works[0]["id"].as_str(),
        Some(fixture.work_blocked_id.as_str())
    );
    assert_eq!(delta_works[0]["phase"].as_str(), Some("active"));
    assert_eq!(delta_works[0]["condition"].as_str(), Some("normal"));
    assert_eq!(delta_works[0]["version"].as_u64(), Some(5));
    let next_since = delta["next_since"].as_u64().expect("next_since");
    assert_eq!(
        next_since,
        baseline_next_since + 1,
        "exactly one new WorkOperation landed since the baseline cursor"
    );

    // Chaining --since with the fresh cursor sees nothing new: the delta read
    // is idempotent at the tip of the operation log.
    let empty = team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "list",
            "--team-run-id",
            &fixture.run_id,
            "--since",
            &next_since.to_string(),
        ],
    );
    assert_eq!(
        empty["works"].as_array().map(Vec::len),
        Some(0),
        "nothing changed since the latest cursor: {empty}"
    );
    assert_eq!(empty["next_since"].as_u64(), Some(next_since));

    // `--since` is a TeamRun-local WorkOperation cursor. A durable Team can
    // span several runs, so the CLI must refuse to mislabel those unrelated
    // run-local positions as one Team-wide order.
    let team_scoped_since = run_firm(
        &fixture.home,
        fixture.home.base(),
        &[
            "--project",
            &fixture.project_id,
            "team-run",
            "work",
            "list",
            "--team-id",
            "team-cross-run",
            "--since",
            "0",
        ],
    );
    assert!(!team_scoped_since.status.success());
    assert!(
        String::from_utf8_lossy(&team_scoped_since.stderr)
            .contains("--since requires --team-run-id"),
        "Team-scoped cursor refusal must be actionable: {}",
        String::from_utf8_lossy(&team_scoped_since.stderr)
    );
}
