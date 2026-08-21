use super::*;

#[test]
fn extracts_thread_id_from_thread_start_response_before_turn_start() {
    let values = vec![serde_json::json!({
        "jsonrpc": "2.0",
        "id": "thread-rpc",
        "result": {"thread": {"id": "real-thread-1"}}
    })];

    assert_eq!(
        extract_thread_id(&values, "thread-rpc").as_deref(),
        Some("real-thread-1")
    );
    assert_eq!(extract_thread_id(&values, "other-rpc"), None);
}
