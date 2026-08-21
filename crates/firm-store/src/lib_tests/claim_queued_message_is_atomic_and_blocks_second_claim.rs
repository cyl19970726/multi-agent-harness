use super::*;

#[test]
fn claim_queued_message_is_atomic_and_blocks_second_claim() {
    let root = std::env::temp_dir().join(format!(
        "firm-store-claim-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_millis()
    ));
    let store = HarnessStore::new(&root);
    store
        .append_message(&test_message("message-1", "agent-1"))
        .expect("append message 1");
    store
        .append_message(&test_message("message-2", "agent-1"))
        .expect("append message 2");

    let claim = store
        .claim_queued_message_delivery("agent-1", "message-1", test_delivery("delivery-1"))
        .expect("claim message");
    assert!(matches!(claim, MessageDeliveryClaimResult::Claimed(_)));

    let latest_message = store
        .messages()
        .expect("messages")
        .into_iter()
        .rev()
        .find(|message| message.id == "message-1")
        .expect("latest message");
    assert_eq!(
        latest_message.delivery_status,
        RegistryDeliveryStatus::Acknowledged
    );
    assert_eq!(
        latest_message
            .delivery
            .and_then(|delivery| delivery.delivery_id),
        Some("delivery-1".into())
    );

    let second_claim = store
        .claim_queued_message_delivery("agent-1", "message-2", test_delivery("delivery-2"))
        .expect("second claim");
    assert_eq!(
        second_claim,
        MessageDeliveryClaimResult::BlockedByDelivery("delivery-1".into())
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
