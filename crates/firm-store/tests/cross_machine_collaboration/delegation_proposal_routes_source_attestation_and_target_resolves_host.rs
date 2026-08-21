use super::*;

#[test]
fn delegation_proposal_routes_source_attestation_and_target_resolves_host() {
    let source = TestStore::new("proposal-source");
    let target = TestStore::new("proposal-target");
    install_policy(&source.store);
    seed_target_team(&target.store);
    let target_placement = TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 1,
        node_id: TARGET_NODE_UUID.into(),
        placement_generation: 1,
    };
    let request = ProposeDelegationRequest {
        delegation_id: "delegation-routed-1".into(),
        source_work_attestation_id: "source-work-attestation-1".into(),
        target_placement: target_placement.clone(),
        requested_outcome: "Implement target component".into(),
        outcome_class: "implementation".into(),
        acceptance_contract: "checks and evidence".into(),
        operation_id: "route-proposal-1".into(),
    };
    let business = source
        .store
        .delegation_propose_operation(
            &context(
                actor(ActorKind::AgentMember, "host-a"),
                "delegation_propose",
                "proposal-route-1",
                0,
            ),
            &request,
            "policy-a-b",
        )
        .expect("source Node builds attested proposal");
    let route = route_collaboration_business_operation(
        &business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "node-a".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "daemon-a:8".into(),
                issued_at_unix_ms: 1,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: actor(ActorKind::AgentMember, "host-a"),
            source: CollaborationFabricSource::Node {
                source_execution_space_id: "space-node-a".into(),
                source_gateway_generation: 8,
                source_node_daemon_id: "daemon-a".into(),
                source_node_daemon_generation: 4,
            },
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 100,
            expires_at_unix_ms: 5_000,
        },
    )
    .expect("Wave5 route");
    let applied = apply_collaboration_target_operation(&target.store, &route, "unix-ms:200")
        .expect("target validates current Team placement and Host");
    assert_eq!(
        applied.0,
        "agentfirm.collaboration.delegation_proposal_validated.v1"
    );
    assert_eq!(applied.1["target_host_ref"]["id"], "host-b");
    assert!(target.store.latest_works().unwrap().is_empty());

    let before = target.store.collaboration_operations().unwrap();
    let mut stale = route;
    stale.body["target_team_revision"] = serde_json::json!(2);
    stale.body_digest = json_digest(&stale.body).unwrap();
    assert!(apply_collaboration_target_operation(&target.store, &stale, "unix-ms:201").is_err());
    assert_eq!(target.store.collaboration_operations().unwrap(), before);
}
