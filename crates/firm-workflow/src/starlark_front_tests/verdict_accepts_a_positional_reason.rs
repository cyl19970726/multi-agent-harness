use super::*;

#[test]
fn verdict_accepts_a_positional_reason() {
    // issue #139 item 6: `verdict(ok, "msg")` (bare positional reason) must
    // work, not only the keyword form `verdict(ok, reason="msg")`.
    let seen = Mutex::new(Vec::new());
    let script = "agent(\"x\")\nverdict(False, \"a regression slipped through\")\n";
    let run = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok")
    };
    assert_eq!(run.outcome.status, WorkflowRunStatus::Failed);
    let fo = run.outcome.final_output.expect("final_output");
    assert_eq!(
        fo["verdict"]["reason"],
        serde_json::json!("a regression slipped through")
    );
}
