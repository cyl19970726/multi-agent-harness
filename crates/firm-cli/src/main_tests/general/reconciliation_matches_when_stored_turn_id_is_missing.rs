use super::*;

    #[test]
    fn reconciliation_matches_when_stored_turn_id_is_missing() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("turnless")));
        let store = HarnessStore::new(&root);
        append_test_delivery_attempt(
            &store,
            "agent-1",
            Some("task-1"),
            ProviderExecutionStatus::Running,
            Some("thread-1"),
            None,
        );

        reconcile_running_delivery_attempts(
            &store,
            "agent-1",
            Some("task-1"),
            Some("thread-1"),
            Some("turn-1"),
            MessageTerminalSource::TurnCompleted,
        )
        .expect("reconcile session with missing stored turn id");

        let latest = latest_messages_in_append_order(&store)
            .expect("messages")
            .into_iter()
            .find(|message| message.to_agent_id.as_deref() == Some("agent-1"))
            .expect("latest delivery message");
        assert_eq!(latest.delivery_status, RegistryDeliveryStatus::Delivered);

        let _ = std::fs::remove_dir_all(root);
    }

