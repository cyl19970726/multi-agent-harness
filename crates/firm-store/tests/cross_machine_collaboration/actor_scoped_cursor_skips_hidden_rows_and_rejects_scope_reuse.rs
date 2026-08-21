use super::*;

#[test]
fn actor_scoped_cursor_skips_hidden_rows_and_rejects_scope_reuse() {
    let test = TestStore::new("scoped-cursor");
    install_policy(&test.store);
    let auth = authority();
    for ordinal in 1..=4 {
        let mut request = proposal();
        request.delegation_id = format!("delegation-{ordinal}");
        request.operation_id = format!("route-propose-{ordinal}");
        test.store
            .propose_collaboration_delegation(
                &context(
                    auth.source_host.clone(),
                    "delegation.propose",
                    &format!("scope-{ordinal}"),
                    0,
                ),
                &request,
                &auth,
                &policy(),
            )
            .unwrap();
    }
    let filter = CollaborationDelegationFilter::default();
    let first = test
        .store
        .list_collaboration_delegations_for_actor(
            "company-1",
            &auth.source_work_owner,
            &filter,
            None,
            2,
        )
        .unwrap();
    assert_eq!(first.items.len(), 2);
    let cursor = first.next_cursor.clone().expect("bounded next cursor");

    let second = test
        .store
        .list_collaboration_delegations_for_actor(
            "company-1",
            &auth.source_work_owner,
            &filter,
            Some(cursor.clone()),
            10,
        )
        .unwrap();
    assert_eq!(second.items.len(), 2);
    assert_eq!(first.items.len() + second.items.len(), 4);

    assert!(test
        .store
        .list_collaboration_delegations_for_actor(
            "company-1",
            &actor(ActorKind::AgentMember, "hostile"),
            &filter,
            Some(cursor.clone()),
            2,
        )
        .is_err());
    assert!(test
        .store
        .list_collaboration_delegations_for_actor(
            "other-company",
            &auth.source_work_owner,
            &filter,
            Some(cursor),
            2,
        )
        .is_err());
}
