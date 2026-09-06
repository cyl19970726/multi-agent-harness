use super::*;

#[test]
fn latest_cache_tracks_atomic_commits_and_keeps_scope_inventory() {
    let h = TestStore::new("latest-cache-commits");
    let host = human("host");
    assert!(h.store.canonical_execution_space_ids().unwrap().is_empty());
    for i in 0..4 {
        let id = format!("member-{i}");
        let mut ctx = context(host.clone(), "member.create", &id, 0);
        if i % 2 == 1 {
            ctx.execution_space_id = "second-space".into();
        }
        h.store
            .create_trust_agent_member(&ctx, member(&id, &host))
            .unwrap();
        let observed = h
            .store
            .trust_agent_members(&ctx.execution_space_id)
            .unwrap();
        assert!(observed.iter().any(|m| m.id == id));
        assert_eq!(h.store.read_scan_metrics()[0].decoded_rows, i + 1);
        h.store
            .trust_agent_members(&ctx.execution_space_id)
            .unwrap();
        assert_eq!(h.store.read_scan_metrics()[0].decoded_rows, 0);
    }
    assert_eq!(
        h.store.canonical_execution_space_ids().unwrap(),
        vec!["second-space", SPACE]
    );
    // Every atomic writer replaces the history; no synthetic append shortcut.
    assert_eq!(h.store.canonical_operations().unwrap().len(), 4);
    assert_eq!(h.store.trust_agent_members(SPACE).unwrap().len(), 2);
}
