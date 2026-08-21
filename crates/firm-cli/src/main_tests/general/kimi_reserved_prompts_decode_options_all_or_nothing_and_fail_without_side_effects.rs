use super::*;

#[test]
fn kimi_reserved_prompts_decode_options_all_or_nothing_and_fail_without_side_effects() {
    let (store, _root) = temp_store("kimi-all-or-nothing-options");
    let (ledger, member) =
        persisted_native_test_member(&store, "kimi", "kimi_acp", "session-all-or-nothing-options");
    let legacy_messages_before = store
        .legacy_team_messages()
        .expect("Legacy messages before");
    let actions_before = store.member_actions().expect("member actions before");
    let members_before = store.member_runs().expect("member runs before");
    let operations_before = store
        .canonical_operations()
        .expect("canonical operations before");
    let fabric_messages_before = store
        .fabric_messages("unit-test-space")
        .expect("fabric messages before");
    let fabric_deliveries_before = store
        .fabric_message_deliveries("unit-test-space")
        .expect("fabric deliveries before");

    let malformed_options = vec![
        serde_json::json!([
            {"optionId": "q0_opt_0", "name": "Yes", "kind": "allow_once"},
            {"optionId": "q0_skip", "name": "Skip"}
        ]),
        serde_json::json!([
            {"optionId": "plan_approve", "name": "Approve", "kind": "allow_once"},
            {"name": "Revise", "kind": "reject_once"},
            {"optionId": "plan_reject_and_exit", "name": "Reject", "kind": "reject_once"}
        ]),
        serde_json::json!([
            {"optionId": "q0_opt_0", "kind": "allow_once"},
            {"optionId": "q0_skip", "name": "Skip", "kind": "reject_once"}
        ]),
        serde_json::json!([
            {"optionId": 7, "name": "Approve", "kind": "allow_once"},
            {"optionId": "plan_revise", "name": "Revise", "kind": "reject_once"},
            {"optionId": "plan_reject_and_exit", "name": "Reject", "kind": "reject_once"}
        ]),
        serde_json::json!([
            {"optionId": "q0_opt_0", "name": "Yes", "kind": "allow_once"}
        ]),
        serde_json::json!([
            {"optionId": "plan_approve", "name": "Approve", "kind": "allow_once"},
            {"optionId": "plan_revise", "name": "Revise", "kind": "reject_once"}
        ]),
    ];
    for (index, options) in malformed_options.into_iter().enumerate() {
        let title = if index % 2 == 0 {
            "AskUserQuestion"
        } else {
            "ExitPlanMode"
        };
        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 820 + index,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-all-or-nothing-options",
                "options": options,
                "toolCall": {
                    "toolCallId": format!("{}:hostile", 820 + index),
                    "title": title
                }
            }
        });
        let outcome = handle_kimi_provider_request(&ledger, &member, &frame)
            .expect("malformed reserved callback cancels in-process");
        assert_eq!(outcome.result["outcome"]["outcome"], "cancelled");
        assert!(outcome.claimed_response.is_none());
    }

    assert_eq!(
        store.legacy_team_messages().expect("Legacy messages after"),
        legacy_messages_before
    );
    assert_eq!(
        store.member_actions().expect("member actions after"),
        actions_before
    );
    assert_eq!(
        store.member_runs().expect("member runs after"),
        members_before
    );
    assert_eq!(
        store
            .canonical_operations()
            .expect("canonical operations after"),
        operations_before
    );
    assert_eq!(
        store
            .fabric_messages("unit-test-space")
            .expect("fabric messages after"),
        fabric_messages_before
    );
    assert_eq!(
        store
            .fabric_message_deliveries("unit-test-space")
            .expect("fabric deliveries after"),
        fabric_deliveries_before
    );
}
