use super::*;

#[test]
fn parse_codex_usage_absent_is_none() {
    let events = ndjson_values(&[r#"{"type":"turn.completed"}"#, r#"{"type":"item.started"}"#]);
    assert!(parse_codex_usage(&events).is_none());
}
