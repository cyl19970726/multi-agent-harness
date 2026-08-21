use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn concurrent_provider_interaction_answers_have_one_winner() {
    let root = team_test_root("provider-interaction-race");
    let store = Arc::new(HarnessStore::new(&root));
    let (request_body, request) = seed_provider_interaction_bridge(&store, "run-interaction-race");
    let barrier = Arc::new(Barrier::new(3));
    let mut joins = Vec::new();
    for choice in ["continue", "stop"] {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let response = provider_interaction_response(&request_body, &request, choice);
        joins.push(std::thread::spawn(move || {
            barrier.wait();
            store.record_provider_interaction_response(&response, "unix-ms:4")
        }));
    }
    barrier.wait();
    let results = joins
        .into_iter()
        .map(|join| join.join().expect("responder"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    assert_eq!(
        store
            .legacy_team_messages()
            .expect("messages")
            .iter()
            .filter(|message| message.kind == ProviderDispatchIntent::ProviderInteractionResponse)
            .count(),
        1
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
