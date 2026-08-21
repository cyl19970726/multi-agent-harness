use super::*;

#[test]
fn thread_idle_without_turn_id_is_terminal_source_for_active_stream() {
    let idle = serde_json::json!({
        "method": "thread/status/changed",
        "params": {
            "threadId": "thread-1",
            "status": {"type": "idle"}
        }
    });
    let idle_with_turn = serde_json::json!({
        "method": "thread/status/changed",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "status": {"type": "idle"}
        }
    });

    assert_eq!(
        terminal_source_from_values(&[idle]),
        Some(MessageTerminalSource::ThreadIdle)
    );
    assert_eq!(
        terminal_source_from_values(&[idle_with_turn]),
        Some(MessageTerminalSource::ThreadIdle)
    );
}
