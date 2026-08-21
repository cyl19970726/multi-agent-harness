use super::*;

    #[test]
    fn stale_failed_delivery_attempt_marks_message_failed_and_clears_member() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("stale-failed")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("agent-1");
        member.status = ProviderLaunchStatus::Stale;
        member.current_task_id = Some("task-1".into());
        store.append_member(&member).expect("append member");
        store
            .append_message(&RegistryMessage {
                id: "message-1".into(),
                task_id: Some("task-1".into()),
                from_agent_id: "lead-1".into(),
                to_agent_id: Some("agent-1".into()),
                channel: Some("assignment".into()),
                kind: RegistryMessageIntent::Message,
                delivery_status: RegistryDeliveryStatus::Acknowledged,
                content: "Do the task".into(),
                evidence_ids: Vec::new(),
                created_at: "unix-ms:1".into(),
                delivery: Some(RegistryDeliveryAttempt {
                    delivery_id: Some("delivery-1".into()),
                    execution_status: Some(ProviderExecutionStatus::Stale),
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
        mark_running_delivery_attempts_terminal(
            &store,
            "agent-1",
            ProviderExecutionStatus::Stale,
            Some(MessageTerminalSource::Failed),
        )
        .expect("mark stale failed");

        assert!(!has_unresolved_delivery_attempt(&store, "agent-1").expect("running check"));
        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            RegistryDeliveryStatus::Failed
        );
        let latest_member = latest_member(&store, "agent-1").expect("latest member");
        assert_eq!(latest_member.status, ProviderLaunchStatus::Idle);
        assert_eq!(latest_member.current_task_id, None);

        let _ = std::fs::remove_dir_all(root);
    }

