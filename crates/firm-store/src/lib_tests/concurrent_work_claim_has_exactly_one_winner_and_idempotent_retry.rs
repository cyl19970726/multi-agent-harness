use super::*;

#[test]
fn concurrent_work_claim_has_exactly_one_winner_and_idempotent_retry() {
    let (root, store, run, member_a, member_b) = work_test_fixture("work-claim-race");
    store
        .insert_work(
            unassigned_test_work(&run.id, "work-race"),
            host_work_context("we-race-1", "create-race", "unix-ms:2"),
        )
        .expect("create Work");
    let store = Arc::new(store);
    let barrier = Arc::new(Barrier::new(3));
    let handles = [member_a, member_b]
        .into_iter()
        .enumerate()
        .map(|(index, member)| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.claim_work(
                    "work-race",
                    1,
                    &member.id,
                    member_work_context(
                        &member.id,
                        &format!("we-race-{}", index + 2),
                        &format!("claim-race-{index}"),
                        "unix-ms:3",
                    ),
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim thread"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let winner = results.into_iter().find_map(Result::ok).expect("winner");
    let retry_member = winner
        .active_member_run_id
        .as_deref()
        .expect("active member");
    let retried = store
        .claim_work(
            "work-race",
            1,
            retry_member,
            member_work_context(
                retry_member,
                "ignored",
                if retry_member.ends_with("-a") {
                    "claim-race-0"
                } else {
                    "claim-race-1"
                },
                "unix-ms:4",
            ),
        )
        .expect("idempotent retry");
    assert_eq!(retried, winner);
    assert!(
        store
            .latest_work_deliveries()
            .expect("deliveries")
            .is_empty(),
        "the winning Member already possesses self-claimed Work in its bound runtime"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
