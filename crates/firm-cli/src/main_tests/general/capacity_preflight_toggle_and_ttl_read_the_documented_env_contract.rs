use super::*;

#[test]
fn capacity_preflight_toggle_and_ttl_read_the_documented_env_contract() {
    // Defaults are on and five minutes; this test asserts the parse rules,
    // not the ambient process env.
    assert_eq!(
        harness_core::PROVIDER_CAPACITY_DEFAULT_TTL_MS,
        5 * 60 * 1000
    );
    assert_eq!(
        parse_unix_ms_timestamp("unix-ms:1785573368310"),
        Some(1785573368310)
    );
    assert_eq!(parse_unix_ms_timestamp("2026-08-01T00:00:00Z"), None);
}
