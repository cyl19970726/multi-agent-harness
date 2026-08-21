use super::*;

#[test]
fn target_host_decision_routes_under_control_plane_and_validates_local_team() {
    let central = TestStore::new("decision-central");
    let target = TestStore::new("decision-target");
    install_policy(&central.store);
    seed_target_team(&target.store);
    let target_placement = TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 1,
        node_id: TARGET_NODE_UUID.into(),
        placement_generation: 1,
    };
    let mut auth = authority();
    auth.target_placement = target_placement.clone();
    let mut request = proposal();
    request.target_placement = target_placement.clone();
    central
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "decision-propose-1",
                0,
            ),
            &request,
            &auth,
            &policy(),
        )
        .expect("central proposal");
    let decision = DelegationDecision {
        id: "decision-route-1".into(),
        delegation_id: request.delegation_id.clone(),
        expected_delegation_revision: 1,
        decision: DelegationDecisionKind::Accept,
        decided_by_target_host: auth.target_host.clone(),
        reason: "capacity available".into(),
        created_at: "2026-08-13T00:00:00Z".into(),
    };
    let business = central
        .store
        .delegation_decide_operation(
            &context(
                auth.target_host.clone(),
                "delegation_decide",
                "decision-route-1",
                1,
            ),
            &request.delegation_id,
            &decision,
        )
        .expect("Control Plane builds exact target Host decision");
    let route = route_collaboration_business_operation(
        &business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "control-plane:3".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["company_control_plane".into()]),
                session_id: "control-plane:3".into(),
                issued_at_unix_ms: 100,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: auth.target_host,
            source: CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 100,
            expires_at_unix_ms: 5_000,
        },
    )
    .expect("decision uses accepted Control Plane route authority");
    let applied = apply_collaboration_target_operation(&target.store, &route, "unix-ms:200")
        .expect("current target Host decision validates");
    assert_eq!(
        applied.0,
        "agentfirm.collaboration.delegation_decision_validated.v1"
    );
    assert_eq!(applied.1["decision"]["id"], "decision-route-1");
    assert!(target.store.latest_works().unwrap().is_empty());

    let before = target.store.latest_works().unwrap();
    let mut hostile = route;
    hostile.body["business_actor_id"] = serde_json::json!("member-b");
    hostile.body_digest = json_digest(&hostile.body).unwrap();
    assert!(apply_collaboration_target_operation(&target.store, &hostile, "unix-ms:201").is_err());
    assert_eq!(target.store.latest_works().unwrap(), before);
}
