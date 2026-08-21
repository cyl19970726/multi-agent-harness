use super::*;

#[test]
#[cfg(any())]
fn fanout_is_atomic_and_creates_exactly_one_delivery_per_recipient() {
    let harness = TestStore::new("fanout");
    let host = human("host");
    let team_run = seed_team(&harness.store, "fanout", &["member-a", "member-b"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        false,
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-b",
        "run-b",
        false,
    );

    let before = harness.store.canonical_operations().unwrap().len();
    let invalid = message(
        "message-invalid",
        &team_run.id,
        &host,
        &["member-a", "missing-member"],
    );
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_team_message_with_deliveries(
                    &context(host.clone(), "message.create", "invalid-fanout", 0),
                    invalid,
                    "t2",
                )
                .expect_err("unresolvable fanout must fail atomically")
        ),
        TrustErrorCode::InvalidStateTransition
    );
    assert_eq!(harness.store.canonical_operations().unwrap().len(), before);
    assert!(harness
        .store
        .trust_message_deliveries(SPACE)
        .unwrap()
        .is_empty());

    let valid = message(
        "message-valid",
        &team_run.id,
        &host,
        &["member-a", "member-b"],
    );
    let result = harness
        .store
        .create_trust_team_message_with_deliveries(
            &context(host, "message.create", "valid-fanout", 0),
            valid,
            "t3",
        )
        .expect("valid fanout");
    assert_eq!(result.event.aggregate_kind, "team_message");
    let deliveries = harness.store.trust_message_deliveries(SPACE).unwrap();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(
        deliveries
            .iter()
            .map(|delivery| delivery.recipient_member_run_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        ["run-a", "run-b"].into_iter().collect()
    );
    assert_eq!(
        harness
            .store
            .canonical_operations()
            .unwrap()
            .last()
            .unwrap()
            .initial_outbox_records
            .len(),
        2
    );
}
