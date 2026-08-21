use super::*;

#[test]
fn accept_cancel_before_accept_race_has_one_linearized_winner() {
    let test = TestStore::new("accept-cancel-race");
    install_policy(&test.store);
    let auth = authority();
    test.store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "race-propose",
                0,
            ),
            &proposal(),
            &auth,
            &policy(),
        )
        .expect("race proposal");
    let before = test.store.collaboration_operations().unwrap().len();
    let store = Arc::new(test.store.clone());
    let barrier = Arc::new(Barrier::new(2));
    let accept_store = Arc::clone(&store);
    let accept_barrier = Arc::clone(&barrier);
    let accept = std::thread::spawn(move || {
        let auth = authority();
        let decision = DelegationDecision {
            id: "race-accept".into(),
            delegation_id: "delegation-1".into(),
            expected_delegation_revision: 1,
            decision: DelegationDecisionKind::Accept,
            decided_by_target_host: auth.target_host.clone(),
            reason: "accept".into(),
            created_at: "2026-08-11T00:00:01Z".into(),
        };
        accept_barrier.wait();
        accept_store.decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "race-accept",
                1,
            ),
            "delegation-1",
            &decision,
            &auth,
            &placement(13),
        )
    });
    let cancel_store = Arc::clone(&store);
    let cancel_barrier = Arc::clone(&barrier);
    let cancel = std::thread::spawn(move || {
        let auth = authority();
        cancel_barrier.wait();
        cancel_store.cancel_delegation_before_accept(
            &context(
                auth.source_host.clone(),
                "delegation.cancel_before_accept",
                "race-cancel",
                1,
            ),
            "delegation-1",
            "withdraw before acceptance",
            &auth,
        )
    });
    let accepted = accept.join().expect("accept thread");
    let cancelled = cancel.join().expect("cancel thread");
    assert_ne!(accepted.is_ok(), cancelled.is_ok());
    assert_eq!(store.collaboration_operations().unwrap().len(), before + 1);
    let current = store
        .collaboration_delegations("company-1")
        .unwrap()
        .pop()
        .unwrap();
    assert!(matches!(
        current.state,
        DelegationState::ProvisioningTargetWork | DelegationState::Terminal
    ));
}
