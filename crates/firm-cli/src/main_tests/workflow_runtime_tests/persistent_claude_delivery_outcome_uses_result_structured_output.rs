use super::*;

#[test]
fn persistent_claude_delivery_outcome_uses_result_structured_output() {
    let raw_events = vec![
        serde_json::json!({"type":"system","subtype":"init","model":"claude-opus-4-8"}),
        serde_json::json!({
            "type":"result",
            "subtype":"success",
            "structured_output": { "verdict": "pass", "score": 100 },
            "total_cost_usd": 0.025,
            "usage": { "input_tokens": 40, "output_tokens": 9 }
        }),
    ];

    let (tokens, cost_usd, model, structured) = claude_delivery_telemetry(&raw_events);
    let outcome = delivery_outcome_for_test(tokens, cost_usd, model, structured);

    assert_eq!(
        outcome.structured,
        Some(serde_json::json!({ "verdict": "pass", "score": 100 }))
    );
    assert_eq!(outcome.cost_usd, Some(0.025));
}
