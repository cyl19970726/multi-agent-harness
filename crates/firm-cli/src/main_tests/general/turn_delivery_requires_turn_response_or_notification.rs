use super::*;

#[test]
fn turn_delivery_requires_turn_response_or_notification() {
    let initialize_only = vec![serde_json::json!({
        "jsonrpc": "2.0",
        "id": "initialize-rpc",
        "result": {"ok": true}
    })];
    assert!(!turn_exchange_confirms_turn_start(
        &initialize_only,
        "turn-rpc"
    ));

    let turn_response = vec![serde_json::json!({
        "jsonrpc": "2.0",
        "id": "turn-rpc",
        "result": {"ok": true}
    })];
    assert!(turn_exchange_confirms_turn_start(
        &turn_response,
        "turn-rpc"
    ));

    let turn_notification = vec![serde_json::json!({
        "method": "turn/started",
        "params": {"turnId": "turn-1"}
    })];
    assert!(turn_exchange_confirms_turn_start(
        &turn_notification,
        "turn-rpc"
    ));
}
