use super::*;

#[test]
fn captured_meta_is_returned_to_the_caller() {
    // A valid header is captured and returned alongside the outcome.
    let seen = Mutex::new(Vec::new());
    let script =
        "workflow(\"triage\", \"fan out one fix per defect the scan found\")\nagent(\"x\")";
    let run = {
        let driver = recording_driver(&seen);
        run_starlark(script, "demo", None, &driver).expect("run ok")
    };
    assert_eq!(run.meta.name, "triage");
    assert_eq!(
        run.meta.design_intent,
        "fan out one fix per defect the scan found"
    );
    assert_eq!(run.meta.source, script);
    assert_eq!(run.outcome.steps.len(), 1);
}
