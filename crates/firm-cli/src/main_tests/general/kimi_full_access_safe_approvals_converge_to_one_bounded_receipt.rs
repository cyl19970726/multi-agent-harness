use super::*;

#[test]
fn kimi_full_access_safe_approvals_converge_to_one_bounded_receipt() {
    let (store, _root) = temp_store("kimi-receipt-once");
    let (ledger, member) =
        persisted_native_test_member(&store, "kimi", "kimi_acp", "session-receipt-bound");

    let safe_frame = |id: u64| {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-receipt-bound",
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

    // Many repeated exact approvals: every prompt is still answered with
    // its exact allow intent, and the
    // durable stream converges to ONE bounded provider_control receipt.
    for id in 700..707 {
        let outcome = handle_kimi_provider_request(&ledger, &member, &safe_frame(id))
            .expect("safe acknowledgement");
        let expected_option = format!("tool_allow_always_{id}");
        assert_eq!(
            outcome.result["outcome"]["optionId"].as_str(),
            Some(expected_option.as_str()),
            "each safe prompt must still be acknowledged"
        );
    }

    let receipts = store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .filter(|action| {
            action.member_run_id == member.id && action.action_type == "provider_control"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        receipts.len(),
        1,
        "receipts must converge to one per ProviderRuntimeProjection: {receipts:?}"
    );
    let receipt = &receipts[0];
    assert_eq!(
        receipt.title,
        "Kimi full-access tool permission acknowledged"
    );
    assert_eq!(receipt.status, MemberActionStatus::Succeeded);
    assert!(receipt.summary.contains("exact allow intent"));
    assert!(
        !receipt.summary.contains("sensitive command") && !receipt.title.contains("Bash"),
        "no tool title or command text may be persisted: {receipt:?}"
    );
}
