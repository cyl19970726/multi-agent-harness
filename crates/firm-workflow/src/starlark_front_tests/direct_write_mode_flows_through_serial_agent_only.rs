use super::*;

#[test]
fn direct_write_mode_flows_through_serial_agent_only() {
    let seen = Mutex::new(Vec::<AgentStepSpec>::new());
    let driver = |spec: &AgentStepSpec| {
        seen.lock().unwrap().push(spec.clone());
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
    let script = r#"
agent(
"make a simple edit directly in the selected repo",
label = "direct-writer",
writable = True,
write_mode = "direct",
)
"#;
    run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].write_mode.as_deref(),
        Some(crate::WRITE_MODE_DIRECT)
    );
    assert!(seen[0].writable);
}
