use super::*;

#[test]
fn pipeline_return_status_uses_last_stage_shape() {
    let driver = |spec: &AgentStepSpec| {
        if spec.label == "final" {
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: false,
                output_summary: "schema parse failed".to_string(),
                step_id: None,
                started_at: None,
                details: Some(serde_json::json!({
                    "failure": {
                        "failed": true,
                        "reason": "schema",
                        "detail": "missing required key summary",
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
                output_summary: "scan ok".to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        }
    };
    let script = r#"
results = pipeline(
["alpha"],
[
    {"prompt": "scan {input}", "label": "scan"},
    {"prompt": "summarize {input}", "label": "final", "return_status": True},
],
)
output({
"ok": results[0]["ok"],
"reason": results[0]["reason"],
"detail": results[0]["detail"],
"text": results[0]["text"],
})
verdict(results[0]["reason"] == "schema", "pipeline inspected final failure")
"#;
    let run = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
    assert_eq!(run.outcome.status, WorkflowRunStatus::Completed);
    let fo = run.outcome.final_output.expect("final_output");
    assert_eq!(fo["result"]["ok"], serde_json::json!(false));
    assert_eq!(fo["result"]["reason"], serde_json::json!("schema"));
    assert_eq!(
        fo["result"]["detail"],
        serde_json::json!("missing required key summary")
    );
    assert_eq!(
        fo["result"]["text"],
        serde_json::json!("schema parse failed")
    );
}
