use super::*;

#[test]
fn persistent_codex_delivery_outcome_uses_raw_event_tokens_and_spec_model() {
    let spec = launch_spec_with_model_effort(Some("gpt-5-codex"), None);
    let raw_events = ndjson_values(&[
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7}}"#,
    ]);

    let (tokens, cost_usd, model) = codex_delivery_telemetry(&raw_events, &spec);
    let outcome = delivery_outcome_for_test(tokens, cost_usd, model, None);

    assert_eq!(
        outcome.tokens,
        Some(TokenUsage {
            input: 11,
            output: 7,
            total: 18,
        })
    );
    assert_eq!(outcome.model.as_deref(), Some("gpt-5-codex"));
    assert_eq!(outcome.cost_usd, None);
}
