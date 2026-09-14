use super::*;

/// W3/W4 — one reader for Work.
///
/// A Work's version chain is one chain, written to the `work` aggregate of the
/// trust journal and read from both it and the pre-cutover
/// `work_operations.jsonl`. Before W3 every consumer that read only the ledger
/// saw half of it. This test pins the three consequences a Host actually
/// feels, on a fixture whose Works span both journals:
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
    // Since the W4 writer cutover no current command produces a ledger row, so
    // the mixed store this reader exists for is staged deliberately: one Work
    // whose only revision is a pre-cutover `work_operations.jsonl` row.
    let legacy_work_id = stage_legacy_ledger_work(&fixture, "legacy-ledger-only-work");
    let tip = list_since(&tip.to_string())["next_since"]
        .as_u64()
        .expect("next_since after the staged legacy row");
    let legacy_cursor = tip % (1u64 << 32);
    assert_eq!(
        legacy_cursor, 1,
        "exactly the one staged legacy ledger row: {legacy_cursor}"
    );
    let from_legacy = changed_ids(&list_since(&legacy_cursor.to_string()));
    assert!(
        !from_legacy.contains(&legacy_work_id),
        "a ledger-only Work at or below the cursor is not replayed: {from_legacy:?}"
    );
    assert!(
        from_legacy.contains(&fixture.work_review_id),
        "a Work whose latest change is a trust transition is reported: {from_legacy:?}"
    );
    assert!(
        changed_ids(&list_since("0")).contains(&legacy_work_id),
        "and a cursor before every row still reports the legacy Work"
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

    // ---- 3b. one journal: every revision is a `work` envelope ---------
    // The file-level half of the W4 claim, on the same real-CLI store the
    // reader assertions above just ran against: a whole lifecycle
    // (create, assign, start, block, submit, accept, cancel) has been driven
    // through the real binary, and every revision it produced is a canonical
    // `work` transition carrying its complete WorkOperation.
    let space = fixture.home.spaces_dir().join(&fixture.project_id);
    let trust_rows = std::fs::read_to_string(space.join("agentfirm_trust_operations.jsonl"))
        .expect("canonical trust ledger")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|row| row["operation"]["event"]["aggregate_kind"] == "work")
        .collect::<Vec<_>>();
    let mut transitions = trust_rows
        .iter()
        .map(|row| {
            row["operation"]["event"]["transition"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect::<Vec<_>>();
    transitions.sort();
    transitions.dedup();
    for expected in [
        "created",
        "assigned",
        "started",
        "blocked",
        "submitted",
        "accepted",
        "cancelled",
    ] {
        assert!(
            transitions.iter().any(|name| name == expected),
            "the lifecycle committed a `work`/{expected} envelope: {transitions:?}"
        );
    }
    for row in &trust_rows {
        let event = &row["operation"]["event"];
        let records = row["operation"]["immutable_side_records"]
            .as_array()
            .expect("side records");
        // A transition ported from the ledger carries the whole WorkOperation,
        // because that is what the ledger row carried. The trust entrances that
        // already owned their transition before W4 — submitted, accepted,
        // cancelled, dependencies_changed — carry the WorkEvent beside the
        // projection that produced it, unchanged.
        let carries_operation = records.iter().any(|record| {
            record["work"]["id"] == event["aggregate_id"]
                && record["work"]["version"] == event["resulting_version"]
                && record["event"]["kind"].is_string()
        });
        let carries_event = records.iter().any(|record| {
            record["work_id"] == event["aggregate_id"]
                && record["resulting_version"] == event["resulting_version"]
                && record["kind"].is_string()
        });
        assert!(
            carries_operation || carries_event,
            "`work`/{} must carry the WorkEvent for its revision: {event}",
            event["transition"]
        );
        if matches!(
            event["transition"].as_str(),
            Some("created" | "assigned" | "started" | "blocked" | "resumed")
        ) {
            assert!(
                carries_operation,
                "a ported ledger transition carries its whole WorkOperation: {event}"
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(space.join("work_operations.jsonl"))
            .expect("the legacy ledger holds exactly the staged row")
            .lines()
            .count(),
        1,
        "no verb in that lifecycle appended to the legacy Work ledger"
    );

    // ---- 4. a LEDGER row after a TRUST row for the same Work -----------
    // This is the exact shape seam F got wrong. The RoleView fold started from
    // the merged latest Work and then re-derived Work from canonical envelopes
    // in APPEND order, so a trust revision that is older than the Work's newest
    // ledger revision overwrote it: the panel showed `open` for a Work the
    // store had already assigned.
    let ledger_after_trust = create_fixture_work(
        &fixture.home,
        &fixture.project_id,
        &fixture.run_id,
        "Ledger row after a trust row",
        None,
    );
    // Trust first: a dependency change is written only to the trust journal.
    team_run_json(
        &fixture.home,
        &fixture.project_id,
        &[
            "work",
            "replace-dependencies",
            "--team-id",
            FIXTURE_TEAM_ID,
            "--work-id",
            &ledger_after_trust,
            "--expected-version",
            "1",
            "--prerequisite-work-id",
            &fixture.work_done_id,
        ],
    );
    // Then a ledger row, at a HIGHER Work version than the trust envelope.
    let assigned = firm_env::work_execution::assign_work_for_member_run(
        &fixture.home,
        &fixture.project_id,
        &ledger_after_trust,
        &fixture.alice_member_run_id,
        false,
    );
    assert_eq!(assigned.version, 3);
    assert_eq!(assigned.phase, harness_core::WorkPhase::Open);
    let membership_id = assigned
        .assignee_membership_id
        .clone()
        .expect("the ledger row is the newer revision and carries the assignee");

    // Every reader must report that newer ledger revision. An append-order
    // fold cannot: the trust envelope is the LAST canonical row written for
    // this Work, so it would win and report version 2 with no assignee.
    let history = show(&ledger_after_trust);
    assert_eq!(history["work"]["version"].as_u64(), Some(3));
    assert_eq!(
        history["work"]["assignee_membership_id"].as_str(),
        Some(membership_id.as_str())
    );
    let kinds_seen = kinds(&history);
    assert_eq!(
        kinds_seen,
        vec!["created", "dependencies_changed", "assigned"],
        "the one chain is in version order across both journals: {history}"
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
        .find(|work| work["id"].as_str() == Some(ledger_after_trust.as_str()))
        .expect("listed Work")
        .clone();
    assert_eq!(listed_work["version"].as_u64(), Some(3));
    assert_eq!(
        listed_work["assignee_membership_id"].as_str(),
        Some(membership_id.as_str())
    );

    let view = run_firm(
        &fixture.home,
        fixture.home.base(),
        &["--project", &fixture.project_id, "work", "list"],
    );
    let view: serde_json::Value =
        serde_json::from_slice(&view.stdout).expect("Global Work RoleView JSON");
    let role_view_work = view["result"]["data"]["items"]
        .as_array()
        .expect("RoleView works")
        .iter()
        .find(|work| work["work_id"].as_str() == Some(ledger_after_trust.as_str()))
        .expect("RoleView Work")
        .clone();
    assert_eq!(
        role_view_work["work_revision"].as_u64(),
        Some(3),
        "the RoleView reports the newer LEDGER revision, not the later-appended trust envelope: {role_view_work}"
    );
    assert_eq!(
        role_view_work["assignee_membership_id"].as_str(),
        Some(membership_id.as_str()),
        "an append-order fold would have reported no assignee here: {role_view_work}"
    );
    assert_eq!(
        role_view_work["latest_event"]["kind"].as_str(),
        Some("assigned"),
        "and its latest event is the ledger one: {role_view_work}"
    );
}

/// Append one pre-cutover `work_operations.jsonl` row, cloned from a Work
/// revision this fixture already committed to the trust journal.
///
/// Cloning the persisted shape rather than hand-writing it keeps the staged
/// row a real legacy row — every field a pre-cutover binary wrote, none this
/// one invented — which is the only thing that makes the mixed-journal
/// assertions above worth anything.
fn stage_legacy_ledger_work(fixture: &BoardReadFixture, work_id: &str) -> String {
    let space = fixture.home.spaces_dir().join(&fixture.project_id);
    let source = std::fs::read_to_string(space.join("agentfirm_trust_operations.jsonl"))
        .expect("canonical trust ledger")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|row| {
            row["operation"]["event"]["aggregate_kind"] == "work"
                && row["operation"]["event"]["transition"] == "created"
        })
        .find_map(|row| {
            row["operation"]["immutable_side_records"]
                .as_array()?
                .iter()
                .find(|record| record["work"]["version"] == 1)
                .cloned()
        })
        .expect("a committed creation WorkOperation to clone");
    let mut operation = source;
    operation["work"]["id"] = work_id.into();
    operation["work"]["title"] = "Legacy ledger-only Work".into();
    operation["event"]["id"] = format!("legacy-event-{work_id}").into();
    operation["event"]["work_id"] = work_id.into();
    operation["event"]["idempotency_key"] = format!("legacy-key-{work_id}").into();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(space.join("work_operations.jsonl"))
        .expect("open the legacy Work ledger");
    use std::io::Write;
    writeln!(file, "{operation}").expect("append the legacy Work row");
    work_id.to_string()
}
