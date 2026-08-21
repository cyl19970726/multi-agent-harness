use super::*;

#[test]
fn agent_with_schema_returns_a_dict_the_script_reads() {
    // agent(prompt, schema={...}) returns the parsed dict, so the script can
    // read a key off it (res["ok"]) and branch on it.
    let schemas = Mutex::new(Vec::new());
    let script = r#"
res = agent("audit it", schema = {"ok": "", "summary": ""})
if res["ok"] == "v:ok":
    log("structured ok: " + res["summary"])
"#;
    let outcome = {
        let driver = structured_driver(&schemas);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    let schemas = schemas.into_inner().unwrap();
    assert_eq!(schemas.len(), 1);
    // The schema dict threaded onto the spec as a JSON object.
    assert_eq!(
        schemas[0],
        Some(serde_json::json!({ "ok": "", "summary": "" }))
    );
    assert_eq!(outcome.steps.len(), 1);
    // The step carried the parsed structured object.
    assert_eq!(
        outcome.steps[0].structured,
        Some(serde_json::json!({ "ok": "v:ok", "summary": "v:summary" }))
    );
}
