use super::*;

#[test]
fn pipeline_forward_injects_structured_output_into_next_stage() {
    // Stage 1 carries a schema; its parsed structured JSON (serialized) must be
    // forward-injected into stage 2's `{input}` placeholder.
    let schemas = Mutex::new(Vec::new());
    let prompts = Mutex::new(Vec::new());
    let script = r#"
pipeline(
["item-x"],
[
    {"prompt": "classify {input}", "schema": {"verdict": ""}},
    {"prompt": "act on {input}", "label": "s2"},
],
)
"#;
    let outcome = {
        // Wrap structured_driver so we also capture stage 2's concrete prompt.
        let inner = structured_driver(&schemas);
        let driver = |spec: &AgentStepSpec| {
            if spec.label == "s2" {
                prompts.lock().unwrap().push(spec.prompt.clone());
            }
            inner(spec)
        };
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    assert_eq!(outcome.steps.len(), 2);
    // Stage 1 produced a structured dict.
    assert!(outcome.steps.iter().any(|s| s.structured.is_some()));
    // Stage 2's prompt carries stage 1's serialized structured JSON.
    let prompts = prompts.into_inner().unwrap();
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts[0].contains("verdict"),
        "stage 2 prompt must carry stage 1's structured JSON, got: {}",
        prompts[0]
    );
}
