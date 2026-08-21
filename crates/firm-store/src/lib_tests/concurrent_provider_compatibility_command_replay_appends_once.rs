use super::*;

#[test]
fn concurrent_provider_compatibility_command_replay_appends_once() {
    let store = Arc::new(provider_admission_test_store("concurrent-command-replay"));
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for (id, admitted_at, evidence_refs) in [
        ("generated-one", "unix-ms:10", vec!["b", "a", "b"]),
        ("generated-two", "unix-ms:20", vec!["a", "b"]),
    ] {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            let mut admission = provider_compatibility_admission(id, "sdk", "contract-v1");
            admission.admitted_at = admitted_at.into();
            admission.evidence_refs = evidence_refs.into_iter().map(String::from).collect();
            barrier.wait();
            store.ensure_provider_compatibility_admission(&admission)
        }));
    }
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("join").expect("ensure"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.created).count(), 1);
    assert_eq!(results[0].admission.id, results[1].admission.id);
    assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);
}
