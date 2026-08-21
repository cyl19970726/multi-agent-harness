use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn concurrent_current_member_receipts_append_exactly_once() {
    let root = team_test_root("member-action-current-race");
    let store = Arc::new(HarnessStore::new(&root));
    let (_, request) = seed_provider_interaction_bridge(&store, "run-action-race");
    let expected = latest_by_id(store.member_runs().expect("members"), |member| {
        member.id.clone()
    })
    .remove(&request.sender_runtime_id)
    .expect("member");
    let action = provider_control_action(&request.team_run_id, &expected.id);
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|index| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let expected = expected.clone();
            let mut action = action.clone();
            action.id = format!("{}-{index}", action.id);
            std::thread::spawn(move || {
                barrier.wait();
                store.append_member_action_if_member_run_current(&expected, &action)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| {
            handle
                .join()
                .expect("receipt thread")
                .expect("receipt call")
        })
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|appended| **appended).count(), 1);
    assert_eq!(store.member_actions().expect("actions").len(), 1);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
