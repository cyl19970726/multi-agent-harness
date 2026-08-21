use super::*;

#[test]
#[ignore = "retired projection-message Handoff contract; canonical result submission is idempotently fenced by WorkReport"]
fn concurrent_same_turn_handoffs_allow_exactly_one_append() {
    let root = team_test_root("same-turn-handoff");
    let store = Arc::new(HarnessStore::new(&root));
    let assignment = TeamMessageProjection {
        id: "tm-assignment".into(),
        team_run_id: "tr-converge".into(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "host".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["mr-codex".into()],
        kind: ProviderDispatchIntent::Message,
        body: "Review the convergence fix".into(),
        correlation_id: "corr-converge".into(),
        causation_id: None,
        response_intent: None,
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: "mr-codex".into(),
            policy: TeamDeliveryPolicy::Queue,
            status: TeamDeliveryStatus::Delivered,
            attempt: 1,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: Some("codex-turn-1".into()),
            failure_reason: None,
            updated_at: "unix-ms:1".into(),
        }],
        created_at: "unix-ms:1".into(),
    };
    store
        .append_team_message_checked(&assignment)
        .expect("append conversation anchor");
    let handoff = TeamMessageProjection {
        id: "tm-handoff-a".into(),
        team_run_id: assignment.team_run_id.clone(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "mr-codex".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["host".into()],
        kind: ProviderDispatchIntent::Message,
        body: "## RESULT\ndone".into(),
        correlation_id: assignment.correlation_id.clone(),
        causation_id: Some(assignment.id.clone()),
        response_intent: None,
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: "host".into(),
            policy: TeamDeliveryPolicy::ManualAck,
            status: TeamDeliveryStatus::Delivered,
            attempt: 1,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: Some("harness-control-plane".into()),
            failure_reason: None,
            updated_at: "unix-ms:2".into(),
        }],
        created_at: "unix-ms:2".into(),
    };
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["tm-handoff-a", "tm-handoff-b"]
        .into_iter()
        .map(|id| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let mut candidate = handoff.clone();
            candidate.id = id.into();
            std::thread::spawn(move || {
                barrier.wait();
                store.append_team_message_checked(&candidate)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("handoff writer"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let conflict = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .expect("one same-turn conflict");
    assert!(conflict.to_string().contains("already handed off"));
    assert_eq!(
        store
            .legacy_team_messages()
            .expect("messages")
            .into_iter()
            .filter(|message| message.kind == ProviderDispatchIntent::Message)
            .count(),
        1
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
