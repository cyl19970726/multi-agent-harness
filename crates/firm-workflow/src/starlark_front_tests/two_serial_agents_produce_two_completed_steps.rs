use super::*;

#[test]
fn two_serial_agents_produce_two_completed_steps() {
    let seen = Mutex::new(Vec::new());
    let script = r#"
phase("scan")
a = agent("scan the code")
phase("fix")
b = agent("fix what scan found: " + a, provider = "claude", label = "fixer")
"#;
    let outcome = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, "codex"); // default provider → default label
    assert_eq!(seen[1].0, "fixer");
    // The second prompt chained the first's output text.
    assert!(seen[1].1.contains("ok: scan the code"));
    assert_eq!(outcome.status, WorkflowRunStatus::Completed);
    assert_eq!(outcome.steps.len(), 2);
    assert_eq!(outcome.steps[0].phase, "scan");
    assert_eq!(outcome.steps[1].phase, "fix");
    assert_eq!(outcome.steps[1].provider, "claude");
}
