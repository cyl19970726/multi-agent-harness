use super::*;

#[test]
fn retry_delivery_requeues_safe_claim_without_provider_request() {
    let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("retry")));
    let store = HarnessStore::new(&root);
    let member = make_member("agent-1");
    store.append_member(&member).expect("append member");
    let message = RegistryMessage {
        id: "message-1".into(),
        task_id: Some("task-1".into()),
        from_agent_id: "leader".into(),
        to_agent_id: Some("agent-1".into()),
        channel: Some("assignment".into()),
        kind: RegistryMessageIntent::Message,
        delivery_status: RegistryDeliveryStatus::Queued,
        content: "Assign task".into(),
        evidence_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        delivery: None,
        sender_kind: SenderKind::Agent,
    };
    store.append_message(&message).expect("append queued");
    claim_message_for_delivery(&store, &member, None, &message, "delivery-1")
        .expect("claim")
        .expect("claimed message");

    retry_delivery_value(
        &store,
        "agent-1",
        "message-1",
        Some("delivery-1"),
        "safe retry test",
        false,
    )
    .expect("retry delivery");

    let latest_message = latest_message(&store, "message-1").expect("latest message");
    assert_eq!(
        latest_message.delivery_status,
        RegistryDeliveryStatus::Queued
    );
    assert!(latest_message.delivery.is_none());
    assert!(
        !store.root().join("provider_sessions.jsonl").exists(),
        "retrying a delivery attempt must not create a provider-session mirror"
    );

    let _ = std::fs::remove_dir_all(root);
}
