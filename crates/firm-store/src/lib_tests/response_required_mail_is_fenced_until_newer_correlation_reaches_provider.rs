use super::*;

#[test]
#[ignore = "retired projection-message Handoff contract; canonical completion is WorkReport + GateEvaluation"]
fn response_required_mail_is_fenced_until_newer_correlation_reaches_provider() {
    let root = team_test_root("handoff-mail-fence");
    let store = HarnessStore::new(&root);
    let correction = TeamMessageProjection {
        id: "tm-correction".into(),
        team_run_id: "tr-fence".into(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "host".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["mr-kimi".into()],
        kind: ProviderDispatchIntent::Message,
        body: "Use the corrected requirement".into(),
        correlation_id: "corr-fence".into(),
        causation_id: Some("tm-assignment".into()),
        // Explicit response intent: this correction must reach the
        // provider before any Handoff, so it fences (ADR 0046 §4).
        response_intent: Some(ProviderResponseIntent::ResponseRequired),
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: "mr-kimi".into(),
            policy: TeamDeliveryPolicy::Queue,
            status: TeamDeliveryStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: "unix-ms:1".into(),
        }],
        created_at: "unix-ms:1".into(),
    };
    store
        .append_team_message_checked(&correction)
        .expect("append correction");
    let handoff = TeamMessageProjection {
        id: "tm-handoff".into(),
        team_run_id: "tr-fence".into(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "mr-kimi".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["host".into()],
        kind: ProviderDispatchIntent::Message,
        body: "done".into(),
        correlation_id: "corr-fence".into(),
        causation_id: Some("tm-assignment".into()),
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
    let queued_error = store
        .append_team_message_checked(&handoff)
        .expect_err("queued correction must fence stale handoff");
    assert!(queued_error.to_string().contains("queued or claimed"));

    let mut claimed = correction.clone();
    claimed.deliveries[0].status = TeamDeliveryStatus::Claimed;
    claimed.deliveries[0].claim_id = Some("claim-1".into());
    store
        .append_team_message(&claimed)
        .expect("persist claim projection");
    let claimed_error = store
        .append_team_message_checked(&handoff)
        .expect_err("uncertain claimed correction must also fence handoff");
    assert!(claimed_error.to_string().contains("queued or claimed"));

    let mut delivered = claimed;
    delivered.deliveries[0].status = TeamDeliveryStatus::Delivered;
    delivered.deliveries[0].attempt = 1;
    delivered.deliveries[0].provider_receipt_id = Some("kimi-session:turn-2".into());
    store
        .append_team_message(&delivered)
        .expect("persist provider receipt");
    store
        .append_team_message_checked(&handoff)
        .expect("handoff is valid after provider receipt");

    std::fs::remove_dir_all(root).expect("remove temp store");
}
