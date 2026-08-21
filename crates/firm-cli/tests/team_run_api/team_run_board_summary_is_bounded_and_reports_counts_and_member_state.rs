use super::*;

#[test]
fn team_run_board_summary_is_bounded_and_reports_counts_and_member_state() {
    let fixture = seed_board_read_fixture("board-summary");

    let out = run_firm(
        &fixture.home,
        fixture.home.base(),
        &[
            "--project",
            &fixture.project_id,
            "team-run",
            "board-summary",
            "--id",
            &fixture.run_id,
        ],
    );
    assert!(
        out.status.success(),
        "board-summary failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let summary = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        summary.chars().count() <= 500,
        "board-summary must stay <=500 chars, got {}: {summary:?}",
        summary.chars().count()
    );
    for expected in [
        "open=1",
        "active=2",
        "blocked=1",
        "review=1",
        "accepted=1",
        "cancelled=1",
        "assigned=4",
        "unassigned=2",
        "ready=1",
        "alice: working",
        "bob: awaiting-review",
        "charlie: idle",
    ] {
        assert!(
            summary.contains(expected),
            "board-summary missing {expected:?}: {summary}"
        );
    }
    assert!(
        serde_json::from_str::<serde_json::Value>(&summary).is_err(),
        "board-summary is plain text, not JSON: {summary}"
    );

    // Unknown run id fails with a descriptive error instead of an empty
    // summary, mirroring `team-run status`.
    let missing = run_firm(
        &fixture.home,
        fixture.home.base(),
        &[
            "--project",
            &fixture.project_id,
            "team-run",
            "board-summary",
            "--id",
            "team-run-does-not-exist",
        ],
    );
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("team run not found"),
        "stderr: {}",
        String::from_utf8_lossy(&missing.stderr)
    );
}
