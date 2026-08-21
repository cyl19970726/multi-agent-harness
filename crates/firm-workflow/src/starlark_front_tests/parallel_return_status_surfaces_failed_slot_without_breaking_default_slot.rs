use super::*;

#[test]
fn parallel_return_status_surfaces_failed_slot_without_breaking_default_slot() {
    let driver = |spec: &AgentStepSpec| {
        if spec.label == "bad" {
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: false,
                output_summary: "timed out waiting for provider output".to_string(),
                step_id: None,
                started_at: None,
                details: Some(serde_json::json!({
                    "failure": {
                        "failed": true,
                        "reason": "timeout",
                        "detail": "leaf exceeded timeout_s=1",
                    }
                })),
                structured: None,
                ordinal: None,
            }
        } else {
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                output_summary: "plain success".to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        }
    };
    let script = r#"
results = parallel([
{"prompt": "fail", "label": "bad", "return_status": True},
{"prompt": "succeed", "label": "good"},
])
output({
"bad_ok": results[0]["ok"],
"bad_reason": results[0]["reason"],
"bad_detail": results[0]["detail"],
"good_text": results[1],
})
"#;
    let run = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
    let fo = run.outcome.final_output.expect("final_output");
    assert_eq!(fo["result"]["bad_ok"], serde_json::json!(false));
    assert_eq!(fo["result"]["bad_reason"], serde_json::json!("timeout"));
    assert_eq!(
        fo["result"]["bad_detail"],
        serde_json::json!("leaf exceeded timeout_s=1")
    );
    assert_eq!(
        fo["result"]["good_text"],
        serde_json::json!("plain success")
    );
}
