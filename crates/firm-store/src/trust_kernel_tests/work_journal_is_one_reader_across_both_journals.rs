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
    let (store, _root) = fabric_store();
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
        ledger_only.packed(),
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
        after_dependencies.packed() > ledger_only.packed(),
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
}
