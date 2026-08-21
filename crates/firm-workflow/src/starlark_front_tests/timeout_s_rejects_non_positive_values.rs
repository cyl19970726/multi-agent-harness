use super::*;

#[test]
fn timeout_s_rejects_non_positive_values() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    for script in [
        r#"agent("x", timeout_s = 0)"#,
        r#"parallel([{"prompt": "x", "timeout_s": -1}])"#,
        r#"pipeline(["x"], [{"prompt": "{input}", "timeout_s": 0}])"#,
    ] {
        let err = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect_err("timeout_s should be rejected");
        assert!(
            err.to_string().contains("greater than 0 seconds"),
            "unexpected error: {err}"
        );
    }
}
