use super::*;

#[test]
fn kimi_invalid_titles_and_mixed_options_fail_before_any_durable_effect() {
    let (store, _root) = temp_store("kimi-invalid-title-fail-closed");
    let (ledger, member) = persisted_native_test_member(
        &store,
        "kimi",
        "kimi_acp",
        "session-invalid-title-fail-closed",
    );
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

    let title_cases = [
        None,
        Some(serde_json::Value::Null),
        Some(serde_json::json!(7)),
        Some(serde_json::json!("")),
        Some(serde_json::json!("   ")),
    ];
    for (index, title) in title_cases.into_iter().enumerate() {
        let mut tool_call = serde_json::json!({
            "toolCallId": format!("{}:missing-title", 840 + index)
        });
        if let Some(title) = title {
            tool_call
                .as_object_mut()
                .expect("toolCall object")
                .insert("title".into(), title);
        }
        let options = if index % 2 == 0 {
            serde_json::json!([
                {"optionId": "q0_opt_0", "name": "Yes", "kind": "allow_once"},
                {"optionId": "q0_skip", "name": "Skip", "kind": "reject_once"}
            ])
        } else {
            serde_json::json!([
                {"optionId": "plan_approve", "name": "Approve", "kind": "allow_once"},
                {"optionId": "plan_revise", "name": "Revise", "kind": "reject_once"},
                {"optionId": "plan_reject_and_exit", "name": "Reject", "kind": "reject_once"}
            ])
        };
        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 840 + index,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-invalid-title-fail-closed",
                "options": options,
                "toolCall": tool_call
            }
        });
        let outcome = handle_kimi_provider_request(&ledger, &member, &frame)
            .expect("invalid title must cancel in-process");
        assert_eq!(outcome.result["outcome"]["outcome"], "cancelled");
        assert!(outcome.claimed_response.is_none());
    }

    for (index, (title, options)) in [
        (
            "AskUserQuestion",
            serde_json::json!([
                {"optionId": "q0_opt_0", "name": "Yes", "kind": "allow_once"},
                {"optionId": "q0_skip", "name": "Skip", "kind": "future_reject"}
            ]),
        ),
        (
            "ExitPlanMode",
            serde_json::json!([
                {"optionId": "plan_approve", "name": "Approve", "kind": "allow_once"},
                {"optionId": "plan_revise", "name": "Revise"},
                {"optionId": "plan_reject_and_exit", "name": "Reject", "kind": "reject_once"}
            ]),
        ),
        (
            "Bash",
            serde_json::json!([
                {"optionId": "approve_once", "name": "Approve", "kind": "allow_once"},
                {"optionId": "reject", "name": "Reject", "kind": "future_reject"}
            ]),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 850 + index,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-invalid-title-fail-closed",
                "options": options,
                "toolCall": {
                    "toolCallId": format!("{}:mixed", 850 + index),
                    "title": title
                }
            }
        });
        let outcome = handle_kimi_provider_request(&ledger, &member, &frame)
            .expect("malformed mixed options must cancel in-process");
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
