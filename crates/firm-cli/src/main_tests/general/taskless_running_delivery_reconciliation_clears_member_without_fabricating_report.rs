use super::*;

    #[test]
    fn taskless_running_delivery_reconciliation_clears_member_without_fabricating_report() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("direct")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("agent-1");
        member.status = ProviderLaunchStatus::Running;
        member.current_task_id = None;
        store.append_member(&member).expect("append member");
        store
            .append_message(&RegistryMessage {
                id: "message-1".into(),
                task_id: None,
                from_agent_id: "lead-1".into(),
                to_agent_id: Some("agent-1".into()),
                channel: Some("direct".into()),
                kind: RegistryMessageIntent::Message,
                delivery_status: RegistryDeliveryStatus::Acknowledged,
                content: "Direct message".into(),
                evidence_ids: Vec::new(),
                created_at: "unix-ms:1".into(),
                delivery: Some(RegistryDeliveryAttempt {
                    delivery_id: Some("delivery-1".into()),
                    execution_status: Some(ProviderExecutionStatus::Running),
                    native_session: None,
                    started_at: Some("unix-ms:1".into()),
                    provider_request_id: None,
                    provider_thread_id: Some("thread-1".into()),
                    provider_turn_id: Some("turn-1".into()),
                    terminal_source: Some(MessageTerminalSource::Unknown),
                    delivered_at: Some("unix-ms:1".into()),
                    last_error: None,
                }),
                sender_kind: SenderKind::Agent,
            })
            .expect("append acknowledged message");
        reconcile_running_delivery_attempts(
            &store,
            "agent-1",
            None,
            Some("thread-1"),
            Some("turn-1"),
            MessageTerminalSource::TurnCompleted,
        )
        .expect("reconcile taskless delivery");

        let latest_member = latest_member(&store, "agent-1").expect("latest member");
        assert_eq!(latest_member.status, ProviderLaunchStatus::Idle);
        assert_eq!(latest_member.current_task_id, None);
        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            RegistryDeliveryStatus::Delivered
        );
        assert!(
            store
                .messages()
                .expect("messages")
                .into_iter()
                .all(|message| message.kind != RegistryMessageIntent::Report),
            "provider terminal activity must never fabricate an authored report Message"
        );

        let _ = std::fs::remove_dir_all(root);
    }

