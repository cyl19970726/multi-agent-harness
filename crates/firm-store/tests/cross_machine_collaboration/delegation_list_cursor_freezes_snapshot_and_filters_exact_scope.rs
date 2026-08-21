use super::*;

#[test]
fn delegation_list_cursor_freezes_snapshot_and_filters_exact_scope() {
    let test = TestStore::new("cursor");
    install_policy(&test.store);
    let auth = authority();
    for ordinal in 1..=3 {
        let mut request = proposal();
        request.delegation_id = format!("delegation-{ordinal}");
        request.operation_id = format!("route-propose-{ordinal}");
        test.store
            .propose_collaboration_delegation(
                &context(
                    auth.source_host.clone(),
                    "delegation.propose",
                    &format!("cursor-propose-{ordinal}"),
                    0,
                ),
                &request,
                &auth,
                &policy(),
            )
            .expect("cursor proposal");
    }
    let filter = CollaborationDelegationFilter {
        source_team_id: Some("team-a".into()),
        target_team_id: Some("team-b".into()),
        node_id: Some("node-b".into()),
        state: Some(DelegationState::AwaitingTargetDecision),
    };
    let first = test
        .store
        .list_collaboration_delegations("company-1", &filter, None, 2)
        .expect("first frozen page");
    assert_eq!(first.items.len(), 2);
    let cursor = first.next_cursor.expect("third item remains");

    let mut fourth = proposal();
    fourth.delegation_id = "delegation-4".into();
    fourth.operation_id = "route-propose-4".into();
    test.store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "cursor-propose-4",
                0,
            ),
            &fourth,
            &auth,
            &policy(),
        )
        .expect("fourth proposal after first page");

    let second = test
        .store
        .list_collaboration_delegations("company-1", &filter, Some(cursor), 2)
        .expect("second page from frozen sequence");
    assert_eq!(second.items.len(), 1);
    assert!(second.next_cursor.is_none());
    assert_eq!(second.as_of_store_sequence, first.as_of_store_sequence);

    let fresh = test
        .store
        .list_collaboration_delegations("company-1", &filter, None, 10)
        .expect("fresh view includes later proposal");
    assert_eq!(fresh.items.len(), 4);
    assert!(fresh.as_of_store_sequence > first.as_of_store_sequence);
}
