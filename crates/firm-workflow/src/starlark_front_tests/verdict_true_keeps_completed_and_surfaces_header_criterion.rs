use super::*;

#[test]
fn verdict_true_keeps_completed_and_surfaces_header_criterion() {
    let seen = Mutex::new(Vec::new());
    let script = "workflow(\"demo\", \"run and self-assess against a declared bar\", success_criterion = \"all checks green\")\nagent(\"x\")\nverdict(True, reason = \"all green\")\n";
    let run = {
        let driver = recording_driver(&seen);
        run_starlark(script, "demo", None, &driver).expect("run ok")
    };
    assert_eq!(run.outcome.status, WorkflowRunStatus::Completed);
    assert!(run.outcome.summary.contains("met"));
    assert!(run.outcome.summary.contains("all checks green"));
    assert_eq!(
        run.meta.success_criterion.as_deref(),
        Some("all checks green")
    );
}
