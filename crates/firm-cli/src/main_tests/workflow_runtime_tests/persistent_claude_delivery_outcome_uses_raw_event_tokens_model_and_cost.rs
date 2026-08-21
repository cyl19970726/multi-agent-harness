use super::*;

#[test]
fn persistent_claude_delivery_outcome_uses_raw_event_tokens_model_and_cost() {
    let raw_events = ndjson_values(&[
        r#"{"type":"system","subtype":"init","model":"claude-opus-4-8"}"#,
        r#"{"type":"result","subtype":"success","total_cost_usd":0.025,"usage":{"input_tokens":40,"output_tokens":9}}"#,
    ]);

    let (tokens, cost_usd, model, structured) = claude_delivery_telemetry(&raw_events);
    let outcome = delivery_outcome_for_test(tokens, cost_usd, model, structured);

    assert_eq!(
        outcome.tokens,
        Some(TokenUsage {
            input: 40,
            output: 9,
            total: 49,
        })
    );
    assert_eq!(outcome.model.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(outcome.cost_usd, Some(0.025));
    assert_eq!(outcome.structured, None);
}
