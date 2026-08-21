use super::*;

#[test]
fn direct_write_mode_is_rejected_in_parallel_and_pipeline_specs() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    for script in [
        r#"parallel([{"prompt": "edit", "writable": True, "write_mode": "direct"}])"#,
        r#"pipeline(["x"], [{"prompt": "edit {input}", "writable": True, "write_mode": "direct"}])"#,
    ] {
        let err = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect_err("direct write mode should be rejected in concurrent specs");
        assert!(
            err.to_string().contains("write_mode=\"direct\""),
            "unexpected error: {err}"
        );
    }
}
