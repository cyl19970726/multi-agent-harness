use super::*;

#[test]
fn concurrent_exact_propose_replay_commits_one_relationship() {
    let test = TestStore::new("concurrent-propose");
    install_policy(&test.store);
    let store = Arc::new(test.store.clone());
    let barrier = Arc::new(Barrier::new(8));
    let mut threads = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            let auth = authority();
            barrier.wait();
            store.propose_collaboration_delegation(
                &context(
                    auth.source_host.clone(),
                    "delegation.propose",
                    "concurrent-propose-1",
                    0,
                ),
                &proposal(),
                &auth,
                &policy(),
            )
        }));
    }
    let results = threads
        .into_iter()
        .map(|thread| {
            thread
                .join()
                .expect("proposal thread")
                .expect("exact replay")
        })
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| !result.replayed).count(), 1);
    assert_eq!(results.iter().filter(|result| result.replayed).count(), 7);
    assert_eq!(
        store
            .collaboration_operations()
            .unwrap()
            .iter()
            .filter(|operation| operation.aggregate_kind == "work_delegation_v1")
            .count(),
        1
    );
}
