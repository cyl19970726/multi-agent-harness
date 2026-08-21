use super::*;

#[test]
fn schema_correction_retry_limits_are_short_and_never_expand_existing_caps() {
    assert_eq!(
        schema_correction_retry_limits(900_000, None),
        (
            SCHEMA_CORRECTION_RETRY_TIMEOUT_MS,
            Some(SCHEMA_CORRECTION_RETRY_TIMEOUT_MS)
        )
    );
    assert_eq!(
        schema_correction_retry_limits(5_000, Some(10_000)),
        (5_000, Some(10_000))
    );
    assert_eq!(
        schema_correction_retry_limits(900_000, Some(15_000)),
        (SCHEMA_CORRECTION_RETRY_TIMEOUT_MS, Some(15_000))
    );
}
