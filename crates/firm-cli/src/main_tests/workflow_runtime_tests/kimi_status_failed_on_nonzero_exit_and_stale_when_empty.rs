use super::*;

#[test]
fn kimi_status_failed_on_nonzero_exit_and_stale_when_empty() {
    assert_eq!(infer_kimi_status(&[], true), ProviderExecutionStatus::Stale);
    assert_eq!(
        infer_kimi_status(
            &[serde_json::json!({"role": "assistant", "content": "x"})],
            false
        ),
        ProviderExecutionStatus::Failed
    );
}
