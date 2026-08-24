use super::*;

#[test]
fn transient_idle_work_delivery_snapshot_contention_retries_without_provider_recovery() {
    let unstable = CliError::Store(harness_store::StoreError::CurrentWorkDeliverySnapshotUnstable);
    assert!(retryable_idle_projection::<()>(Err(unstable))
        .expect("exact transient idle projection contention is retryable")
        .is_none());

    assert_eq!(
        retryable_idle_projection::<u64>(Ok(7)).expect("stable projection"),
        Some(7)
    );

    let integrity_conflict = CliError::Store(harness_store::StoreError::Conflict(
        "CURRENT_WORK_DELIVERY_SNAPSHOT_UNSTABLE: forged display prefix".into(),
    ));
    let error = retryable_idle_projection::<()>(Err(integrity_conflict))
        .expect_err("non-transient integrity conflicts remain fail closed");
    assert!(error.to_string().contains("forged display prefix"));
}
