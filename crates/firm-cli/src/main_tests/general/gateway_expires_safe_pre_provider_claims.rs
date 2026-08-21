use super::*;

    #[test]
    fn gateway_expires_safe_pre_provider_claims() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("expire")));
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
        let mut old_claim = latest_message(&store, "message-1").expect("claimed message");
        old_claim
            .delivery
            .as_mut()
            .expect("delivery attempt")
            .started_at = Some("unix-ms:1".into());
        store.append_message(&old_claim).expect("append old claim");

        let result = provider_gateway_tick_value(
            &store,
            None,
            GatewayOptions {
                dry_run: false,
                start_runtime: false,
                timeout_ms: 100,
                claim_ttl_ms: 1,
            },
        )
        .expect("gateway tick");

        assert_eq!(result["expired_claims"].as_array().map(Vec::len), Some(1));
        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            RegistryDeliveryStatus::Failed
        );
        assert!(!store.root().join("provider_sessions.jsonl").exists());

        let _ = std::fs::remove_dir_all(root);
    }

