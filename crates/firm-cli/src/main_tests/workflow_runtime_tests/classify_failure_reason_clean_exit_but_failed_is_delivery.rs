use super::*;

#[test]
fn classify_failure_reason_clean_exit_but_failed_is_delivery() {
    // Process exited 0 yet the delivery produced no successful turn (e.g. an
    // auth / usage-limit terminal) → a delivery-layer failure.
    assert_eq!(
        classify_failure_reason(false, Some(0), false),
        Some("delivery")
    );
}
