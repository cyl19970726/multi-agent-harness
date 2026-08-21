use super::*;

    #[test]
    fn turn_input_uses_stable_harness_envelope() {
        let message = RegistryMessage {
            id: "message-1".into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: RegistryMessageIntent::Message,
            delivery_status: RegistryDeliveryStatus::Acknowledged,
            content: "Do the task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
            sender_kind: SenderKind::Agent,
        };

        let input = build_turn_input(&message, "delivery-1");
        let text = input[0]["text"].as_str().expect("turn text");

        assert!(text.contains("message_id: message-1"));
        assert!(text.contains("kind: message"));
        assert!(text.contains("task_id: task-1"));
        assert!(text.contains("from_agent_id: leader"));
        assert!(text.contains("to_agent_id: agent-1"));
        assert!(text.contains("channel: assignment"));
        assert!(text.contains("delivery_attempt: delivery-1"));
        assert!(text.contains("content:\nDo the task"));
        assert!(!text.contains("kind: Assignment"));
    }

