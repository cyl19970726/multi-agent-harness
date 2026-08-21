use super::*;

#[test]
fn agent_return_status_allows_failure_reason_retry() {
    let driver = |spec: &AgentStepSpec| {
        if spec.label == "flaky" {
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: false,
                output_summary: "delivery failed before response".to_string(),
                step_id: None,
                started_at: None,
                details: Some(serde_json::json!({
                    "failure": {
                        "failed": true,
                        "reason": "delivery",
                        "detail": "provider stream closed before final answer",
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
                output_summary: "retry recovered".to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        }
    };
    let script = r#"
first = agent("try once", label = "flaky", return_status = True)
if not first["ok"] and first["reason"] == "delivery":
    second = agent("retry once", label = "retry", return_status = True)
    output({
        "retried": True,
        "first_reason": first["reason"],
        "first_detail": first["detail"],
        "second_ok": second["ok"],
        "second_text": second["text"],
    })
    verdict(second["ok"], "retried after " + first["reason"])
else:
    output({"retried": False, "first": first})
    verdict(False, "unexpected first result")
"#;
    let run = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");

    assert_eq!(run.outcome.status, WorkflowRunStatus::Completed);
    assert_eq!(run.outcome.steps.len(), 2);
    assert!(!run.outcome.steps[0].ok);
    assert!(run.outcome.steps[1].ok);
    let fo = run.outcome.final_output.expect("final_output");
    assert_eq!(fo["result"]["retried"], serde_json::json!(true));
    assert_eq!(fo["result"]["first_reason"], serde_json::json!("delivery"));
    assert_eq!(
        fo["result"]["first_detail"],
        serde_json::json!("provider stream closed before final answer")
    );
    assert_eq!(fo["result"]["second_ok"], serde_json::json!(true));
    assert_eq!(
        fo["result"]["second_text"],
        serde_json::json!("retry recovered")
    );
}
