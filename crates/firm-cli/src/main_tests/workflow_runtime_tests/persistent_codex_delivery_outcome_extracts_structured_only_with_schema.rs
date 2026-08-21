use super::*;

#[test]
fn persistent_codex_delivery_outcome_extracts_structured_only_with_schema() {
    let mut spec = launch_spec_with_model_effort(Some("gpt-5-codex"), None);
    spec.output_schema = Some(serde_json::json!({ "verdict": "pass/fail" }));
    let reply = r#"{"verdict":"pass","summary":"done"}"#;

    let outcome = delivery_outcome_for_test(
        None,
        None,
        spec.model.clone(),
        codex_delivery_structured(Some(reply), &spec),
    );

    assert_eq!(
        outcome.structured,
        Some(serde_json::json!({ "verdict": "pass", "summary": "done" }))
    );

    let no_schema = launch_spec_with_model_effort(Some("gpt-5-codex"), None);
    let outcome = delivery_outcome_for_test(
        None,
        None,
        no_schema.model.clone(),
        codex_delivery_structured(Some(reply), &no_schema),
    );

    assert_eq!(outcome.structured, None);
}
