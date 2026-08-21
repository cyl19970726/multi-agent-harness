use super::*;

#[test]
#[cfg(any())]
fn successor_supervisor_fences_stale_claim_before_any_canonical_side_effect() {
    let harness = TestStore::new("delivery-supervisor-successor");
    let host = human("host");
    let team_run = seed_team(
        &harness.store,
        "delivery-supervisor-successor",
        &["member-a"],
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "run-a",
        true,
    );
    harness
        .store
        .create_trust_team_message_with_deliveries(
            &context(host, "message.create", "message-successor", 0),
            message(
                "message-successor",
                &team_run.id,
                &human("host"),
                &["member-a"],
            ),
            "t2",
        )
        .expect("queue delivery");
    let first = acquire_supervisor(&harness.store, &team_run, "supervisor-old");
    harness
        .store
        .release_team_supervisor_lease(
            &team_run.id,
            &first.supervisor_id,
            first.generation,
            unix_ms(),
        )
        .expect("release old supervisor");
    let successor = acquire_supervisor(&harness.store, &team_run, "supervisor-successor");
    assert!(successor.generation > first.generation);

    let before = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_message_delivery(
                    &context(
                        service(&first.supervisor_id),
                        "delivery.claim",
                        "stale-supervisor-claim",
                        0,
                    ),
                    "message-successor:run-a",
                    delivery_claim("claim-stale-supervisor", first.generation, 1),
                    "t3",
                )
                .expect_err("successor acquisition must fence old supervisor")
        ),
        TrustErrorCode::SupervisorGenerationFenced
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before,
        "stale Supervisor loses at the same Store lock before provider-visible state"
    );
    harness
        .store
        .claim_trust_message_delivery(
            &context(
                service(&successor.supervisor_id),
                "delivery.claim",
                "successor-claim",
                0,
            ),
            "message-successor:run-a",
            delivery_claim("claim-successor", successor.generation, 1),
            "t4",
        )
        .expect("current successor can claim");
}
