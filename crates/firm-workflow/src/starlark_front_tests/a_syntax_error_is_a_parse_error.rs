use super::*;

#[test]
fn a_syntax_error_is_a_parse_error() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    let err = run_starlark("agent(", "demo", None, &driver).expect_err("should fail");
    assert!(matches!(err, StarlarkRunError::Parse(_)));
}
