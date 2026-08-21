use super::*;

#[test]
fn kimi_full_access_receipt_ignores_unrelated_provider_control_rows() {
    let (store, _root) = temp_store("kimi-receipt-identity");
    let (ledger, member) =
        persisted_native_test_member(&store, "kimi", "kimi_acp", "session-receipt-identity");

    // An unrelated provider_control row for the same ProviderRuntimeProjection must not
    // suppress the bounded Kimi receipt: the convergence key is the
    // stable (member_run_id, action_type, title) identity, not the bare
    // action type.
    ledger
        .append_provider_control_receipt_once(
            &member,
            "Unrelated provider control observation",
            "pre-existing control row from another control path",
        )
        .expect("seed unrelated provider_control row");

    let safe_frame = |id: u64| {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-receipt-identity",
                "options": [
                    {"optionId": format!("tool_allow_always_{id}"), "name": "Always allow", "kind": "allow_always"},
                    {"optionId": "tool_reject_once", "name": "Reject", "kind": "reject_once"}
                ],
                "toolCall": {
                    "toolCallId": format!("{id}:bash"),
                    "title": "Bash",
                    "content": [{"type": "content", "content": {"type": "text", "text": format!("Run sensitive command number {id}?")}}]
                }
            }
        })
    };

    for id in 710..713 {
        let outcome = handle_kimi_provider_request(&ledger, &member, &safe_frame(id))
            .expect("safe acknowledgement");
        let expected_option = format!("tool_allow_always_{id}");
        assert_eq!(
            outcome.result["outcome"]["optionId"].as_str(),
            Some(expected_option.as_str()),
            "each safe prompt must still be acknowledged"
        );
    }

    let actions = store.member_actions().expect("member actions");
    let kimi_receipts = actions
        .iter()
        .filter(|action| {
            action.member_run_id == member.id
                && action.action_type == "provider_control"
                && action.title == "Kimi full-access tool permission acknowledged"
        })
        .count();
    assert_eq!(
        kimi_receipts, 1,
        "first safe approval writes exactly one Kimi receipt; later ones converge: {actions:?}"
    );
    let member_controls = actions
        .iter()
        .filter(|action| {
            action.member_run_id == member.id && action.action_type == "provider_control"
        })
        .count();
    assert_eq!(
        member_controls, 2,
        "the seeded unrelated row is preserved alongside the one bounded receipt: {actions:?}"
    );
}
