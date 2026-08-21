use super::*;

/// Durability: a claim writes and fsyncs the Acknowledged message row with
/// its Running delivery attempt, and a *separate* store handle opened
/// against the same root (no shared in-memory state, mirroring a process
/// restart after a crash) reads them back. This guards the double-delivery
/// regression: if the Acknowledged row were lost, latest-wins would revert
/// the message to Queued and it would be claimable again.
#[test]
fn claim_appends_survive_reopen() {
    let root = std::env::temp_dir().join(format!(
        "firm-store-durability-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_millis()
    ));
    let store = HarnessStore::new(&root);
    store
        .append_message(&test_message("message-d", "agent-d"))
        .expect("append message");

    let claim = store
        .claim_queued_message_delivery("agent-d", "message-d", test_delivery("delivery-d"))
        .expect("claim message");
    assert!(matches!(claim, MessageDeliveryClaimResult::Claimed(_)));

    // Reopen with a fresh handle: only on-disk (fsynced) state is visible.
    let reopened = HarnessStore::new(&root);

    let message = reopened
        .messages()
        .expect("read messages")
        .into_iter()
        .rev()
        .find(|message| message.id == "message-d")
        .expect("acknowledged message row survives reopen");
    assert_eq!(
        message.delivery_status,
        RegistryDeliveryStatus::Acknowledged,
        "acknowledged status must survive a restart so the message is not re-delivered"
    );

    let delivery = message.delivery.expect("delivery attempt survives reopen");
    assert_eq!(delivery.delivery_id.as_deref(), Some("delivery-d"));
    assert_eq!(
        delivery.execution_status,
        Some(ProviderExecutionStatus::Running)
    );

    // The reopened store must refuse to re-claim: because both the
    // Acknowledged message row and its Running delivery attempt survived
    // the fsync, the re-claim is rejected (the active attempt for this
    // agent blocks delivery; were the row lost it would return Claimed and
    // double-deliver). Either rejection variant proves no double-delivery.
    let reclaim = reopened
        .claim_queued_message_delivery("agent-d", "message-d", test_delivery("delivery-d2"))
        .expect("reclaim attempt");
    assert!(
        !matches!(reclaim, MessageDeliveryClaimResult::Claimed(_)),
        "fsynced claim state must prevent a second delivery, got {reclaim:?}"
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
