use super::*;

#[test]
fn delegation_relationship_is_idempotent_placement_fenced_and_source_independent() {
    let test = TestStore::new("delegation");
    install_policy(&test.store);
    let auth = authority();
    let request = proposal();
    let propose_context = context(
        auth.source_host.clone(),
        "delegation.propose",
        "propose-1",
        0,
    );

    let first = test
        .store
        .propose_collaboration_delegation(&propose_context, &request, &auth, &policy())
        .expect("first proposal");
    let replay = test
        .store
        .propose_collaboration_delegation(&propose_context, &request, &auth, &policy())
        .expect("exact proposal replay");
    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(test.store.collaboration_operations().unwrap().len(), 3);

    let hostile_context = context(
        actor(ActorKind::AgentMember, "sibling-member"),
        "delegation.decide",
        "hostile-decide",
        1,
    );
    let decision = DelegationDecision {
        id: "decision-1".into(),
        delegation_id: request.delegation_id.clone(),
        expected_delegation_revision: 1,
        decision: DelegationDecisionKind::Accept,
        decided_by_target_host: actor(ActorKind::AgentMember, "sibling-member"),
        reason: "spoof".into(),
        created_at: "2026-08-11T00:00:01Z".into(),
    };
    let before = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .decide_collaboration_delegation(
            &hostile_context,
            &request.delegation_id,
            &decision,
            &auth,
            &placement(13),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let proper_decision = DelegationDecision {
        decided_by_target_host: auth.target_host.clone(),
        ..decision
    };
    let stale_before = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "stale-placement",
                1
            ),
            &request.delegation_id,
            &proper_decision,
            &auth,
            &placement(14),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), stale_before);
}
