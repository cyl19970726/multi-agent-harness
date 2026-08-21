use super::*;

#[test]
fn provider_interaction_body_is_strict_canonical_json() {
    let request = provider_interaction_request_body();
    let canonical = request.to_canonical_json().expect("canonical request");
    assert_eq!(
        ProviderInteractionRequestBody::parse_canonical_json(&canonical).expect("parse"),
        request
    );

    let reordered = canonical.replacen(
        r#"{"type":"question","prompt":"Choose a path""#,
        r#"{"prompt":"Choose a path","type":"question""#,
        1,
    );
    assert!(
        ProviderInteractionRequestBody::parse_canonical_json(&reordered)
            .expect_err("noncanonical key order")
            .contains("not canonical")
    );
    let with_unknown = canonical.replacen(
        r#"{"type":"question""#,
        r#"{"unknown":true,"type":"question""#,
        1,
    );
    assert!(
        ProviderInteractionRequestBody::parse_canonical_json(&with_unknown)
            .expect_err("unknown body field")
            .contains("unknown field")
    );
}
