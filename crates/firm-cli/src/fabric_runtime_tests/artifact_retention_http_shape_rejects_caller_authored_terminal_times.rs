use super::*;

#[test]
fn artifact_retention_http_shape_rejects_caller_authored_terminal_times() {
    let accepted =
        serde_json::from_value::<CollaborationArtifactRetentionHttpRequest>(serde_json::json!({
            "expected_artifact_revision": 7,
            "retention_duration_ms": 86_400_000,
        }))
        .expect("closed retention request");
    assert_eq!(accepted.expected_artifact_revision, 7);
    for field in [
        "terminal_transport_at_unix_ms",
        "terminal_delegation_at_unix_ms",
        "source_import_completed_at_unix_ms",
    ] {
        let mut hostile = serde_json::json!({
            "expected_artifact_revision": 7,
            "retention_duration_ms": 86_400_000,
        });
        hostile[field] = serde_json::json!(1);
        assert!(
            serde_json::from_value::<CollaborationArtifactRetentionHttpRequest>(hostile).is_err(),
            "caller-supplied {field} must fail before artifact mutation"
        );
    }
}
