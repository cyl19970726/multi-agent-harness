use super::*;
use firm_core::WorkEventKind;

fn host_work_context(
    store: &HarnessStore,
    run_id: &str,
    event_id: &str,
    key: &str,
) -> firm_core::WorkCommandContext {
    let actor = store.exact_team_run_host_actor(run_id).unwrap();
    firm_core::WorkCommandContext {
        event_id: event_id.into(),
        performed_by_actor: actor.clone(),
        authority_actor: Some(actor),
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: "t-journal".into(),
        duplicate_ok: false,
    }
}

fn kinds(records: &[crate::WorkJournalRecord]) -> Vec<WorkEventKind> {
    records.iter().map(|record| record.event.kind).collect()
}

/// W3: the Store exposes one Work reader over both journals.
///
/// `work_operations.jsonl` carries Created/Assigned/... while Cancelled and
/// DependenciesChanged are canonical trust transitions. Before this slice no
/// writer ever constructed `WorkEventKind::DependenciesChanged`, and every
/// ledger-only reader — `work_events`, the `--since` cursor, the canonical
/// state fingerprint, the dashboard cursor — was blind to both.
#[test]
fn work_journal_is_one_reader_across_both_journals() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-journal", "team-run-journal");
    let prerequisite = insert_runtime_work(
        &store,
        "work-journal-prerequisite",
        "team-journal",
        "team-run-journal",
    );
    let work = insert_runtime_work(&store, "work-journal-a", "team-journal", "team-run-journal");

    // Both Works exist only in the ledger so far.
    let ledger_only = store.work_journal_position().unwrap();
    assert_eq!(
        ledger_only.trust, 0,
        "no trust Work transition yet: {ledger_only:?}"
    );
    assert_eq!(
        ledger_only.packed().unwrap(),
        ledger_only.ledger,
        "a ledger-only position packs to the bare row count a pre-W3 cursor carried"
    );
    assert_eq!(
        kinds(&store.work_history(&work.id).unwrap()),
        [WorkEventKind::Created]
    );

    // A dependency change is written to the trust journal only.
    let changed = store
        .replace_work_dependencies(
            &work.id,
            work.version,
            vec![prerequisite.id.clone()],
            host_work_context(&store, "team-run-journal", "event-deps", "deps-journal-a"),
        )
        .unwrap();
    assert_eq!(changed.version, work.version + 1);

    let after_dependencies = store.work_journal_position().unwrap();
    assert_eq!(
        after_dependencies.ledger, ledger_only.ledger,
        "a dependency change appends no ledger row: {after_dependencies:?}"
    );
    assert_eq!(
        after_dependencies.trust,
        ledger_only.trust + 1,
        "it advances the trust component instead: {after_dependencies:?}"
    );
    assert!(
        after_dependencies.advanced_past(&ledger_only),
        "and therefore advances past the previous position"
    );
    assert!(
        after_dependencies.packed().unwrap() > ledger_only.packed().unwrap(),
        "a trust-bearing position orders after every ledger-only value"
    );

    // The transition is a real, named Work event in both per-Work history and
    // the store-wide event read.
    assert_eq!(
        kinds(&store.work_history(&work.id).unwrap()),
        [WorkEventKind::Created, WorkEventKind::DependenciesChanged],
        "one Work's history spans both journals, strictly in version order"
    );
    let dependencies_event = store
        .work_events()
        .unwrap()
        .into_iter()
        .find(|event| event.work_id == work.id && event.kind == WorkEventKind::DependenciesChanged)
        .expect("the dependency change is a Work event");
    assert_eq!(dependencies_event.expected_version, work.version);
    assert_eq!(dependencies_event.resulting_version, changed.version);
    assert_eq!(dependencies_event.team_run_id, "team-run-journal");

    // A cancellation is the second trust transition, on the other Work.
    store
        .cancel_work(
            &prerequisite.id,
            prerequisite.version,
            "no longer required",
            host_work_context(
                &store,
                "team-run-journal",
                "event-cancel",
                "cancel-journal-a",
            ),
        )
        .unwrap();

    // Run cursors advance per journal, and a Work with no trust row is never
    // hidden behind another Work's trust row.
    let cursors = store
        .work_journal_cursors_for_team_run("team-run-journal")
        .unwrap();
    assert_eq!(cursors.watermark.trust, 2, "two trust transitions");
    assert_eq!(
        cursors.watermark.ledger, after_dependencies.ledger,
        "and the ledger component is unchanged"
    );
    assert!(
        cursors.changed_after(&work.id, &ledger_only),
        "the dependency change is visible to a cursor taken before it"
    );
    assert!(
        cursors.changed_after(&prerequisite.id, &ledger_only),
        "so is the cancellation"
    );
    assert!(
        !cursors.changed_after(&work.id, &cursors.watermark),
        "nothing changed after the current watermark"
    );
    assert!(
        cursors.by_work.get(&work.id).unwrap().ledger > 0,
        "a Work keeps its ledger position alongside its trust position"
    );

    // A pre-W3 integer cursor decodes to exactly the ledger position it named.
    let legacy = crate::WorkJournalPosition::from_packed(ledger_only.ledger);
    assert_eq!(legacy, ledger_only);
    assert!(
        cursors.changed_after(&work.id, &legacy),
        "a legacy cursor still surfaces the trust transition it could never see"
    );

    // The store-wide event read is deterministic and complete.
    let all = store.work_journal_records().unwrap();
    assert_eq!(
        all.len() as u64,
        store.work_journal_position().unwrap().total(),
        "the position counts exactly the records the reader returns"
    );
    assert_eq!(
        all,
        store.work_journal_records().unwrap(),
        "the total order is deterministic across reads"
    );

    // A physical store may temporarily hold more than one Execution Space
    // during recovery or import. Append the cancellation again under a second
    // space, at a higher Work version, and prove every space-scoped reader
    // ignores it. These are the exact three functions the acceptance wake, the
    // RoleView `Facts` fold and `work_action_service::current_work` call.
    append_foreign_space_work_envelope(&root, &prerequisite.id, "space-other");
    let scoped = store.work_journal_records_for_space("space-test").unwrap();
    assert!(
        scoped
            .iter()
            .all(|record| record.execution_space_id.as_deref() != Some("space-other")),
        "a space-scoped read never folds another scope's trust truth"
    );
    assert_eq!(
        scoped.len(),
        all.len(),
        "and it still returns every record of its own scope, ledger rows included"
    );
    assert!(
        store
            .work_journal_records()
            .unwrap()
            .iter()
            .any(|record| record.execution_space_id.as_deref() == Some("space-other")),
        "the unscoped read is the one that sees the whole store"
    );

    let work_ids = std::collections::HashSet::from([prerequisite.id.clone()]);
    let foreign_version = store
        .current_work(&prerequisite.id)
        .unwrap()
        .expect("the unscoped fold sees the foreign revision")
        .version;
    assert_eq!(
        store
            .current_work_in_space("space-test", &prerequisite.id)
            .unwrap()
            .expect("the Work still exists in its own space")
            .version,
        foreign_version - 1,
        "current_work_in_space must not adopt another scope's higher revision"
    );
    assert!(
        store
            .work_journal_events_for_ids_in_space_unlocked("space-test", &work_ids)
            .unwrap()
            .iter()
            .all(|event| event.resulting_version < foreign_version),
        "the acceptance wake's scan never sees another scope's Work event"
    );
}

/// Append a duplicate of one Work's latest canonical envelope under a second
/// Execution Space, one revision higher. Raw-row fixtures are how this suite
/// already stages pre-existing canonical shapes it has no writer for.
fn append_foreign_space_work_envelope(root: &std::path::Path, work_id: &str, space: &str) {
    let ledger = root.join("agentfirm_trust_operations.jsonl");
    let contents = std::fs::read_to_string(&ledger).expect("read canonical trust ledger");
    let mut rows = contents.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut foreign: serde_json::Value = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .rfind(|row| {
            row["operation"]["event"]["aggregate_kind"] == "work"
                && row["operation"]["event"]["aggregate_id"] == work_id
        })
        .expect("one canonical Work envelope to duplicate");
    let version = foreign["operation"]["event"]["resulting_version"]
        .as_u64()
        .expect("resulting version")
        + 1;
    let sequence = rows.len() as u64 + 1;
    foreign["execution_space_id"] = serde_json::json!(space);
    foreign["operation"]["event"]["id"] = serde_json::json!(format!("trust-event-foreign-{space}"));
    foreign["operation"]["event"]["idempotency_key"] =
        serde_json::json!(format!("foreign-{space}-{work_id}"));
    foreign["operation"]["event"]["store_sequence"] = serde_json::json!(sequence);
    foreign["operation"]["event"]["expected_version"] = serde_json::json!(version - 1);
    foreign["operation"]["event"]["resulting_version"] = serde_json::json!(version);
    foreign["operation"]["resulting_projection"]["version"] = serde_json::json!(version);
    foreign["operation"]["immutable_side_records"] = serde_json::json!([]);
    foreign["operation"]["initial_outbox_records"] = serde_json::json!([]);
    rows.push(serde_json::to_string(&foreign).expect("serialize foreign trust row"));
    std::fs::write(&ledger, format!("{}\n", rows.join("\n")))
        .expect("append the foreign-space trust row");
}
