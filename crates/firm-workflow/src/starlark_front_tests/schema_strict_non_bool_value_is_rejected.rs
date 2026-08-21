use super::*;

#[test]
fn schema_strict_non_bool_value_is_rejected() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    // A non-bool schema_strict on a spec dict is a type error (dict_bool).
    let script = r#"parallel([{"prompt": "x", "schema": {"w": "who"}, "schema_strict": "yes"}])"#;
    let err = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
        .expect_err("non-bool schema_strict should be rejected");
    assert!(
        err.to_string().contains("schema_strict") && err.to_string().contains("must be a bool"),
        "unexpected error: {err}"
    );
}
