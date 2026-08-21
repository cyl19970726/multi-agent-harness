use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn append_and_read_team_message_jsonl() {
    let root = team_test_root("team-message");
    let store = HarnessStore::new(&root);
    let message = TeamMessageProjection {
        id: "tm-1".into(),
        team_run_id: "tr-1".into(),
        work_id: None,
        source_plan_ref: Some("wave-2".into()),
        sender: None,
        sender_runtime_id: "host".into(),
        recipients: vec![TeamRecipientRef {
            kind: TeamRecipientKind::Host,
            id: "host".into(),
        }],
        recipient_runtime_ids: vec!["mr-1".into()],
        kind: ProviderDispatchIntent::Message,
        body: "Please review task-1".into(),
        correlation_id: "corr-1".into(),
        causation_id: None,
        response_intent: None,
        evidence_refs: vec!["ev-1".into()],
        deliveries: vec![ProviderDispatchAttempt {
            member_id: "mr-1".into(),
            policy: TeamDeliveryPolicy::Inject,
            status: TeamDeliveryStatus::Delivered,
            attempt: 1,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: Some("test-receipt".into()),
            failure_reason: None,
            updated_at: "unix-ms:2".into(),
        }],
        created_at: "unix-ms:1".into(),
    };

    store
        .append_team_message(&message)
        .expect("append team message");
    append_sparse_row(
        &root,
        "team_messages.jsonl",
        r#"{"id":"tm-sparse","team_run_id":"tr-1","sender_runtime_id":"host","kind":"message","body":"hi","correlation_id":"corr-2","created_at":"unix-ms:3"}"#,
    );

    let messages = store
        .legacy_team_messages()
        .expect("read legacy team messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], message);
    let sparse = &messages[1];
    assert_eq!(sparse.id, "tm-sparse");
    assert_eq!(sparse.kind, ProviderDispatchIntent::Message);
    assert!(sparse.recipient_runtime_ids.is_empty());
    assert!(sparse.causation_id.is_none());
    assert!(sparse.evidence_refs.is_empty());
    assert!(sparse.deliveries.is_empty());

    std::fs::remove_dir_all(root).expect("remove temp store");
}
