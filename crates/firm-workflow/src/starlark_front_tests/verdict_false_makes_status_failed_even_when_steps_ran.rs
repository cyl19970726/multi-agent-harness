use super::*;

#[test]
fn verdict_false_makes_status_failed_even_when_steps_ran() {
    // A successful step + verdict(False) -> the run is Failed: "workers ran"
    // is no longer "intent satisfied".
    let seen = Mutex::new(Vec::new());
    let script =
        "\nagent(\"do the work\")\nverdict(False, reason = \"a regression slipped through\")\n";
    let run = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok")
    };
    assert_eq!(run.outcome.status, WorkflowRunStatus::Failed);
    assert!(run.outcome.summary.contains("NOT met"));
    assert!(run.outcome.summary.contains("regression"));
    // The step itself still ran fine — the verdict overrides the status only.
    assert_eq!(run.outcome.steps.len(), 1);
    assert!(run.outcome.steps[0].ok);
}
