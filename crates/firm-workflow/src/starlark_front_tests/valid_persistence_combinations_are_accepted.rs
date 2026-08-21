use super::*;

#[test]
fn valid_persistence_combinations_are_accepted() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    for script in [
        r#"agent("x", writable = True, persist_changes = "patch", auto_apply_on_verdict = True)"#,
        r#"agent("x", writable = True, persist_changes = "discard")"#,
        r#"agent("x", persist_changes = "discard")"#,
        r#"parallel([{"prompt": "x", "writable": True, "persist_changes": "patch"}])"#,
    ] {
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .unwrap_or_else(|e| panic!("script `{script}` should parse: {e}"));
    }
}
