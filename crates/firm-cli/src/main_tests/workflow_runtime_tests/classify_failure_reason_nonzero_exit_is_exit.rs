use super::*;

#[test]
fn classify_failure_reason_nonzero_exit_is_exit() {
    assert_eq!(classify_failure_reason(false, Some(2), false), Some("exit"));
    // Killed by a signal (no code) without a timeout is still an exit failure.
    assert_eq!(classify_failure_reason(false, None, false), Some("exit"));
}
