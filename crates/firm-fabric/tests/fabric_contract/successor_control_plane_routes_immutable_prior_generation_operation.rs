use super::*;

#[test]
fn successor_control_plane_routes_immutable_prior_generation_operation() {
    let target_root = TestRoot::new("successor-control-plane-target-persist");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let successor = fabric_session("node-b", 7, 4);
    target_local
        .bind_gateway_session(&successor)
        .expect("bind successor Control Plane session");

    let mut accepted_by_predecessor = operation(2, 3);
    accepted_by_predecessor.id = "operation-accepted-by-predecessor-control".into();
    accepted_by_predecessor.idempotency_key = "predecessor-control".into();
    let successor_attempt = RouteAttempt {
        id: "attempt-successor-control".into(),
        company_id: COMPANY.into(),
        operation_id: accepted_by_predecessor.id.clone(),
        attempt_no: 2,
        target_node_id: "node-b".into(),
        target_gateway_generation: successor.gateway_generation,
        control_plane_generation: successor.control_plane_generation,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };

    let (persisted, replayed) = target_local
        .persist_inbox(
            &successor,
            &accepted_by_predecessor,
            &successor_attempt,
            101,
        )
        .expect("current successor attempt routes immutable predecessor operation");
    assert!(!replayed);
    assert_eq!(persisted.control_plane_generation, 4);
    assert_eq!(persisted.gateway_generation, 7);

    let mut stale_attempt = successor_attempt.clone();
    stale_attempt.id = "attempt-stale-control".into();
    stale_attempt.operation_id = "operation-stale-control".into();
    stale_attempt.control_plane_generation = 3;
    let mut stale_operation = accepted_by_predecessor.clone();
    stale_operation.id = stale_attempt.operation_id.clone();
    stale_operation.idempotency_key = "stale-control".into();
    let before = target_local.snapshot().expect("before stale attempt");
    let error = target_local
        .persist_inbox(&successor, &stale_operation, &stale_attempt, 102)
        .expect_err("stale route attempt remains fenced");
    assert_eq!(error.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(
        target_local.snapshot().expect("after stale attempt"),
        before
    );
}
