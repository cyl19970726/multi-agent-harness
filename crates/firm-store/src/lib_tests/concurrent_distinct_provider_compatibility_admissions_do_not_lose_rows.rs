use super::*;

#[test]
fn concurrent_distinct_provider_compatibility_admissions_do_not_lose_rows() {
    let store = Arc::new(provider_admission_test_store("concurrent"));
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for (id, mode) in [("one", "sdk"), ("two", "interactive")] {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            let admission = provider_compatibility_admission(id, mode, "contract-v1");
            barrier.wait();
            store.admit_provider_compatibility(&admission)
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().expect("join").expect("append");
    }
    assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 2);
}
