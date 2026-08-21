use super::*;

#[test]
fn artifact_complete_body_limit_matches_the_frozen_64_mib_contract_only() {
    assert_eq!(
        fabric_http_body_limit("POST", "/v1/fabric/artifacts/artifact-a/complete"),
        ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
    );
    const { assert!(ARTIFACT_COMPLETE_HTTP_BODY_LIMIT > MAX_FABRIC_ARTIFACT_BYTES * 2) };
    assert_eq!(
        fabric_http_body_limit("POST", "/v1/fabric/artifacts/initiate"),
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    );
    assert_eq!(
        fabric_http_body_limit("GET", "/v1/fabric/artifacts/artifact-a/complete"),
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    );
    assert_eq!(
        fabric_http_body_limit("POST", "/v1/fabric/artifacts/a/nested/complete"),
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    );
}
