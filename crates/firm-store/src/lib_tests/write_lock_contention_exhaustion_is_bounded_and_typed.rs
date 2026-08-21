use super::*;

#[test]
fn write_lock_contention_exhaustion_is_bounded_and_typed() {
    let store = lock_policy_test_store("timeout");
    let held = hold_store_lock(&store);
    let started = Instant::now();
    let error = match store
        .acquire_write_lock_with_policy(Duration::from_millis(25), Duration::from_millis(2))
    {
        Ok(_) => panic!("held lock must exhaust the short test policy"),
        Err(error) => error,
    };
    let elapsed = started.elapsed();
    assert!(matches!(error, StoreError::LockTimeout(_)));
    assert!(elapsed >= Duration::from_millis(20), "elapsed={elapsed:?}");
    assert!(elapsed < Duration::from_millis(500), "elapsed={elapsed:?}");
    unlock_file(&held);
    drop(held);
    std::fs::remove_dir_all(store.root()).expect("cleanup store");
}
