use super::*;

#[test]
fn sparse_mixed_version_update_recovers_and_repersists_work_provenance() {
    let (root, store, run, member, _) = work_test_fixture("sparse-rebound-provenance");

    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-sparse-rebound"),
            member_work_context(
                &member.id,
                "event-create-sparse-rebound",
                "command-create-sparse-rebound",
                "unix-ms:3",
            ),
        )
        .expect("Member creates Team-scoped Work");
    assert_eq!(
        created.accountable_team_id.as_deref(),
        Some(run.agent_team_id.as_str())
    );
    assert_eq!(
        created.created_by_member_id,
        Some(member.agent_member_id.clone())
    );

    let rebound_context = host_work_context(
        "event-sparse-mixed-writer-rebound",
        "command-sparse-mixed-writer-rebound",
        "unix-ms:5",
    );
    let mut sparse_work = created.clone();
    // Keep the accountable Team so the row passes the DOC-106 required-field
    // validation; the provenance regression under test is the dropped
    // creator provenance below.
    sparse_work.created_by_member_id = None;
    sparse_work.version += 1;
    sparse_work.updated_at = rebound_context.created_at.clone();
    let sparse_operation = WorkOperation {
        event: WorkEvent {
            id: rebound_context.event_id,
            team_run_id: sparse_work.team_run_id.clone(),
            work_id: sparse_work.id.clone(),
            sequence: 2,
            kind: WorkEventKind::Updated,
            expected_version: created.version,
            resulting_version: sparse_work.version,
            performed_by_actor: rebound_context.performed_by_actor,
            authority_actor: rebound_context.authority_actor,
            causation_ref: rebound_context.causation_ref,
            idempotency_key: rebound_context.idempotency_key,
            payload: serde_json::json!({"source":"stale_mixed_version_writer"}),
            created_at: rebound_context.created_at,
        },
        work: sparse_work,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
        decisions: Vec::new(),
        delegation_revisions: Vec::new(),
    };
    let refused = store
        .append_work_operation_unlocked(&sparse_operation)
        .expect_err("current writer must refuse provenance regression");
    assert!(refused
        .to_string()
        .contains("WORK_PROJECTION_PROVENANCE_REGRESSION"));

    // Model the already-observed stale HTTP writer: it omitted both keys
    // entirely, bypassing code this newer binary did not yet contain.
    let mut sparse_json = serde_json::to_value(&sparse_operation).expect("operation JSON");
    let sparse_projection = sparse_json["work"]
        .as_object_mut()
        .expect("Work projection object");
    sparse_projection.remove("accountable_team_id");
    sparse_projection.remove("created_by_member_id");
    store
        .append_jsonl("work_operations.jsonl", &sparse_json)
        .expect("simulate stale mixed-version append");
    let raw = store.work_operations().expect("raw WorkOperations");
    assert!(raw
        .last()
        .expect("sparse rebound")
        .work
        .accountable_team_id
        .is_none());
    assert!(raw
        .last()
        .expect("sparse rebound")
        .work
        .created_by_member_id
        .is_none());

    let recovered = store.latest_works().expect("recovered Works").remove(0);
    assert_eq!(recovered.accountable_team_id, created.accountable_team_id);
    assert_eq!(recovered.created_by_member_id, created.created_by_member_id);
    let repair_context = host_work_context(
        "event-reconcile-sparse-rebound",
        "command-reconcile-sparse-rebound",
        "unix-ms:6",
    );
    let repaired = store
        .reconcile_work_projection_provenance(
            &recovered.id,
            recovered.version,
            repair_context.clone(),
        )
        .expect("explicit reconciliation re-persists recovered provenance");
    assert_eq!(repaired.phase, WorkPhase::Open);
    assert!(repaired.active_member_run_id.is_none());
    assert_eq!(repaired.accountable_team_id, created.accountable_team_id);
    assert_eq!(repaired.created_by_member_id, created.created_by_member_id);
    assert_eq!(
        store
            .reconcile_work_projection_provenance(&recovered.id, recovered.version, repair_context,)
            .expect("repair retry is idempotent"),
        repaired
    );
    let raw = store.work_operations().expect("repaired WorkOperations");
    assert_eq!(raw.last().expect("repair operation").work, repaired);
    assert_eq!(raw.last().unwrap().event.kind, WorkEventKind::Updated);

    std::fs::remove_dir_all(root).expect("remove temp store");
}
