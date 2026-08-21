use super::*;

#[test]
#[cfg(any())]
fn delivery_claim_and_receipt_are_generation_fenced_and_reconcile_is_explicit() {
    let harness = TestStore::new("delivery-generation");
    let host = human("host");
    let team_run = seed_team(&harness.store, "delivery-generation", &["member-a"]);
    let supervisor = acquire_supervisor(&harness.store, &team_run, "supervisor-a");
    let supervisor_actor = service(&supervisor.supervisor_id);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "run-a",
        true,
    );
    for id in ["old", "uncertain", "queued"] {
        harness
            .store
            .create_trust_team_message_with_deliveries(
                &context(host.clone(), "message.create", &format!("message-{id}"), 0),
                message(&format!("message-{id}"), &team_run.id, &host, &["member-a"]),
                "t2",
            )
            .expect("queue delivery");
    }

    let stale_before = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_message_delivery(
                    &context(supervisor_actor.clone(), "delivery.claim", "stale-claim", 0),
                    "message-old:run-a",
                    delivery_claim("claim-stale", supervisor.generation, 0),
                    "t3",
                )
                .expect_err("stale generation cannot claim")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        stale_before
    );

    for (delivery, claim_id) in [
        ("message-old:run-a", "claim-old"),
        ("message-uncertain:run-a", "claim-uncertain"),
    ] {
        harness
            .store
            .claim_trust_message_delivery(
                &context(
                    supervisor_actor.clone(),
                    "delivery.claim",
                    &format!("key-{claim_id}"),
                    0,
                ),
                delivery,
                delivery_claim(claim_id, supervisor.generation, 1),
                "t3",
            )
            .expect("claim at generation one");
    }
    harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.close", "delivery-close", 1),
            "run-a",
            MemberCoordinationStatus::Closed,
            "t4",
        )
        .expect("close run");
    harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.reopen", "delivery-reopen", 2),
            "run-a",
            MemberCoordinationStatus::Active,
            "t5",
        )
        .expect("reopen at generation two");

    let receipt = ProviderReceipt {
        claim_id: "claim-old".into(),
        supervisor_generation: supervisor.generation,
        member_generation: 1,
        provider_receipt_id: "provider-old".into(),
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .receive_trust_message_delivery(
                    &context(
                        supervisor_actor.clone(),
                        "delivery.receive",
                        "stale-receipt",
                        1
                    ),
                    "message-old:run-a",
                    receipt,
                    "t6",
                )
                .expect_err("old-generation receipt must be fenced")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );

    let reconciled = harness
        .store
        .reconcile_trust_message_delivery(
            &context(host.clone(), "delivery.reconcile", "explicit-reconcile", 1),
            "message-uncertain:run-a",
            DeliveryReconcileOutcome::RetrySafeFailure,
            "evidence://provider-query",
            "t7",
        )
        .expect("uncertain old claim requires explicit evidence-backed reconciliation")
        .projection;
    assert_eq!(reconciled.status, MessageDeliveryStatus::Failed);
    assert_eq!(
        reconciled.failure_detail.as_deref(),
        Some("evidence://provider-query")
    );

    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_message_delivery(
                    &context(
                        supervisor_actor.clone(),
                        "delivery.claim",
                        "queued-old-generation",
                        0
                    ),
                    "message-queued:run-a",
                    delivery_claim("claim-queued-stale", supervisor.generation, 1),
                    "t8",
                )
                .expect_err("frozen queued delivery cannot use old generation")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    harness
        .store
        .claim_trust_message_delivery(
            &context(
                supervisor_actor.clone(),
                "delivery.claim",
                "queued-new-generation",
                0,
            ),
            "message-queued:run-a",
            delivery_claim("claim-queued", supervisor.generation, 2),
            "t8",
        )
        .expect("new generation may claim frozen delivery");
    harness
        .store
        .receive_trust_message_delivery(
            &context(
                supervisor_actor.clone(),
                "delivery.receive",
                "fresh-receipt",
                1,
            ),
            "message-queued:run-a",
            ProviderReceipt {
                claim_id: "claim-queued".into(),
                supervisor_generation: supervisor.generation,
                member_generation: 2,
                provider_receipt_id: "provider-fresh".into(),
            },
            "t9",
        )
        .expect("matching receipt");
    let acknowledged = harness
        .store
        .acknowledge_trust_message_delivery(
            &context(supervisor_actor, "delivery.ack", "fresh-ack", 2),
            "message-queued:run-a",
            "claim-queued",
            2,
            "t10",
        )
        .expect("matching acknowledgement")
        .projection;
    assert_eq!(acknowledged.status, MessageDeliveryStatus::Acknowledged);
}
