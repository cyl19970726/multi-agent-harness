use super::*;

#[test]
fn blank_or_short_design_intent_is_rejected() {
    let seen = Mutex::new(Vec::new());
    let driver = recording_driver(&seen);
    // Too short (< MIN_DESIGN_INTENT_LEN) and blank both fail.
    for intent in ["too short", "   "] {
        let script = format!("workflow(\"demo\", \"{intent}\")\nagent(\"x\")");
        let err = run_starlark(&script, "demo", None, &driver).expect_err("rejected");
        assert!(
            matches!(err, StarlarkRunError::MissingDesignIntent(_)),
            "intent {intent:?} should be rejected"
        );
    }
}
