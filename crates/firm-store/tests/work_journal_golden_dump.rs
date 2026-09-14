//! Dump the Work reader's complete answer for a store, as canonical JSON.
//!
//! Run with `W4_GOLDEN_STORE=<path to an Execution Space store>` to print the
//! dump for that store. Used to prove that a dogfood-shaped store — both
//! journals populated — reads identically before and after the W4 writer
//! cutover: the same dump is produced by a binary built at the merge base and
//! by this one, and the two are diffed.
use firm_store::HarnessStore;

#[test]
fn w4_golden_dump() {
    let Ok(root) = std::env::var("W4_GOLDEN_STORE") else {
        eprintln!("W4_GOLDEN_STORE not set; nothing to dump");
        return;
    };
    let store = HarnessStore::new(std::path::Path::new(&root));
    let mut works = store.latest_works().expect("latest works");
    works.sort_by(|a, b| a.id.cmp(&b.id));
    let mut out = Vec::new();
    for work in &works {
        let history = store.work_history(&work.id).expect("work history");
        out.push(serde_json::json!({
            "id": work.id,
            "phase": work.phase,
            "condition": work.condition,
            "resolution": work.resolution,
            "version": work.version,
            "team_run_id": work.team_run_id,
            "accountable_team_id": work.accountable_team_id,
            "assignee_membership_id": work.assignee_membership_id,
            "owner_member_id": work.owner_member_id,
            "history": history.iter().map(|record| serde_json::json!({
                "source": record.source,
                "space": record.execution_space_id,
                "event_id": record.event.id,
                "kind": record.event.kind,
                "sequence": record.event.sequence,
                "expected_version": record.event.expected_version,
                "resulting_version": record.event.resulting_version,
                "performer_kind": record.event.performed_by_actor.kind,
                "performer_id": record.event.performed_by_actor.id,
                "work_version": record.work.version,
                "work_phase": record.work.phase,
            })).collect::<Vec<_>>(),
        }));
    }
    let position = store.work_journal_position().expect("journal position");
    let summary = serde_json::json!({
        "works": out,
        "work_count": works.len(),
        "journal_records": store.work_journal_records().expect("journal records").len(),
        "events": store.work_events().expect("work events").len(),
        "position_ledger": position.ledger,
        "position_trust": position.trust,
        "position_packed": position.packed().expect("packed cursor"),
    });
    println!(
        "W4_GOLDEN_BEGIN\n{}\nW4_GOLDEN_END",
        serde_json::to_string_pretty(&summary).expect("dump JSON")
    );
}
