use super::*;

#[test]
fn unknown_persist_changes_and_write_mode_values_are_rejected() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    for (script, needle) in [
        (
            r#"agent("x", writable = True, persist_changes = "patchh")"#,
            "unknown persist_changes",
        ),
        (
            r#"agent("x", writable = True, write_mode = "sideways")"#,
            "unknown write_mode",
        ),
        (
            r#"parallel([{"prompt": "x", "writable": True, "persist_changes": "keepit"}])"#,
            "unknown persist_changes",
        ),
        (
            r#"pipeline(["i"], [{"prompt": "{input}", "writable": True, "persist_changes": "nope"}])"#,
            "unknown persist_changes",
        ),
    ] {
        let err = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect_err("unknown enum value should be rejected");
        assert!(
            err.to_string().contains(needle),
            "script `{script}` — unexpected error: {err}"
        );
    }
}
