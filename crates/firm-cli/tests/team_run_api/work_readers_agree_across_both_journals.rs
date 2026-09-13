use super::*;

/// W3 — one reader for Work.
///
/// A Work's version chain is one chain, but its rows live in two journals
/// until the W4 writer cutover: `work_operations.jsonl` and the `work`
/// aggregate of the trust journal. Before this slice every consumer that read
/// only the ledger saw half of it. This test pins the three consequences a
/// Host actually feels, on a fixture whose Works span both journals:
///
/// 1. `work show` names the trust transitions (`submitted`, `accepted`,
///    `cancelled`) — they had no writer at all and no reader could see them.
/// 2. `work list --since` advances for a Work whose ONLY change was a trust
///    transition, and an integer cursor issued before W3 still decodes.
/// 3. `team-run work show`, `team-run work list` and the RoleView projection
///    (`firm work list`, the DOC-106 Global Work read over the same RoleView
///    `Facts` fold the HTTP RoleViews serve) all report the same version and
///    phase for the same Work. That fold used to re-derive Work from canonical
///    envelopes in APPEND order on top of the merged one, so a trust revision
///    could overwrite a newer ledger revision (architecture seam F). The
///    dashboard's `latest_op_seq` is covered in `dashboard_meta_api`.
#[test]
fn work_readers_agree_across_both_journals() {
    let fixture = seed_board_read_fixture("work-readers-agree");

    // ---- 1. trust transitions are named Work events -------------------
    let show = |work_id: &str| -> serde_json::Value {
        team_run_json(
            &fixture.home,
            &fixture.project_id,
            &["work", "show", "--work-id", work_id],
        )
    };
    let kinds = |show: &serde_json::Value| -> Vec<String> {
        show["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|event| event["kind"].as_str().unwrap_or_default().to_string())
            .collect()
    };

    let accepted_show = show(&fixture.work_done_id);
    let accepted_kinds = kinds(&accepted_show);
    for expected in ["created", "assigned", "started", "submitted", "accepted"] {
        assert!(
            accepted_kinds.iter().any(|kind| kind == expected),
            "an accepted Work's history must name {expected}: {accepted_kinds:?}"
        );
    }
    let versions = accepted_show["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["resulting_version"].as_u64().unwrap_or_default())
        .collect::<Vec<_>>();
    let mut sorted = versions.clone();
    sorted.sort_unstable();
    assert_eq!(versions, sorted, "one Work's history is in version order");
    assert_eq!(
        versions.last().copied(),
        accepted_show["work"]["version"].as_u64(),
        "the last event is the current revision: {accepted_show}"
    );

    let cancelled_show = show(&fixture.work_cancelled_id);
    assert!(
        kinds(&cancelled_show)
            .iter()
            .any(|kind| kind == "cancelled"),
        "a cancellation is a Work event: {cancelled_show}"
    );

    // ---- 2. the cursor advances on a trust transition -----------------
    let list_since = |cursor: &str| -> serde_json::Value {
        team_run_json(
            &fixture.home,
            &fixture.project_id,
            &[
                "work",
                "list",
                "--team-run-id",
                &fixture.run_id,
                "--since",
                cursor,
            ],
        )
    };
    let changed_ids = |delta: &serde_json::Value| -> Vec<String> {
        delta["works"]
            .as_array()
            .expect("works")
            .iter()
            .map(|work| work["id"].as_str().unwrap_or_default().to_string())
            .collect()
    };

    let baseline = list_since("0");
    assert_eq!(
        changed_ids(&baseline).len(),
        6,
        "since=0 returns every Work: {baseline}"
    );
    let watermark = baseline["next_since"].as_u64().expect("next_since");

    // Accept the Work sitting in review. Acceptance is written only to the
    // trust journal, so a ledger-only cursor could never report it.
    team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "accept",
            "--team-run-id",
            &fixture.run_id,
            "--work-id",
            &fixture.work_review_id,
            "--expected-version",
            "4",
        ],
    );
    let after_accept = list_since(&watermark.to_string());
    assert_eq!(
        changed_ids(&after_accept),
        vec![fixture.work_review_id.clone()],
        "an accept advances only the accepted Work's cursor: {after_accept}"
    );
    let after_accept_watermark = after_accept["next_since"].as_u64().expect("next_since");
    assert!(
        after_accept_watermark > watermark,
        "the watermark is monotonic: {after_accept_watermark} must exceed {watermark}"
    );
    // A dependency change is the third trust-journal transition.
    team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "replace-dependencies",
            "--team-id",
            FIXTURE_TEAM_ID,
            "--work-id",
            &fixture.work_open_id,
            "--expected-version",
            "1",
            "--prerequisite-work-id",
            &fixture.work_done_id,
        ],
    );
    let after_dependencies = list_since(&after_accept_watermark.to_string());
    assert_eq!(
        changed_ids(&after_dependencies),
        vec![fixture.work_open_id.clone()],
        "a dependency change advances that Work's cursor: {after_dependencies}"
    );
    let after_dependencies_watermark = after_dependencies["next_since"]
        .as_u64()
        .expect("next_since");

    // A cancellation, and then the chained cursor sees nothing new.
    team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "cancel",
            "--team-run-id",
            &fixture.run_id,
            "--work-id",
            &fixture.work_open_id,
            "--expected-version",
            "2",
            "--reason",
            "superseded by the accepted Work",
        ],
    );
    let after_cancel = list_since(&after_dependencies_watermark.to_string());
    assert_eq!(
        changed_ids(&after_cancel),
        vec![fixture.work_open_id.clone()],
        "a cancellation advances that Work's cursor: {after_cancel}"
    );
    let tip = after_cancel["next_since"].as_u64().expect("next_since");
    let idle = list_since(&tip.to_string());
    assert!(
        changed_ids(&idle).is_empty(),
        "the delta read is idempotent at the tip: {idle}"
    );
    assert_eq!(idle["next_since"].as_u64(), Some(tip));

    // A pre-W3 cursor is a bare ledger row count and must still be accepted.
    // At the tip's own ledger count it names every ledger row already written,
    // so a Work whose latest change is a ledger row is not replayed; the
    // Works whose latest change is a trust transition come back, which is the
    // correction this slice exists to make.
    let legacy_cursor = tip % (1u64 << 32);
    assert!(
        legacy_cursor > 0,
        "the fixture writes ledger rows: {legacy_cursor}"
    );
    let from_legacy = changed_ids(&list_since(&legacy_cursor.to_string()));
    assert!(
        !from_legacy.contains(&fixture.work_in_progress_id),
        "a ledger-only Work at or below the cursor is not replayed: {from_legacy:?}"
    );
    assert!(
        from_legacy.contains(&fixture.work_review_id),
        "a Work whose latest change is a trust transition is reported: {from_legacy:?}"
    );

    // ---- 3. every reader reports the same version and phase -----------
    let work_id = fixture.work_review_id.clone();
    let accepted = show(&work_id);
    let expected_version = accepted["work"]["version"].as_u64().expect("version");
    let expected_phase = accepted["work"]["phase"]
        .as_str()
        .expect("phase")
        .to_string();
    assert_eq!(
        expected_phase, "closed",
        "the Work was accepted: {accepted}"
    );

    let listed = team_run_json(
        &fixture.home,
        &fixture.project_id,
        &["work", "list", "--team-run-id", &fixture.run_id],
    );
    let listed_work = listed
        .as_array()
        .expect("works")
        .iter()
        .find(|work| work["id"].as_str() == Some(work_id.as_str()))
        .expect("listed Work")
        .clone();
    assert_eq!(listed_work["version"].as_u64(), Some(expected_version));
    assert_eq!(listed_work["phase"].as_str(), Some(expected_phase.as_str()));

    // `firm work list` is the DOC-106 Global Work read: the same RoleView
    // `Facts` fold and the same Work item projection the HTTP RoleViews serve,
    // read in process.
    let view = run_firm(
        &fixture.home,
        fixture.home.base(),
        &["--project", &fixture.project_id, "work", "list"],
    );
    assert!(
        view.status.success(),
        "work list failed: {}",
        String::from_utf8_lossy(&view.stderr)
    );
    let view: serde_json::Value =
        serde_json::from_slice(&view.stdout).expect("Global Work RoleView JSON");
    let role_view_work = view["result"]["data"]["items"]
        .as_array()
        .expect("RoleView works")
        .iter()
        .find(|work| work["work_id"].as_str() == Some(work_id.as_str()))
        .expect("RoleView Work")
        .clone();
    assert_eq!(
        role_view_work["work_revision"].as_u64(),
        Some(expected_version),
        "the RoleView reads the same merged fold: {role_view_work}"
    );
    assert_eq!(
        role_view_work["phase"].as_str(),
        Some(expected_phase.as_str()),
        "the RoleView reads the same merged fold: {role_view_work}"
    );
    assert_eq!(
        role_view_work["latest_event"]["kind"].as_str(),
        Some("accepted"),
        "the RoleView's latest event follows the version chain, not append order: {role_view_work}"
    );
}
