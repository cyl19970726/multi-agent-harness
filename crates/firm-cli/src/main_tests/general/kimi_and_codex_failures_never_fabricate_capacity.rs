use super::*;

    #[test]
    fn kimi_and_codex_failures_never_fabricate_capacity() {
        // Kimi ACP surfaces a 403 as free-form JSON-RPC error text with no
        // status field, and Codex app-server errors arrive as adapter strings.
        // Neither carries structured metadata, so neither may gate a start.
        let actions = vec![
            provider_error_action(
                "member-run-1",
                "unix-ms:1000",
                "provider turn failed: kimi acp session/prompt rejected: {\"code\":-32603,\
                 \"message\":\"403 quota exceeded\"}",
                None,
            ),
            provider_error_action(
                "member-run-1",
                "unix-ms:1100",
                "provider turn failed: codex app-server turn/start failed: 429 too many requests",
                None,
            ),
        ];

        assert!(
            capacity_from_provider_error_actions(
                &actions,
                "member-run-1",
                "kimi",
                "kimi_acp",
                1_500,
                1_000,
            )
            .is_none(),
            "an execution mode with no structured terminal metadata stays unknown"
        );
    }

