use super::*;

#[test]
fn host_binding_stale_attention_is_derived_and_idempotent() {
    let root = team_test_root("host-binding-stale-attention");
    let store = HarnessStore::new(&root);
    let (run, _, _) = seed_host_attention_fixture(&store, "lease-stale", Some("thread-a"));

    let first = store
        .reconcile_host_binding_stale_attentions(100, "unix-ms:100")
        .expect("derive unleased attention");
    let retry = store
        .reconcile_host_binding_stale_attentions(101, "unix-ms:101")
        .expect("repeat scan");
    assert_eq!(first.len(), 1);
    assert_eq!(retry.len(), 1);
    assert_eq!(first[0].id, retry[0].id);
    assert_eq!(first[0].kind, HostAttentionKind::HostBindingStale);
    assert_eq!(
        store
            .host_attentions()
            .unwrap()
            .into_iter()
            .filter(|attention| attention.kind == HostAttentionKind::HostBindingStale)
            .count(),
        1
    );

    let lease = store
        .acquire_host_binding_lease(
            &run.id,
            "codex",
            "thread-a",
            HostBindingLeaseOwnerKind::Interactive,
            "human",
            "lease-live",
            110,
            10,
        )
        .unwrap();
    assert!(store
        .reconcile_host_binding_stale_attentions(119, "unix-ms:119")
        .unwrap()
        .is_empty());
    let expired = store
        .reconcile_host_binding_stale_attentions(120, "unix-ms:120")
        .unwrap();
    assert_eq!(expired.len(), 1);
    assert_ne!(expired[0].id, first[0].id);
    assert_eq!(lease.generation, 1);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
