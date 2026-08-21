use super::*;

#[test]
#[ignore = "retired projection-message Handoff contract; canonical response intent is covered by trust-kernel delivery tests"]
fn informational_mail_neither_fences_handoff_nor_requires_response() {
    let root = team_test_root("handoff-informational-fence");
    let store = HarnessStore::new(&root);
    // Acknowledgement-only peer mail: kind `message` with no explicit
    // intent is informational by default (ADR 0046 §4).
    let ack_only = TeamMessageProjection {
        id: "tm-ack".into(),
        team_run_id: "tr-info".into(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "mr-peer".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["mr-kimi".into()],
        kind: ProviderDispatchIntent::Message,
        body: "ACK: noted, no reply needed".into(),
        correlation_id: "corr-info".into(),
        causation_id: Some("tm-assignment".into()),
        response_intent: None,
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
    assert!(!ack_only.requires_response());
    store
        .append_team_message_checked(&ack_only)
        .expect("append informational mail");
    let handoff = TeamMessageProjection {
        id: "tm-handoff".into(),
        team_run_id: "tr-info".into(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "mr-kimi".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["host".into()],
        kind: ProviderDispatchIntent::Message,
        body: "done".into(),
        correlation_id: "corr-info".into(),
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
    // Informational mail never starts a provider round on its own, so it
    // must not fence a Handoff either — otherwise the Handoff would
    // deadlock behind mail that is intentionally never driven.
    store
        .append_team_message_checked(&handoff)
        .expect("informational mail must not fence handoff");

    // The same pending delivery with explicit response intent fences.
    let question = TeamMessageProjection {
        id: "tm-question".into(),
        correlation_id: "corr-info-q".into(),
        causation_id: None,
        response_intent: Some(ProviderResponseIntent::ResponseRequired),
        created_at: "unix-ms:3".into(),
        ..ack_only.clone()
    };
    assert!(question.requires_response());
    store
        .append_team_message_checked(&question)
        .expect("append response-required question");
    let fenced = TeamMessageProjection {
        id: "tm-handoff-q".into(),
        correlation_id: "corr-info-q".into(),
        causation_id: Some("tm-assignment-q".into()),
        created_at: "unix-ms:4".into(),
        ..handoff.clone()
    };
    let error = store
        .append_team_message_checked(&fenced)
        .expect_err("response-required question must fence stale handoff");
    assert!(error.to_string().contains("queued or claimed"));

    // Safety regression guard: a Host mid-round correction is ordinary
    // `message` mail with no explicit intent, but it is sender-aware
    // response-required, so it MUST still fence a same-correlation Handoff
    // — otherwise a member could hand off work that never absorbed the
    // correction.
    let host_correction = TeamMessageProjection {
        id: "tm-host-correction".into(),
        sender_runtime_id: "host".into(),
        correlation_id: "corr-info-host".into(),
        causation_id: None,
        response_intent: None,
        body: "Revise: drop the extra scope before handing off".into(),
        created_at: "unix-ms:5".into(),
        ..ack_only.clone()
    };
    assert!(
        host_correction.requires_response(),
        "Host ordinary mail defaults to response_required"
    );
    store
        .append_team_message_checked(&host_correction)
        .expect("append host correction");
    let stale = TeamMessageProjection {
        id: "tm-handoff-host".into(),
        correlation_id: "corr-info-host".into(),
        causation_id: Some("tm-assignment-host".into()),
        created_at: "unix-ms:6".into(),
        ..handoff.clone()
    };
    let error = store
        .append_team_message_checked(&stale)
        .expect_err("pending Host correction must fence stale handoff");
    assert!(error.to_string().contains("queued or claimed"));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
