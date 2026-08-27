use super::*;

#[test]
fn work_list_brief_prints_one_stable_line_per_work_with_truncated_title() {
    let fixture = seed_board_read_fixture("work-brief");
    let out = run_firm(
        &fixture.home,
        fixture.home.base(),
        &[
            "--project",
            &fixture.project_id,
            "team-run",
            "work",
            "list",
            "--team-run-id",
            &fixture.run_id,
            "--brief",
        ],
    );
    assert!(
        out.status.success(),
        "work list --brief failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 6, "one brief line per Work: {lines:?}");
    assert!(
        lines.iter().all(|line| line.starts_with("work-")),
        "brief output must be plain text with no JSON wrapper: {lines:?}"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "brief output must not be JSON: {stdout:?}"
    );

    let field_of = |work_id: &str| -> Vec<String> {
        let line = lines
            .iter()
            .find(|line| line.starts_with(&format!("{work_id}  ")))
            .unwrap_or_else(|| panic!("no brief line for {work_id}: {lines:?}"));
        line.split("  ").map(str::to_string).collect()
    };

    // <work-id>  <status>  <owner-agent-member-id|unassigned>  v<version>  <title>
    let open_fields = field_of(&fixture.work_open_id);
    assert_eq!(open_fields[0], fixture.work_open_id);
    assert_eq!(open_fields[1], "open");
    assert_eq!(open_fields[2], "unassigned");
    assert_eq!(open_fields[3], "v1");
    assert_eq!(
        open_fields[4].chars().count(),
        60,
        "title over 60 chars must be hard-truncated to exactly 60: {:?}",
        open_fields[4]
    );

    let in_progress_fields = field_of(&fixture.work_in_progress_id);
    assert_eq!(in_progress_fields[1], "active");
    assert_eq!(in_progress_fields[2], fixture.alice_agent_member_id);
    assert_eq!(in_progress_fields[3], "v3");
    assert_eq!(in_progress_fields[4], "In-progress Work");

    let review_fields = field_of(&fixture.work_review_id);
    assert_eq!(review_fields[1], "review");
    assert_eq!(review_fields[2], fixture.bob_agent_member_id);
    assert_eq!(review_fields[3], "v4");

    let blocked_fields = field_of(&fixture.work_blocked_id);
    assert_eq!(blocked_fields[1], "blocked");
    assert_eq!(blocked_fields[2], fixture.alice_agent_member_id);
    assert_eq!(blocked_fields[3], "v4");

    let done_fields = field_of(&fixture.work_done_id);
    assert_eq!(done_fields[1], "accepted");
    assert_eq!(done_fields[2], fixture.bob_agent_member_id);
    assert_eq!(done_fields[3], "v5");

    let cancelled_fields = field_of(&fixture.work_cancelled_id);
    assert_eq!(cancelled_fields[1], "cancelled");
    assert_eq!(cancelled_fields[2], "unassigned");
    assert_eq!(cancelled_fields[3], "v2");
}
