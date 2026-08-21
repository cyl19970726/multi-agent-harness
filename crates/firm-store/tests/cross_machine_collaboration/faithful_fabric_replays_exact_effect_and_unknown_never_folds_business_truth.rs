use super::*;

#[test]
fn faithful_fabric_replays_exact_effect_and_unknown_never_folds_business_truth() {
    let test = TestStore::new("faithful-fabric");
    install_policy(&test.store);
    let auth = authority();
    test.store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "fabric-propose",
                0,
            ),
            &proposal(),
            &auth,
            &policy(),
        )
        .expect("fabric proposal");
    let decision = DelegationDecision {
        id: "fabric-accept".into(),
        delegation_id: "delegation-1".into(),
        expected_delegation_revision: 1,
        decision: DelegationDecisionKind::Accept,
        decided_by_target_host: auth.target_host.clone(),
        reason: "accepted".into(),
        created_at: "2026-08-11T00:00:01Z".into(),
    };
    test.store
        .decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "fabric-accept",
                1,
            ),
            "delegation-1",
            &decision,
            &auth,
            &placement(13),
        )
        .expect("fabric accepted");

    let route = test
        .store
        .target_work_create_operation("company-1", "delegation-1", "2026-08-11T00:00:02Z")
        .unwrap();
    assert_eq!(route.authenticated_actor, auth.target_host);
    let route_client = TerminalRouteClient::default();
    let remote_port = RemoteFabricCollaborationPort::new(
        &route_client,
        CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "node-a".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "session-host-b".into(),
                issued_at_unix_ms: 1,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: actor(ActorKind::AgentMember, "host-b"),
            source: CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 100,
            expires_at_unix_ms: 5_000,
        },
        "2026-08-11T00:00:03Z",
    );
    let remote_first = remote_port
        .dispatch(&route)
        .expect("real Wave5 route adapter");
    let remote_replay = remote_port.dispatch(&route).expect("exact route replay");
    assert_eq!(remote_first, remote_replay);
    assert_eq!(route_client.effects.lock().unwrap().len(), 1);
    let fabric = FaithfulFabric::default();
    let first_receipt = fabric.dispatch(&route).unwrap();
    let replay_receipt = fabric.dispatch(&route).unwrap();
    assert_eq!(first_receipt, replay_receipt);
    assert_eq!(fabric.effect_count(), 1);

    let control_plane = actor(ActorKind::Service, "fabric-control-plane");
    let before_hostile_fold = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .apply_target_work_created(
            &context(
                actor(ActorKind::Service, "forged-service"),
                "target_work.applied",
                "forged-fold-1",
                2,
            ),
            "delegation-1",
            &work_ref("node-b", "team-b", "work-b", 1),
            &placement(13),
            &route.id,
            &control_plane,
        )
        .is_err());
    assert_eq!(
        test.store.collaboration_operations().unwrap(),
        before_hostile_fold
    );
    let service = CollaborationApplicationService::new(&test.store, &fabric, &control_plane);
    let applied = service
        .provision_target_work(
            &context(
                actor(ActorKind::Service, "fabric-control-plane"),
                "target_work.applied",
                "fabric-fold-1",
                2,
            ),
            "delegation-1",
            &placement(13),
        )
        .expect("fold faithful applied receipt");
    assert_eq!(applied.projection.state, DelegationState::Active);
    assert_eq!(fabric.effect_count(), 1);

    let second = TestStore::new("unknown-fabric");
    install_policy(&second.store);
    second
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "unknown-propose",
                0,
            ),
            &proposal(),
            &auth,
            &policy(),
        )
        .unwrap();
    second
        .store
        .decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "unknown-accept",
                1,
            ),
            "delegation-1",
            &decision,
            &auth,
            &placement(13),
        )
        .unwrap();
    let before = second.store.collaboration_operations().unwrap();
    let unknown_fabric = UnknownFabric;
    let unknown =
        CollaborationApplicationService::new(&second.store, &unknown_fabric, &control_plane);
    assert!(unknown
        .provision_target_work(
            &context(
                actor(ActorKind::Service, "fabric-control-plane"),
                "target_work.applied",
                "unknown-fold-1",
                2,
            ),
            "delegation-1",
            &placement(13),
        )
        .is_err());
    assert_eq!(second.store.collaboration_operations().unwrap(), before);
    assert_eq!(
        second.store.collaboration_delegations("company-1").unwrap()[0].state,
        DelegationState::ProvisioningTargetWork
    );
}
