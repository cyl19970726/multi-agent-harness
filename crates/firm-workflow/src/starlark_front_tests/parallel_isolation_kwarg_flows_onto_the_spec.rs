use super::*;

#[test]
fn parallel_isolation_kwarg_flows_onto_the_spec() {
    // The `isolation` dict key must reach the AgentStepSpec the driver sees.
    let isolations = Mutex::new(Vec::<Option<String>>::new());
    let script = r#"parallel([{"prompt": "edit", "isolation": "worktree"}])"#;
    let outcome = {
        let driver = |spec: &AgentStepSpec| {
            isolations.lock().unwrap().push(spec.isolation.clone());
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                output_summary: "ok".to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        };
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    let isolations = isolations.into_inner().unwrap();
    assert_eq!(isolations, vec![Some("worktree".to_string())]);
    assert_eq!(outcome.steps.len(), 1);
    assert_eq!(outcome.steps[0].isolation.as_deref(), Some("worktree"));
}
