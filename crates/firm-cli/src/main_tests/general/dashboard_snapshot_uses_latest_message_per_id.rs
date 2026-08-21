use super::*;

#[test]
fn dashboard_snapshot_uses_latest_message_per_id() {
    let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("messages")));
    let store = HarnessStore::new(&root);
    let mut message = RegistryMessage {
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
    store
        .append_message(&message)
        .expect("append queued message");
    message.delivery_status = RegistryDeliveryStatus::Acknowledged;
    store
        .append_message(&message)
        .expect("append acknowledged message");

    let snapshot = dashboard_snapshot(&store).expect("dashboard snapshot");
    let messages = snapshot
        .get("messages")
        .and_then(|value| value.as_array())
        .expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]
            .get("delivery_status")
            .and_then(|value| value.as_str()),
        Some("acknowledged")
    );

    let _ = std::fs::remove_dir_all(root);
}
