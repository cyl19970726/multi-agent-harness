use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn provider_interaction_response_atomically_acks_and_is_strictly_idempotent() {
    let root = team_test_root("provider-interaction-idempotent");
    let store = HarnessStore::new(&root);
    let (request_body, request) =
        seed_provider_interaction_bridge(&store, "run-interaction-idempotent");
    let response = provider_interaction_response(&request_body, &request, "continue");
    let first = store
        .record_provider_interaction_response(&response, "unix-ms:4")
        .expect("record response");
    assert_eq!(first, response);

    let exact_retry = response.clone();
    let retried = store
        .record_provider_interaction_response(&exact_retry, "unix-ms:9")
        .expect("same stable id and semantic reply returns existing");
    assert_eq!(retried.id, response.id);

    let messages = latest_by_id(
        store.legacy_team_messages().expect("legacy messages"),
        |message| message.id.clone(),
    );
    let request_after = messages.get(&request.id).expect("request remains");
    assert_eq!(
        request_after.deliveries[0].status,
        TeamDeliveryStatus::Acknowledged
    );
    let response_after = messages.get(&response.id).expect("response remains");
    assert_eq!(
        response_after.deliveries[0].status,
        TeamDeliveryStatus::Queued,
        "Host answer is not provider delivery truth"
    );
    assert_eq!(
        messages
            .values()
            .filter(|message| message.kind == ProviderDispatchIntent::ProviderInteractionResponse)
            .count(),
        1
    );

    store
        .acquire_test_supervisor_lease(
            &request.team_run_id,
            "supervisor-interaction",
            42,
            "test",
            100,
            1_000,
        )
        .expect("lease response delivery");
    let claimed = store
        .claim_team_message_delivery(
            &request.team_run_id,
            &response.id,
            &request_body.member,
            "supervisor-interaction",
            1,
            "claim-interaction-response",
            101,
            1_000,
            "unix-ms:5",
        )
        .expect("claim response");
    assert!(matches!(
        claimed,
        TeamMessageDeliveryClaimResult::Claimed(_)
    ));
    store
        .complete_team_message_delivery_claim(
            &request.team_run_id,
            &response.id,
            &request_body.member,
            "supervisor-interaction",
            1,
            "claim-interaction-response",
            "native-response-receipt",
            102,
            "unix-ms:6",
        )
        .expect("provider accepted response");
    let retry_after_delivery = store
        .record_provider_interaction_response(&response, "unix-ms:7")
        .expect("semantic retry survives mutable delivery projection");
    assert_eq!(
        retry_after_delivery.deliveries[0].status,
        TeamDeliveryStatus::Delivered
    );
    assert_eq!(
        retry_after_delivery.deliveries[0]
            .provider_receipt_id
            .as_deref(),
        Some("native-response-receipt")
    );
    let current_member = latest_by_id(store.member_runs().expect("members"), |member| {
        member.id.clone()
    })
    .remove(&request_body.member)
    .expect("current member");
    let mut closed_member = current_member.clone();
    closed_member.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    closed_member.status = MemberRunStatus::Stopped;
    closed_member.finished_at = Some("unix-ms:8".into());
    store
        .compare_and_append_member_run(&current_member, &closed_member)
        .expect("close member");
    let retry_after_close = store
        .record_provider_interaction_response(&response, "unix-ms:9")
        .expect("exact retry remains valid after member close");
    assert_eq!(retry_after_close.id, response.id);

    let conflict = provider_interaction_response(&request_body, &request, "stop");
    assert!(store
        .record_provider_interaction_response(&conflict, "unix-ms:10")
        .expect_err("different answer conflicts")
        .to_string()
        .contains("RESPONSE_CONFLICT"));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
