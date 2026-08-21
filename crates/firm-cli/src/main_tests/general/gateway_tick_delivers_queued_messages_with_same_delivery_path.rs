use super::*;

    #[test]
    fn gateway_tick_delivers_queued_messages_with_same_delivery_path() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("gateway")));
        let store = HarnessStore::new(&root);
        store
            .append_member(&make_member("agent-1"))
            .expect("append member 1");
        store
            .append_member(&make_member("agent-2"))
            .expect("append member 2");
        for agent_id in ["agent-1", "agent-2"] {
            store
                .append_message(&RegistryMessage {
                    id: format!("message-{agent_id}"),
                    task_id: Some(format!("task-{agent_id}")),
                    from_agent_id: "leader".into(),
                    to_agent_id: Some(agent_id.into()),
                    channel: Some("assignment".into()),
                    kind: RegistryMessageIntent::Message,
                    delivery_status: RegistryDeliveryStatus::Queued,
                    content: "Assign task".into(),
                    evidence_ids: Vec::new(),
                    created_at: "unix-ms:1".into(),
                    delivery: None,
                    sender_kind: SenderKind::Agent,
                })
                .expect("append queued");
        }

        let result = provider_gateway_tick_value(
            &store,
            None,
            GatewayOptions {
                dry_run: true,
                start_runtime: false,
                timeout_ms: 100,
                claim_ttl_ms: 300_000,
            },
        )
        .expect("gateway tick");

        assert_eq!(result["agent_count"].as_u64(), Some(2));
        for agent_id in ["agent-1", "agent-2"] {
            let latest =
                latest_message(&store, &format!("message-{agent_id}")).expect("latest message");
            assert_eq!(latest.delivery_status, RegistryDeliveryStatus::Delivered);
            assert!(latest
                .delivery
                .and_then(|delivery| delivery.delivery_id)
                .is_some());
        }

        let _ = std::fs::remove_dir_all(root);
    }

