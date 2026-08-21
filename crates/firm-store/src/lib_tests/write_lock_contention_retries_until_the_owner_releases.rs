use super::*;

#[test]
fn write_lock_contention_retries_until_the_owner_releases() {
    let store = Arc::new(lock_policy_test_store("release"));
    let held = hold_store_lock(&store);
    let contender = Arc::clone(&store);
    let waiter = std::thread::spawn(move || {
        contender
            .acquire_write_lock_with_policy(Duration::from_millis(500), Duration::from_millis(2))
    });
    std::thread::sleep(Duration::from_millis(25));
    unlock_file(&held);
    drop(held);
    let acquired = waiter
        .join()
        .expect("contention waiter")
        .expect("waiter acquires after release");
    drop(acquired);
    std::fs::remove_dir_all(store.root()).expect("cleanup store");
}
