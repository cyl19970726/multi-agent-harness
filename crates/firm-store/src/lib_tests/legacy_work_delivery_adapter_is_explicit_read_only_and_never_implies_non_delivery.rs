use super::*;

#[test]
fn legacy_work_delivery_adapter_is_explicit_read_only_and_never_implies_non_delivery() {
    let root = team_test_root("legacy-work-delivery-export");
    let store = HarnessStore::new(&root);
    let team_run_id = "legacy-only-team-run";
    let mut work = unassigned_test_work(team_run_id, "legacy-only-work");
    work.version = 1;
    work.created_at = "unix-ms:1".into();
    work.updated_at = "unix-ms:1".into();
    let actor = host_work_context("unused", "unused", "unix-ms:1").performed_by_actor;
    let operation = WorkOperation {
        event: WorkEvent {
            id: "legacy-only-event".into(),
            team_run_id: team_run_id.into(),
            work_id: work.id.clone(),
            sequence: 1,
            kind: WorkEventKind::Created,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: actor.clone(),
            authority_actor: None,
            causation_ref: None,
            idempotency_key: "legacy-only-create".into(),
            payload: serde_json::Value::Null,
            created_at: "unix-ms:1".into(),
        },
        work,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
        decisions: Vec::new(),
        deliveries: vec![ProviderWorkDispatch {
            id: "legacy-only-delivery".into(),
            work_event_id: "legacy-only-event".into(),
            team_run_id: team_run_id.into(),
            work_id: "legacy-only-work".into(),
            work_version: 1,
            recipient_member_run_id: "legacy-only-member-run".into(),
            status: ProviderWorkDispatchStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: "unix-ms:1".into(),
        }],
        delivery_updates: Vec::new(),
        delegation_revisions: Vec::new(),
    };
    store
        .append_jsonl("work_operations.jsonl", &operation)
        .expect("append frozen legacy fixture");

    let views = store
        .legacy_current_work_deliveries_for_team_run_export(team_run_id)
        .expect("explicit legacy-only adapter");
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0].authority,
        firm_application::CurrentWorkDeliveryAuthority::LegacyCompatibility
    );
    assert!(views[0].read_only);
    assert!(views[0].provider_receipt_id.is_none());
    assert!(views[0].integrity_annotations.contains(
        &firm_application::CurrentWorkDeliveryIntegrityAnnotation::ProviderReceiptAbsenceIsNotEvidenceOfNonDelivery
    ));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
