use super::*;

#[test]
fn output_accepts_a_bare_string_and_last_call_wins() {
    // A free-text answer is allowed (stored as a JSON string), and the LAST
    // output() call wins — so a refine loop can overwrite the draft answer.
    let seen = Mutex::new(Vec::new());
    let script = "workflow(\"demo\", \"declare a textual result, then supersede it\")\noutput(\"first draft\")\noutput(\"final answer\")\n";
    let run = {
        let driver = recording_driver(&seen);
        run_starlark(script, "demo", None, &driver).expect("run ok")
    };
    let fo = run.outcome.final_output.expect("final_output present");
    assert_eq!(fo["result"], serde_json::json!("final answer"));
}
