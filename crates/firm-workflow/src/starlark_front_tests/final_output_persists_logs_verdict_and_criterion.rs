use super::*;

#[test]
fn final_output_persists_logs_verdict_and_criterion() {
    // log() lines, the typed verdict, and the success_criterion must survive
    // the run in final_output (previously logs were dropped entirely and the
    // verdict/criterion lived only in summary prose).
    let seen = Mutex::new(Vec::new());
    let script = "workflow(\"demo\", \"do work then self-assess\", success_criterion = \"tests pass\")\nlog(\"starting the scan\")\nagent(\"x\")\nlog(\"scan done\")\nverdict(False, reason = \"a test regressed\")\n";
    let run = {
        let driver = recording_driver(&seen);
        run_starlark(script, "demo", None, &driver).expect("run ok")
    };
    let fo = run.outcome.final_output.expect("final_output present");
    let logs = fo["logs"].as_array().expect("logs array");
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0], serde_json::json!("starting the scan"));
    assert_eq!(fo["verdict"]["ok"], serde_json::json!(false));
    assert_eq!(
        fo["verdict"]["reason"],
        serde_json::json!("a test regressed")
    );
    assert_eq!(fo["success_criterion"], serde_json::json!("tests pass"));
    // The per-step array is preserved under `steps`.
    assert!(fo["steps"].as_array().expect("steps array").len() == 1);
    // And the verdict still drove the status.
    assert_eq!(run.outcome.status, WorkflowRunStatus::Failed);
    // No output() was declared, so the run's result is null (not omitted), so a
    // caller can distinguish "no declared answer" from a missing field.
    assert_eq!(fo["result"], serde_json::Value::Null);
}
