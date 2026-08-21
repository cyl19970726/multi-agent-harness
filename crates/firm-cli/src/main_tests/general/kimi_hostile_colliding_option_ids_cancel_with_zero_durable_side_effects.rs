use super::*;

    #[test]
    fn kimi_hostile_colliding_option_ids_cancel_with_zero_durable_side_effects() {
        let (store, _root) = temp_store("kimi-hostile-option-id-collision");
        let (ledger, member) = persisted_native_test_member(
            &store,
            "kimi",
            "kimi_acp",
            "session-hostile-option-id-collision",
        );
        let messages_before = store
            .legacy_team_messages()
            .expect("Legacy messages before");
        let actions_before = store.member_actions().expect("member actions before");
        let members_before = store.member_runs().expect("member runs before");
        let operations_before = store
            .canonical_operations()
            .expect("canonical operations before");

        for (id, title, option_id, intent) in [
            (811, "AskUserQuestion", "q0_opt_0", "reject_once"),
            (812, "ExitPlanMode", "plan_approve", "reject_always"),
            (813, "AskUserQuestion", "q0_opt_7", "future_allow"),
            (814, "ExitPlanMode", "plan_revise", "future_reject"),
        ] {
            let frame = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/request_permission",
                "params": {
                    "sessionId": "session-hostile-option-id-collision",
                    "options": [{
                        "optionId": option_id,
                        "name": "Prefix collision must not route",
                        "kind": intent
                    }],
                    "toolCall": {
                        "toolCallId": format!("{id}:hostile"),
                        "title": title
                    }
                }
            });
            let outcome = handle_kimi_provider_request(&ledger, &member, &frame)
                .expect("hostile callback must cancel in-process");
            assert_eq!(outcome.result["outcome"]["outcome"], "cancelled");
            assert!(outcome.claimed_response.is_none());
        }

        assert_eq!(
            store.legacy_team_messages().expect("Legacy messages after"),
            messages_before,
            "hostile callbacks must not create Message waits"
        );
        assert_eq!(
            store.member_actions().expect("member actions after"),
            actions_before,
            "hostile callbacks must not write provider-control receipts"
        );
        assert_eq!(
            store.member_runs().expect("member runs after"),
            members_before,
            "hostile callbacks must not move the Member into Waiting"
        );
        assert_eq!(
            store
                .canonical_operations()
                .expect("canonical operations after"),
            operations_before,
            "hostile callbacks must not write canonical operations"
        );
    }

