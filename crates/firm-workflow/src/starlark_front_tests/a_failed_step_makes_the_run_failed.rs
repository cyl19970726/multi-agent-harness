use super::*;

#[test]
fn a_failed_step_makes_the_run_failed() {
    // A driver that fails every step → 0 ok → Failed (outcome_from_steps rule).
    let driver = |spec: &AgentStepSpec| StepResult {
        phase: spec.phase.clone(),
        label: spec.label.clone(),
        provider: spec.provider.clone(),
        isolation: spec.isolation.clone(),
        ok: false,
        output_summary: "boom".to_string(),
        step_id: None,
        started_at: None,
        details: None,
        structured: None,
        ordinal: None,
    };
    let outcome = run_starlark(&format!("{HEADER}agent(\"x\")"), "demo", None, &driver)
        .expect("run ok")
        .outcome;
    assert_eq!(outcome.status, WorkflowRunStatus::Failed);
    assert_eq!(outcome.steps.len(), 1);
}
