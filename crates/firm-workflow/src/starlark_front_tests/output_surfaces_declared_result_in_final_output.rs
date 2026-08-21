use super::*;

#[test]
fn output_surfaces_declared_result_in_final_output() {
    // output(value) is the run's first-class return: the calling agent reads
    // final_output.result as the one unambiguous answer, verbatim, uncapped —
    // a dict stays a dict (not stringified, not dug out of a step by label).
    let seen = Mutex::new(Vec::new());
    let script = "workflow(\"demo\", \"produce an answer and declare it as the result\")\nagent(\"do the work\")\noutput({\"report\": \"all clear\", \"confirmed\": 3})\n";
    let run = {
        let driver = recording_driver(&seen);
        run_starlark(script, "demo", None, &driver).expect("run ok")
    };
    let fo = run.outcome.final_output.expect("final_output present");
    assert_eq!(fo["result"]["report"], serde_json::json!("all clear"));
    assert_eq!(fo["result"]["confirmed"], serde_json::json!(3));
    // The per-step array still rides alongside under `steps`.
    assert_eq!(fo["steps"].as_array().expect("steps array").len(), 1);
}
