use super::*;

#[test]
#[cfg(any())]
fn close_reopen_and_retire_fence_queued_delivery_by_generation() {
    let harness = TestStore::new("run-lifecycle");
    let host = human("host");
    let team_run = seed_team(&harness.store, "lifecycle", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        true,
    );
    harness
        .store
        .create_trust_team_message_with_deliveries(
            &context(host.clone(), "message.create", "queue-message", 0),
            message("message-a", &team_run.id, &host, &["member-a"]),
            "t2",
        )
        .expect("queue message");

    let closed = harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.close", "close", 1),
            "run-a",
            MemberCoordinationStatus::Closed,
            "t3",
        )
        .expect("close run")
        .projection;
    assert_eq!(closed.runtime_generation, 1);
    assert_eq!(closed.runtime_status, MemberRuntimeStatus::Stopped);
    assert_eq!(
        harness.store.trust_message_deliveries(SPACE).unwrap()[0].freeze_generation,
        Some(1)
    );

    let reopened = harness
        .store
        .transition_trust_member_run(
            &context(host.clone(), "member_run.reopen", "reopen", 2),
            "run-a",
            MemberCoordinationStatus::Active,
            "t4",
        )
        .expect("reopen resumable run")
        .projection;
    assert_eq!(reopened.runtime_generation, 2);
    assert_eq!(reopened.runtime_status, MemberRuntimeStatus::Idle);

    let retired = harness
        .store
        .transition_trust_member_run(
            &context(host, "member_run.retire", "retire-run", 3),
            "run-a",
            MemberCoordinationStatus::Retired,
            "t5",
        )
        .expect("retire run")
        .projection;
    assert_eq!(retired.runtime_generation, 2);
    assert_eq!(retired.runtime_status, MemberRuntimeStatus::Stopped);
    assert_eq!(retired.finished_at.as_deref(), Some("t5"));
    let delivery = &harness.store.trust_message_deliveries(SPACE).unwrap()[0];
    assert_eq!(
        delivery.status,
        firm_core::agentfirm_api::MessageDeliveryStatus::Invalidated
    );
    assert_eq!(delivery.version, 3);
}
