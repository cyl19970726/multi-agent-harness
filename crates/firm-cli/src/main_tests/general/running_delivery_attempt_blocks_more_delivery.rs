use super::*;

    #[test]
    fn running_delivery_attempt_blocks_more_delivery() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("block")));
        let store = HarnessStore::new(&root);
        append_test_delivery_attempt(
            &store,
            "agent-1",
            Some("task-1"),
            ProviderExecutionStatus::Running,
            Some("thread-1"),
            Some("turn-1"),
        );

        assert!(has_unresolved_delivery_attempt(&store, "agent-1").expect("running check"));

        mark_running_delivery_attempts_terminal(
            &store,
            "agent-1",
            ProviderExecutionStatus::Stale,
            Some(MessageTerminalSource::Failed),
        )
        .expect("mark stale");
        assert!(!has_unresolved_delivery_attempt(&store, "agent-1").expect("running check"));
        let latest = latest_messages_in_append_order(&store)
            .expect("messages")
            .into_iter()
            .find(|message| message.to_agent_id.as_deref() == Some("agent-1"))
            .expect("latest delivery message");
        assert_eq!(latest.delivery_status, RegistryDeliveryStatus::Failed);

        let _ = std::fs::remove_dir_all(root);
    }

