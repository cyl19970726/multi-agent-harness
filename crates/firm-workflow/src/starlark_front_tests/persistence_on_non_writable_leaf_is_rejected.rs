use super::*;

#[test]
fn persistence_on_non_writable_leaf_is_rejected() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    for (script, needle) in [
        (
            r#"agent("x", auto_apply_on_verdict = True)"#,
            "auto_apply_on_verdict=True requires writable=True",
        ),
        (
            r#"agent("x", persist_changes = "patch")"#,
            "persist_changes=\"patch\" requires writable=True",
        ),
        (
            r#"parallel([{"prompt": "x", "auto_apply_on_verdict": True}])"#,
            "auto_apply_on_verdict=True requires writable=True",
        ),
        (
            r#"parallel([{"prompt": "x", "persist_changes": "patch"}])"#,
            "persist_changes=\"patch\" requires writable=True",
        ),
        (
            r#"pipeline(["i"], [{"prompt": "{input}", "auto_apply_on_verdict": True}])"#,
            "auto_apply_on_verdict=True requires writable=True",
        ),
    ] {
        let err = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect_err("persistence on non-writable leaf should be rejected");
        assert!(
            err.to_string().contains(needle),
            "script `{script}` — unexpected error: {err}"
        );
    }
}
