use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn provider_interaction_response_rejects_unknown_choice_and_predelivery() {
    let root = team_test_root("provider-interaction-invalid-response");
    let store = HarnessStore::new(&root);
    let (request_body, request) =
        seed_provider_interaction_bridge(&store, "run-interaction-invalid");
    let mut unstable_id = provider_interaction_response(&request_body, &request, "continue");
    unstable_id.id = "caller-selected-response-id".into();
    assert!(store
        .record_provider_interaction_response(&unstable_id, "unix-ms:4")
        .expect_err("response id is request-derived")
        .to_string()
        .contains("must be stable"));
    let unknown = provider_interaction_response(&request_body, &request, "invented");
    assert!(store
        .record_provider_interaction_response(&unknown, "unix-ms:4")
        .expect_err("unknown choice")
        .to_string()
        .contains("not a request option"));

    let mut predelivered = provider_interaction_response(&request_body, &request, "continue");
    predelivered.deliveries[0].status = TeamDeliveryStatus::Delivered;
    predelivered.deliveries[0].provider_receipt_id = Some("forged".into());
    assert!(store
        .record_provider_interaction_response(&predelivered, "unix-ms:4")
        .expect_err("cannot claim provider receipt early")
        .to_string()
        .contains("Inject+Queued"));
    let mut extra_route = provider_interaction_response(&request_body, &request, "continue");
    extra_route
        .recipient_runtime_ids
        .push("other-member".into());
    extra_route.deliveries.push(ProviderDispatchAttempt {
        member_id: "other-member".into(),
        policy: TeamDeliveryPolicy::Inject,
        status: TeamDeliveryStatus::Queued,
        attempt: 0,
        claim_id: None,
        claimed_by_supervisor_id: None,
        claimed_generation: None,
        claimed_unix_ms: None,
        claim_expires_unix_ms: None,
        provider_receipt_id: None,
        failure_reason: None,
        updated_at: "unix-ms:4".into(),
    });
    assert!(store
        .record_provider_interaction_response(&extra_route, "unix-ms:4")
        .expect_err("response cannot fan out")
        .to_string()
        .contains("route only"));

    let mut unacknowledgeable = request.clone();
    unacknowledgeable.deliveries[0].status = TeamDeliveryStatus::Failed;
    {
        let _lock = store.acquire_write_lock().expect("fault injection lock");
        store
            .append_jsonl_unlocked("team_messages.jsonl", &unacknowledgeable)
            .expect("simulate failed Host delivery through private ledger primitive");
    }
    let valid = provider_interaction_response(&request_body, &request, "continue");
    assert!(store
        .record_provider_interaction_response(&valid, "unix-ms:4")
        .expect_err("ACK failure must preflight before response append")
        .to_string()
        .contains("cannot be acknowledged"));
    assert_eq!(
        store
            .legacy_team_messages()
            .expect("messages")
            .iter()
            .filter(|message| message.kind == ProviderDispatchIntent::ProviderInteractionResponse)
            .count(),
        0,
        "failed ACK must not leave a partial response row"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
