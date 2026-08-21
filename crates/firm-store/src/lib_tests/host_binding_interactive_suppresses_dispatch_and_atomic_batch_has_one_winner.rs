use super::*;

#[test]
fn host_binding_interactive_suppresses_dispatch_and_atomic_batch_has_one_winner() {
    let root = team_test_root("host-binding-dispatch-race");
    let store = HarnessStore::new(&root);
    let (run, member, work) =
        seed_host_attention_fixture(&store, "lease-dispatch", Some("thread-a"));
    seed_test_host_attention(
        &store,
        &run,
        &member,
        &work,
        "attention-dispatch-race",
        "unix-ms:10",
    );
    let interactive = store
        .acquire_host_binding_lease(
            &run.id,
            "codex-app",
            "thread-a",
            HostBindingLeaseOwnerKind::Interactive,
            "human",
            "lease-human",
            100,
            10,
        )
        .unwrap();
    let suppressed = store.claim_dispatcher_host_attention_batch(
        &interactive,
        100,
        10,
        "suppressed",
        101,
        "unix-ms:101",
    );
    assert!(suppressed
        .unwrap_err()
        .to_string()
        .contains("INTERACTIVE_SUPPRESSES_DISPATCH"));

    let dispatcher = store
        .acquire_host_binding_lease(
            &run.id,
            "codex-app",
            "thread-a",
            HostBindingLeaseOwnerKind::Dispatcher,
            "dispatcher",
            "lease-dispatcher",
            110,
            100,
        )
        .expect("take over expired interactive lease");
    let store = std::sync::Arc::new(store);
    let handles = (0..2)
        .map(|index| {
            let store = std::sync::Arc::clone(&store);
            let dispatcher = dispatcher.clone();
            std::thread::spawn(move || {
                store
                    .claim_dispatcher_host_attention_batch(
                        &dispatcher,
                        100,
                        10,
                        &format!("batch-{index}"),
                        111,
                        "unix-ms:111",
                    )
                    .expect("batch claim")
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim thread"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|batch| !batch.is_empty()).count(), 1);
    assert_eq!(results.iter().map(Vec::len).sum::<usize>(), 1);
    let claimed = results.into_iter().flatten().next().unwrap();
    assert_eq!(claimed.claimed_host_lease_generation, Some(2));

    let released = store
        .release_host_binding_lease(&dispatcher, 112)
        .expect("release dispatcher");
    assert!(store
        .complete_host_attention_claim(
            &claimed.id,
            claimed.claim_id.as_deref().unwrap(),
            "receipt",
            "unix-ms:113",
        )
        .unwrap_err()
        .to_string()
        .contains("LEASE_FENCED"));
    assert_eq!(released.status, HostBindingLeaseStatus::Released);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
