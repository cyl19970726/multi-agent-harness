use super::*;

#[test]
fn missing_workflow_header_is_rejected() {
    // A program that never calls `workflow(...)` is rejected fail-fast.
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    let err = run_starlark(r#"agent("x")"#, "demo", None, &driver).expect_err("rejected");
    assert!(matches!(err, StarlarkRunError::MissingDesignIntent(_)));
    assert!(err.to_string().contains("design_intent"));
}
