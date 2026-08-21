use super::*;

#[test]
fn parse_claude_usage_absent_is_none() {
    let events = ndjson_values(&[r#"{"type":"result","subtype":"success"}"#]);
    assert!(parse_claude_usage(&events).is_none());
}
