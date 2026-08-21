use super::*;

#[test]
fn host_binding_lease_acquire_renew_release_takeover_and_stale_fence() {
    let root = team_test_root("host-binding-lease-lifecycle");
    let store = HarnessStore::new(&root);
    let (run, _, _) = seed_host_attention_fixture(&store, "lease-lifecycle", Some("thread-a"));

    assert_eq!(
        store.latest_host_binding_lease(&run.id).unwrap(),
        None,
        "legacy binding is explicitly unleased"
    );
    let first = store
        .acquire_host_binding_lease(
            &run.id,
            "codex-app",
            "thread-a",
            HostBindingLeaseOwnerKind::Interactive,
            "human-a",
            "lease-a",
            100,
            50,
        )
        .expect("acquire interactive lease");
    assert_eq!(first.generation, 1);
    assert_eq!(
        store.effective_host_binding_lease_at(&run.id, 149).unwrap(),
        Some(first.clone())
    );
    assert!(store
        .effective_host_binding_lease_at(&run.id, 150)
        .unwrap()
        .is_none());

    let second = store
        .acquire_host_binding_lease(
            &run.id,
            "codex-app",
            "thread-a",
            HostBindingLeaseOwnerKind::Dispatcher,
            "dispatcher-b",
            "lease-b",
            150,
            100,
        )
        .expect("expired takeover");
    assert_eq!(second.generation, 2);
    assert!(store.renew_host_binding_lease(&first, 151, 100).is_err());
    let renewed = store
        .renew_host_binding_lease(&second, 175, 100)
        .expect("renew exact lease");
    assert_eq!(renewed.expires_unix_ms, 275);
    let released = store
        .release_host_binding_lease(&renewed, 180)
        .expect("release exact lease");
    assert_eq!(released.status, HostBindingLeaseStatus::Released);
    assert!(store.renew_host_binding_lease(&renewed, 181, 100).is_err());
    assert_eq!(
        store
            .release_host_binding_lease(&released, 999)
            .expect("release retry")
            .released_unix_ms,
        Some(180)
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
