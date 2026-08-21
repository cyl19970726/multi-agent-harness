use super::*;

#[test]
fn schema_strict_accepted_with_schema_across_primitives() {
    let schemas = Mutex::new(Vec::new());
    let driver = structured_driver(&schemas);
    for script in [
        r#"agent("x", schema = {"winner": "who"}, schema_strict = True)"#,
        r#"parallel([{"prompt": "x", "schema": {"winner": "who"}, "schema_strict": True}])"#,
        r#"pipeline(["i"], [{"prompt": "{input}", "schema": {"winner": "who"}, "schema_strict": True}])"#,
    ] {
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .unwrap_or_else(|e| panic!("script `{script}` should parse: {e}"));
    }
}
