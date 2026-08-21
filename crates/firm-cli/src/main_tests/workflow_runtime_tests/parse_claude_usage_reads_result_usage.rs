use super::*;

#[test]
fn parse_claude_usage_reads_result_usage() {
    // Claude stream-json terminal `result` carries usage.
    let events = ndjson_values(&[
        r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
        r#"{"type":"result","subtype":"success","usage":{"input_tokens":42,"output_tokens":15}}"#,
    ]);
    let usage = parse_claude_usage(&events).expect("usage present");
    assert_eq!((usage.input, usage.output, usage.total), (42, 15, 57));
}
