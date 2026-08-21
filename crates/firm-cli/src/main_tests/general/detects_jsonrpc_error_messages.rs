use super::*;

#[test]
fn detects_jsonrpc_error_messages() {
    let values = vec![serde_json::json!({
        "jsonrpc": "2.0",
        "id": "turn-rpc",
        "error": {"code": -32602, "message": "bad thread"}
    })];

    assert_eq!(jsonrpc_error_messages(&values), vec!["bad thread"]);
}
