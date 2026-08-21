use super::*;

#[test]
fn structured_is_surfaced_only_on_succeeded_status() {
    let value = serde_json::json!({ "verdict": "pass" });
    assert_eq!(
        structured_for_status(&ProviderExecutionStatus::Succeeded, Some(value.clone())),
        Some(value.clone())
    );
    // A turn that RAN but did not succeed must not report a (possibly partial /
    // schema-violating) structured result, even if one was extracted.
    for status in [
        ProviderExecutionStatus::Failed,
        ProviderExecutionStatus::Stale,
        ProviderExecutionStatus::Canceled,
        ProviderExecutionStatus::Running,
        ProviderExecutionStatus::Queued,
    ] {
        assert_eq!(structured_for_status(&status, Some(value.clone())), None);
    }
}
