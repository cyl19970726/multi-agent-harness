use super::*;

    #[test]
    fn codex_multi_question_request_fails_closed_without_durable_side_effects() {
        let (store, _root) = temp_store("codex-multi-question-fail-closed");
        let (ledger, member) = persisted_native_test_member(
            &store,
            "codex",
            "codex_app_server",
            "thread-multi-question",
        );
        let messages_before = store
            .legacy_team_messages()
            .expect("Legacy messages before")
            .len();
        let actions_before = store.member_actions().expect("member actions before").len();
        let frame = serde_json::json!({
            "id": 700,
            "method": "item/tool/requestUserInput",
            "params": {
                "threadId": "thread-multi-question",
                "questions": [
                    {"id": "first", "header": "First", "question": "First question?", "options": []},
                    {"id": "second", "header": "Second", "question": "Second question?", "options": []}
                ]
            }
        });

        let error = handle_codex_provider_request(&ledger, &member, &frame)
            .expect_err("multi-question request must fail closed");
        assert!(
            error
                .to_string()
                .contains("supports exactly one question; received 2"),
            "unexpected error: {error}"
        );
        assert_eq!(
            store
                .legacy_team_messages()
                .expect("Legacy messages after")
                .len(),
            messages_before,
            "unsupported request must not create a provider request message"
        );
        assert_eq!(
            store.member_actions().expect("member actions after").len(),
            actions_before,
            "unsupported request must not create a provider-control receipt"
        );
    }

