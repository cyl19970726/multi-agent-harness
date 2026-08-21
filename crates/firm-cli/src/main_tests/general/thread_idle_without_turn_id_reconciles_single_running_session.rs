use super::*;

    #[test]
    fn thread_idle_without_turn_id_reconciles_single_running_session() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("idle")));
        let store = HarnessStore::new(&root);
        append_test_delivery_attempt(
            &store,
            "agent-1",
            Some("task-1"),
            ProviderExecutionStatus::Running,
            Some("thread-1"),
            Some("turn-1"),
        );

        reconcile_running_delivery_attempts(
            &store,
            "agent-1",
            Some("task-1"),
            Some("thread-1"),
            None,
            MessageTerminalSource::ThreadIdle,
        )
        .expect("thread idle should reconcile the active session");

        let latest = latest_messages_in_append_order(&store)
            .expect("messages")
            .into_iter()
            .find(|message| message.to_agent_id.as_deref() == Some("agent-1"))
            .expect("latest delivery message");
        assert_eq!(latest.delivery_status, RegistryDeliveryStatus::Delivered);
        assert_eq!(
            latest
                .delivery
                .as_ref()
                .and_then(|delivery| delivery.provider_turn_id.as_deref()),
            Some("turn-1")
        );

        let _ = std::fs::remove_dir_all(root);
    }

