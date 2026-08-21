use super::*;

#[test]
fn kimi_permission_matching_uses_exact_intent_not_option_id_substrings() {
    let (store, _root) = temp_store("kimi-exact-permission-intent");
    let (ledger, member) =
        persisted_native_test_member(&store, "kimi", "kimi_acp", "session-exact-intent");
    for (id, option_id, intent) in [
        (801, "disallow_tool", "reject_once"),
        (802, "not_approved_but_contains_approve", "reject_always"),
        (803, "allowish_display_id", "deny"),
    ] {
        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-exact-intent",
                "options": [{"optionId": option_id, "name": "Misleading", "kind": intent}],
                "toolCall": {"toolCallId": format!("{id}:bash"), "title": "Bash"}
            }
        });
        let outcome = handle_kimi_provider_request(&ledger, &member, &frame)
            .expect("non-allow intent is rejected in-process");
        assert_eq!(outcome.result["outcome"]["outcome"], "cancelled");
    }
    assert!(
        store
            .member_actions()
            .expect("member actions")
            .into_iter()
            .all(|action| action.title != "Kimi full-access tool permission acknowledged"),
        "misleading option ids must never create an approval receipt"
    );
}
