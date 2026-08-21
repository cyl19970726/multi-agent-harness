use super::*;

#[test]
fn target_work_create_applies_once_through_native_work_authority() {
    let target = TestStore::new("target-work-application");
    seed_target_team(&target.store);
    let target_placement = TargetPlacementRef {
        team_id: "team-b".into(),
        team_revision: 1,
        node_id: TARGET_NODE_UUID.into(),
        placement_generation: 1,
    };
    let payload = serde_json::json!({
        "delegation_id": "delegation-native-1",
        "requested_outcome": "Implement the target component",
        "acceptance_contract": "checks and evidence are required",
        "source_work_ref": work_ref("node-a", "team-a", "work-a", 9),
        "target_placement": target_placement,
    });
    let business = RoutedBusinessOperation {
        id: "route-target-native-1".into(),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: "company-1".into(),
        kind: RoutedBusinessKind::TargetWorkCreate,
        authenticated_actor: actor(ActorKind::AgentMember, "host-b"),
        source_node_id: "node-a".into(),
        target_placement: target_placement.clone(),
        expected_revision: 2,
        idempotency_key: "target-native-1".into(),
        payload_digest: canonical_json_fingerprint(&payload),
        payload,
        required_capability: "collaboration.target_work_create".into(),
        ordering_key: "delegation:delegation-native-1".into(),
        created_at: "2026-08-13T00:00:00Z".into(),
    };
    let route = route_collaboration_business_operation(
        &business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "node-a".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "daemon-a:1".into(),
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
    )
    .unwrap();
    let first = apply_collaboration_target_operation(&target.store, &route, "unix-ms:200")
        .expect("native target Work creation");
    let replay = apply_collaboration_target_operation(&target.store, &route, "unix-ms:201")
        .expect("native target Work exact replay");
    assert_eq!(first.1, replay.1);
    let works = target.store.latest_works().unwrap();
    assert_eq!(works.len(), 1);
    assert_eq!(works[0].id, "remote-work:delegation-native-1");
    assert_eq!(works[0].accountable_team_id.as_deref(), Some("team-b"));

    for (label, status) in [
        ("closed", AgentTeamStatus::Inactive),
        ("archived", AgentTeamStatus::Trashed),
    ] {
        let unavailable = TestStore::new(&format!("target-work-{label}"));
        seed_target_team(&unavailable.store);
        let team = unavailable
            .store
            .teams()
            .unwrap()
            .into_iter()
            .rev()
            .find(|team| team.id == "team-b")
            .unwrap();
        unavailable
            .store
            .transition_agent_team(
                &MutationContext {
                    execution_space_id: "space-node-b".into(),
                    authenticated_actor: actor(ActorKind::Human, "fixture-operator"),
                    authority_actor: None,
                    command_name: "agent_team.transition".into(),
                    idempotency_key: format!("team-b-{label}"),
                    expected_version: team.revision,
                    request_fingerprint: None,
                },
                &team.id,
                status,
                &format!("unix-ms:{label}"),
            )
            .unwrap();
        let before_works = unavailable.store.latest_works().unwrap();
        let before_events = unavailable.store.work_events().unwrap();
        let error = apply_collaboration_target_operation(&unavailable.store, &route, "unix-ms:201")
            .expect_err("terminal target Team must reject remote Work creation");
        assert_eq!(error.code, TransportFabricErrorCode::NodeStaleGeneration);
        assert_eq!(unavailable.store.latest_works().unwrap(), before_works);
        assert_eq!(unavailable.store.work_events().unwrap(), before_events);
    }

    let before = target.store.latest_works().unwrap();
    let mut stale = route;
    stale.body["target_team_revision"] = serde_json::json!(2);
    stale.body_digest = json_digest(&stale.body).unwrap();
    assert!(apply_collaboration_target_operation(&target.store, &stale, "unix-ms:202").is_err());
    assert_eq!(target.store.latest_works().unwrap(), before);

    let source = TestStore::new("remote-fact-source-cache");
    seed_team(
        &source.store,
        SOURCE_NODE_UUID,
        "Node A",
        "space-node-a",
        "project-a",
        "mission-a",
        "team-a",
        "Team A",
        "host-a",
        "run-a",
    );
    let mut target_work_ref: RemoteWorkRef =
        serde_json::from_value(first.1["target_work_ref"].clone()).unwrap();
    let relationship_target_work_ref = target_work_ref.clone();

    let cancellation_request = DelegationCancellationRequest {
        id: "cancel-native-1".into(),
        delegation_id: "delegation-native-1".into(),
        expected_delegation_revision: 3,
        requested_by: actor(ActorKind::AgentMember, "host-a"),
        reason: "source Work no longer needs this outcome".into(),
        state: CancellationRequestState::Pending,
        target_host_decision_ref: None,
        revision: 1,
        created_at: "unix-ms:202".into(),
        updated_at: "unix-ms:202".into(),
    };
    let cancellation_payload = serde_json::json!({
        "request": cancellation_request,
        "target_placement": target_placement,
        "target_work_ref": target_work_ref,
    });
    let cancellation_business = RoutedBusinessOperation {
        id: "route-cancel-native-1".into(),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: "company-1".into(),
        kind: RoutedBusinessKind::DelegationCancelRequest,
        authenticated_actor: actor(ActorKind::AgentMember, "host-a"),
        source_node_id: SOURCE_NODE_UUID.into(),
        target_placement: target_placement.clone(),
        expected_revision: 3,
        idempotency_key: "cancel-native-1".into(),
        payload_digest: canonical_json_fingerprint(&cancellation_payload),
        payload: cancellation_payload,
        required_capability: RoutedBusinessKind::DelegationCancelRequest.required_capability(),
        ordering_key: "delegation:delegation-native-1".into(),
        created_at: "unix-ms:202".into(),
    };
    let cancellation_route = route_collaboration_business_operation(
        &cancellation_business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "control-plane:3".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["company_control_plane".into()]),
                session_id: "control-plane:3".into(),
                issued_at_unix_ms: 202,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: actor(ActorKind::AgentMember, "host-a"),
            source: CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-b".into()),
            created_at_unix_ms: 202,
            expires_at_unix_ms: 5_000,
        },
    )
    .expect("central cancellation request uses Wave5 route");
    let observed =
        apply_collaboration_target_operation(&target.store, &cancellation_route, "unix-ms:203")
            .expect("target observes exact active Work cancellation request");
    assert_eq!(
        observed.0,
        "agentfirm.collaboration.cancellation_request_observed.v1"
    );

    let before_cancel = target.store.latest_works().unwrap();
    let premature_decision = DelegationCancellationDecision {
        id: "cancel-decision-native-1".into(),
        cancellation_request_id: "cancel-native-1".into(),
        expected_request_revision: 1,
        decision: CancellationDecisionKind::Accept,
        decided_by_target_host: actor(ActorKind::AgentMember, "host-b"),
        native_work_event_ref: target_work_ref.work_event_id.clone(),
        reason: "pretend the Work was cancelled".into(),
        created_at: "unix-ms:204".into(),
    };
    let decision_payload = |decision: &DelegationCancellationDecision| {
        serde_json::json!({
            "delegation_id": "delegation-native-1",
            "request_id": "cancel-native-1",
            "decision": decision,
            "target_placement": target_placement,
            "target_work_ref": target_work_ref,
        })
    };
    let route_decision = |decision: &DelegationCancellationDecision| {
        let payload = decision_payload(decision);
        let business = RoutedBusinessOperation {
            id: format!("route:{}", decision.id),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: "company-1".into(),
            kind: RoutedBusinessKind::DelegationCancelDecide,
            authenticated_actor: decision.decided_by_target_host.clone(),
            source_node_id: SOURCE_NODE_UUID.into(),
            target_placement: target_placement.clone(),
            expected_revision: 4,
            idempotency_key: decision.id.clone(),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: RoutedBusinessKind::DelegationCancelDecide.required_capability(),
            ordering_key: "delegation:delegation-native-1".into(),
            created_at: decision.created_at.clone(),
        };
        route_collaboration_business_operation(
            &business,
            &CollaborationFabricRouteContext {
                authenticated_actor: AuthenticatedActor {
                    company_id: "company-1".into(),
                    actor_id: "control-plane:3".into(),
                    actor_kind: FabricActorKind::Service,
                    role_bindings: BTreeSet::from(["company_control_plane".into()]),
                    session_id: "control-plane:3".into(),
                    issued_at_unix_ms: 204,
                    expires_at_unix_ms: 10_000,
                },
                resolved_business_actor: actor(ActorKind::AgentMember, "host-b"),
                source: CollaborationFabricSource::ControlPlane,
                control_plane_generation: 3,
                target_execution_space_id: Some("space-node-b".into()),
                created_at_unix_ms: 204,
                expires_at_unix_ms: 5_000,
            },
        )
        .unwrap()
    };
    assert!(apply_collaboration_target_operation(
        &target.store,
        &route_decision(&premature_decision),
        "unix-ms:204",
    )
    .is_err());
    assert_eq!(target.store.latest_works().unwrap(), before_cancel);

    let native_cancel_event = "native-work-cancelled:delegation-native-1";
    let current_work = target
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == target_work_ref.work_id)
        .unwrap();
    target
        .store
        .cancel_work(
            &current_work.id,
            current_work.version,
            "target Host accepted source cancellation request",
            WorkCommandContext {
                event_id: native_cancel_event.into(),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: "host-b".into(),
                    display_name: None,
                    authn_source: Some("remote_fabric_verified_source_node".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "native-cancel-1".into(),
                created_at: "unix-ms:205".into(),
                duplicate_ok: false,
            },
        )
        .expect("target Host cancels native Work through Work authority");
    let accepted_decision = DelegationCancellationDecision {
        native_work_event_ref: native_cancel_event.into(),
        reason: "native Work is quiesced and cancelled".into(),
        ..premature_decision
    };
    let validated = apply_collaboration_target_operation(
        &target.store,
        &route_decision(&accepted_decision),
        "unix-ms:206",
    )
    .expect("target validates exact native cancellation event");
    assert_eq!(
        validated.0,
        "agentfirm.collaboration.cancellation_decision_validated.v1"
    );
    let cancelled_work = target
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == target_work_ref.work_id)
        .unwrap();
    target_work_ref.work_revision = cancelled_work.version;
    target_work_ref.work_event_id = native_cancel_event.into();
    target_work_ref.digest =
        canonical_json_fingerprint(&serde_json::to_value(&cancelled_work).unwrap());

    let source_work = RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: "space-node-a".into(),
        node_id: SOURCE_NODE_UUID.into(),
        team_id: "team-a".into(),
        team_revision: 1,
        placement_generation: 1,
        work_id: "work-a".into(),
        work_revision: 9,
        work_event_id: "event-work-a-9".into(),
        digest: format!("sha256:{:064x}", 9),
    };
    let fact = serde_json::json!({
        "submitted_work_revision": target_work_ref.work_revision,
        "outcome": "implemented",
        "checks": ["check:unit"],
    });
    let fact_digest = canonical_json_fingerprint(&fact);
    let publication = RemoteFactPublication {
        id: "publication-routed-1".into(),
        company_id: "company-1".into(),
        delegation_id: "delegation-native-1".into(),
        origin_node_id: TARGET_NODE_UUID.into(),
        origin_team_id: "team-b".into(),
        fact_work_ref: relationship_target_work_ref,
        native_fact_work_ref: target_work_ref,
        delegation_source_work_ref: source_work,
        fact_kind: RemoteFactKind::Report,
        fact_id: "report-routed-1".into(),
        fact_revision: 1,
        fact_digest: fact_digest.clone(),
        summary: "target result".into(),
        classification: "team-visible".into(),
        snapshot: RemoteFactSnapshot {
            publication_id: "publication-routed-1".into(),
            fact_schema: "agentfirm.work-report.v1".into(),
            canonical_redacted_fact: fact,
            canonical_digest: fact_digest,
        },
        artifact_refs: Vec::new(),
        evidence_refs: vec!["check:unit".into()],
        operational_decision_ref: None,
        created_by: actor(ActorKind::AgentMember, "host-b"),
        created_at: "unix-ms:203".into(),
        retain_until: "unix-ms:999999".into(),
    };
    let source_placement = TargetPlacementRef {
        team_id: "team-a".into(),
        team_revision: 1,
        node_id: SOURCE_NODE_UUID.into(),
        placement_generation: 1,
    };
    let publication_business = target
        .store
        .remote_fact_publish_operation(
            &CollaborationMutationContext {
                company_id: "company-1".into(),
                authenticated_actor: publication.created_by.clone(),
                command_name: "remote_fact_publish".into(),
                idempotency_key: "publish-routed-1".into(),
                expected_revision: 3,
                occurred_at: "unix-ms:203".into(),
            },
            &publication,
            &source_placement,
            TARGET_NODE_UUID,
        )
        .expect("target WorkApplicationService builds routed publication");
    let publication_route = route_collaboration_business_operation(
        &publication_business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: TARGET_NODE_UUID.into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "daemon-b:1".into(),
                issued_at_unix_ms: 203,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: publication.created_by.clone(),
            source: CollaborationFabricSource::Node {
                source_execution_space_id: "space-node-b".into(),
                source_gateway_generation: 9,
                source_node_daemon_id: "daemon-b".into(),
                source_node_daemon_generation: 1,
            },
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-a".into()),
            created_at_unix_ms: 203,
            expires_at_unix_ms: 5_000,
        },
    )
    .expect("publication uses Wave5 route");
    let cached =
        apply_collaboration_target_operation(&source.store, &publication_route, "unix-ms:204")
            .expect("source Node persists read-only publication cache");
    assert_eq!(cached.0, "agentfirm.collaboration.remote_fact_cached.v1");
    let replay =
        apply_collaboration_target_operation(&source.store, &publication_route, "unix-ms:205")
            .expect("publication cache exact replay");
    assert_eq!(cached.1["publication_id"], replay.1["publication_id"]);
}
