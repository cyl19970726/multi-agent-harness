use super::*;

#[test]
fn agent_without_schema_returns_the_text_summary() {
    // No schema -> the script gets the output_summary STRING exactly as today.
    let schemas = Mutex::new(Vec::new());
    let script = r#"
out = agent("scan it")
log("got: " + out)
"#;
    let outcome = {
        let driver = structured_driver(&schemas);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    assert_eq!(outcome.steps.len(), 1);
    assert!(outcome.steps[0].structured.is_none());
    assert_eq!(outcome.steps[0].output_summary, "text: scan it");
}
