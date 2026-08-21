use super::*;

#[test]
fn classify_failure_reason_timeout_dominates() {
    // Timeout fired (and killed the child, so exit_code is None) → "timeout".
    assert_eq!(classify_failure_reason(false, None, true), Some("timeout"));
    // Even with a code present, a fired timeout still classifies as timeout.
    assert_eq!(
        classify_failure_reason(false, Some(1), true),
        Some("timeout")
    );
}
