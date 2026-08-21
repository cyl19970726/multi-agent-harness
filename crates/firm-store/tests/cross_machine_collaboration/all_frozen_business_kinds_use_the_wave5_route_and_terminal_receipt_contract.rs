use super::*;

#[test]
fn all_frozen_business_kinds_use_the_wave5_route_and_terminal_receipt_contract() {
    let kinds = [
        RoutedBusinessKind::DelegationPropose,
        RoutedBusinessKind::DelegationDecide,
        RoutedBusinessKind::TargetWorkCreate,
        RoutedBusinessKind::DelegationCancelRequest,
        RoutedBusinessKind::DelegationCancelDecide,
        RoutedBusinessKind::TeamMessageDeliver,
        RoutedBusinessKind::RemoteFactPublish,
        RoutedBusinessKind::ArtifactGrant,
    ];
    let context = CollaborationFabricRouteContext {
        authenticated_actor: AuthenticatedActor {
            company_id: "company-1".into(),
            actor_id: "node-a".into(),
            actor_kind: FabricActorKind::Service,
            role_bindings: BTreeSet::from(["fabric_submit".into()]),
            session_id: "session-host-a".into(),
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
    };

    for kind in kinds {
        let payload = serde_json::json!({"kind": kind.wire_name(), "delegation_id": "d-1"});
        let operation = RoutedBusinessOperation {
            id: format!("route-{}", kind.wire_name()),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: "company-1".into(),
            kind,
            authenticated_actor: actor(ActorKind::AgentMember, "host-a"),
            source_node_id: "node-a".into(),
            target_placement: placement(13),
            expected_revision: 7,
            idempotency_key: format!("idem-{}", kind.wire_name()),
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
            required_capability: kind.required_capability(),
            ordering_key: "delegation:d-1".into(),
            created_at: "2026-08-13T00:00:00Z".into(),
        };
        let mut route_context = context.clone();
        if matches!(
            kind,
            RoutedBusinessKind::DelegationDecide
                | RoutedBusinessKind::TargetWorkCreate
                | RoutedBusinessKind::DelegationCancelRequest
                | RoutedBusinessKind::DelegationCancelDecide
                | RoutedBusinessKind::ArtifactGrant
        ) {
            route_context.source = CollaborationFabricSource::ControlPlane;
            route_context.authenticated_actor.actor_id = "control-plane-1".into();
        }
        let routed = route_collaboration_business_operation(&operation, &route_context)
            .expect("frozen collaboration kind must use the Wave5 envelope");
        assert_eq!(routed.kind, COLLABORATION_BUSINESS_OPERATION_KIND);
        routed.closed_body().expect("closed transport registry");

        let result = serde_json::json!({"operation": operation.id, "applied": true});
        let receipt = RouteReceipt {
            id: format!("receipt:{}", operation.id),
            company_id: operation.company_id.clone(),
            operation_id: operation.id.clone(),
            target_node_id: operation.target_placement.node_id.clone(),
            target_gateway_generation: 9,
            control_plane_generation: 3,
            route_seq: 11,
            kind: ReceiptKind::OperationApplied,
            application_effect: Some(EffectCertainty::Applied),
            result_schema: Some("agentfirm.collaboration.result.v1".into()),
            result_digest: Some(json_digest(&result).unwrap()),
            result: Some(result.clone()),
            error: None,
            created_at_unix_ms: 150,
            schema_version: "agentfirm.remote_fabric.v1".into(),
        };
        let business =
            collaboration_receipt_from_fabric(&operation, &receipt, "2026-08-13T00:00:01Z")
                .expect("only terminal applied is business success");
        assert_eq!(business.result, result);

        let mut accepted_only = receipt.clone();
        accepted_only.kind = ReceiptKind::ControlPlaneAccepted;
        accepted_only.application_effect = None;
        assert!(collaboration_receipt_from_fabric(
            &operation,
            &accepted_only,
            "2026-08-13T00:00:01Z"
        )
        .is_err());

        let mut unknown = receipt;
        unknown.kind = ReceiptKind::RecoveryRequired;
        unknown.application_effect = Some(EffectCertainty::Unknown);
        let error = collaboration_receipt_from_fabric(&operation, &unknown, "2026-08-13T00:00:01Z")
            .unwrap_err();
        assert_eq!(error.code, FabricErrorCode::RecoveryRequired);
        assert_eq!(error.effect_certainty, FabricEffectCertainty::Unknown);
    }
}
