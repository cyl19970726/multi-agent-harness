use super::*;

#[test]
fn parallel_returns_structured_dicts_and_summary_strings_per_spec() {
    // A mixed barrier: one spec has a schema (returns a dict), one does not
    // (returns its summary string). Both flow back through parallel().
    let schemas = Mutex::new(Vec::new());
    let script = r#"
results = parallel([
{"prompt": "a", "schema": {"verdict": ""}},
{"prompt": "b"},
])
log("first verdict: " + results[0]["verdict"])
log("second: " + results[1])
"#;
    let outcome = {
        let driver = structured_driver(&schemas);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    assert_eq!(outcome.steps.len(), 2);
    assert_eq!(
        outcome.steps[0].structured,
        Some(serde_json::json!({ "verdict": "v:verdict" }))
    );
    assert!(outcome.steps[1].structured.is_none());
}
