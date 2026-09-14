use super::*;

/// The Work-ledger delegation stack is retired, and retirement must never cost
/// a store its history. Two shapes exist in the wild and both must stay
/// readable: a `WorkOperation` row that still carries `delegation_revisions`,
/// and a store directory that still holds the two delegation ledgers.
///
/// `WorkOperation` is not `deny_unknown_fields`, so the field folds away
/// without a deprecated placeholder or a custom deserializer; nothing writes it
/// back. The delegation files are simply never read: they are left on disk,
/// their rows never become Work, and every current reader still answers.
#[test]
fn retired_delegation_files_are_ignored_and_legacy_rows_still_fold() {
    let (_root, store, run, _member, _) = work_test_fixture("retired-delegation-tolerance");
    let store = &store;
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-legacy-delegated"),
            host_work_context("tolerance-create", "tolerance-create", "unix-ms:1"),
        )
        .expect("seed one ordinary Work");

    // 1. A legacy row carrying the retired field still decodes, and the field
    //    does not survive a round trip.
    let mut row = serde_json::to_value(WorkOperation {
        event: firm_core::WorkEvent {
            id: "legacy-event-delegated".into(),
            team_run_id: run.id.clone(),
            work_id: work.id.clone(),
            sequence: 1,
            kind: WorkEventKind::Created,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: work.created_by_actor.clone(),
            authority_actor: None,
            causation_ref: None,
            idempotency_key: "legacy-create-delegated".into(),
            payload: serde_json::Value::Null,
            created_at: "t0".into(),
            executed_by_member_run_id: None,
        },
        work: work.clone(),
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
    })
    .expect("operation JSON");
    row["delegation_revisions"] = serde_json::json!([{
        "delegation": {
            "id": "delegation-legacy-1",
            "source_work_ref": {"team_run_id": run.id, "work_id": work.id},
            "source_work_version": 1,
            "source_owner_member_id": "agent-owner",
            "target_agent_team_id": "team-b",
            "target_work_ref": {"team_run_id": "run-b", "work_id": "target-1"},
            "delegated_by_actor": work.created_by_actor,
            "state": "active",
            "version": 1,
            "created_at": "t0",
            "updated_at": "t0"
        },
        "event": {"id": "delegation-event-1", "delegation_id": "delegation-legacy-1"}
    }]);
    let folded: WorkOperation =
        serde_json::from_value(row).expect("a legacy row with delegation_revisions still folds");
    assert_eq!(folded.work.id, work.id);
    assert!(
        serde_json::to_value(&folded)
            .expect("re-serialize")
            .get("delegation_revisions")
            .is_none(),
        "nothing writes the retired field back"
    );

    // 2. A store directory that still holds both delegation ledgers opens, and
    //    their rows never become Work.
    for ledger in [
        "work_delegation_operations.jsonl",
        "work_delegation_events.jsonl",
    ] {
        std::fs::write(
            store.root().join(ledger),
            b"{\"delegation\":{\"id\":\"delegation-legacy-1\"},\"event\":{\"id\":\"e1\"}}\n",
        )
        .expect("seed a retired delegation ledger");
    }
    let works = store.latest_works().expect("Works still read");
    assert_eq!(
        works
            .iter()
            .map(|work| work.id.as_str())
            .collect::<Vec<_>>(),
        vec![work.id.as_str()],
        "the retired ledgers contribute no Work"
    );
    store.work_journal_records().expect("journal still reads");
    store.work_events().expect("events still read");
    for ledger in [
        "work_delegation_operations.jsonl",
        "work_delegation_events.jsonl",
    ] {
        assert!(
            store.root().join(ledger).exists(),
            "{ledger} is left exactly as the store held it"
        );
    }
}
