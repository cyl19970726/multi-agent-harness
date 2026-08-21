use super::*;

#[test]
fn schema_strict_without_schema_is_rejected() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    for script in [
        r#"agent("x", schema_strict = True)"#,
        r#"parallel([{"prompt": "x", "schema_strict": True}])"#,
        r#"pipeline(["i"], [{"prompt": "{input}", "schema_strict": True}])"#,
    ] {
        let err = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect_err("schema_strict without a schema should be rejected");
        assert!(
            err.to_string()
                .contains("schema_strict=True requires a schema"),
            "script `{script}` — unexpected error: {err}"
        );
    }
}
