use super::*;

/// ADR 0051 mandatory reader: `team-run recover` must print the linked
/// Mission's Log tail before its recovery report, so a recovering Host
/// re-reads judgment before it re-derives intent from provider-native state.
/// A freshly created run's members default to `MemberCoordinationStatus::Active`
/// (`classify_member_recovery_path`'s first check), so recovery is a clean
/// no-op pass here -- this test is only about what gets printed and in what
/// order, not about the member-reopen/rebind machinery covered elsewhere.
#[test]
fn team_run_recover_prints_mission_log_tail_before_the_report() {
    let home = TempHome::new("team-run-recover-mission-log");
    let project_id = init_project(&home, "alpha");

    for (revision, (kind, body)) in [
        ("judgment", "First judgment before recovery."),
        ("replan", "Re-planned after review."),
        ("recovery", "Most recent judgment entry."),
    ]
    .into_iter()
    .enumerate()
    {
        // DOC-108 retired `mission log append`; the tail reader is proven
        // against directly-seeded pre-cutover history.
        seed_historical_mission_log(
            &home,
            &project_id,
            FIXTURE_MISSION_ID,
            revision as u64 + 1,
            kind,
            body,
            "host",
        );
    }

    let create_out = run_firm(
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
            "Recoverable run",
            "--member",
            "lead:coordinator:kimi#Coordinate the delivery",
        ],
    );
    assert!(
        create_out.status.success(),
        "team-run create failed: {}",
        String::from_utf8_lossy(&create_out.stderr)
    );
    let run_id = String::from_utf8_lossy(&create_out.stdout)
        .trim()
        .to_string();
    assert!(run_id.starts_with("team-run-"), "run id: {run_id}");

    let recover_out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "recover",
            "--id",
            &run_id,
        ],
    );
    assert!(
        recover_out.status.success(),
        "team-run recover failed: {}",
        String::from_utf8_lossy(&recover_out.stderr)
    );
    let stdout = String::from_utf8_lossy(&recover_out.stdout).to_string();
    let log_header_pos = stdout
        .find("mission log (last 3)")
        .unwrap_or_else(|| panic!("mission log tail header missing: {stdout}"));
    let report_pos = stdout
        .find("recovery complete")
        .unwrap_or_else(|| panic!("recovery report missing: {stdout}"));
    assert!(
        log_header_pos < report_pos,
        "mission log tail must print before the recovery report: {stdout}"
    );
    // Exactly 3 entries exist, so tail(3) shows all three, oldest first.
    let judgment_pos = stdout
        .find("First judgment before recovery.")
        .unwrap_or_else(|| panic!("revision 1 body missing from tail: {stdout}"));
    let replan_pos = stdout
        .find("Re-planned after review.")
        .unwrap_or_else(|| panic!("revision 2 body missing from tail: {stdout}"));
    let recovery_pos = stdout
        .find("Most recent judgment entry.")
        .unwrap_or_else(|| panic!("revision 3 body missing from tail: {stdout}"));
    assert!(
        judgment_pos < replan_pos && replan_pos < recovery_pos,
        "tail must render oldest-of-the-tail first: {stdout}"
    );
    assert!(stdout.contains("[judgment]"), "stdout: {stdout}");
    assert!(stdout.contains("[replan]"), "stdout: {stdout}");
    assert!(stdout.contains("[recovery]"), "stdout: {stdout}");
}
