use super::*;

#[test]
fn parse_codex_usage_accepts_nested_turn_usage_and_legacy_name() {
    let events = ndjson_values(&[
        r#"{"type":"turn_completed","turn":{"usage":{"input_tokens":5,"output_tokens":7}}}"#,
    ]);
    let usage = parse_codex_usage(&events).expect("usage present");
    assert_eq!((usage.input, usage.output, usage.total), (5, 7, 12));
}
