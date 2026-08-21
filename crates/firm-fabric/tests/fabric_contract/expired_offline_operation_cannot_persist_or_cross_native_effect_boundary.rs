use super::*;

#[test]
fn expired_offline_operation_cannot_persist_or_cross_native_effect_boundary() {
    let target_root = TestRoot::new("expired-target");
    let target =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let session = fabric_session("node-b", 1, 1);
    target.bind_gateway_session(&session).expect("bind session");
    let request = operation(1, 1);
    let attempt = RouteAttempt {
        id: "route-attempt:operation-1:1".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: 1,
        control_plane_generation: 1,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Sent,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let before = target.snapshot().expect("snapshot");
    let expired = target
        .persist_inbox(&session, &request, &attempt, request.expires_at_unix_ms)
        .expect_err("expired offline work cannot be persisted on reconnect");
    assert_eq!(expired.code, FabricErrorCode::OperationExpired);
    assert_eq!(expired.effect, EffectCertainty::None);
    assert_eq!(target.snapshot().expect("snapshot"), before);

    target
        .persist_inbox(&session, &request, &attempt, request.expires_at_unix_ms - 1)
        .expect("unexpired operation can persist");
    let persisted = target.snapshot().expect("persisted snapshot");
    let expired_claim = target
        .claim_inbox(&session, &request, request.expires_at_unix_ms)
        .expect_err("expiry is rechecked before native effect");
    assert_eq!(expired_claim.code, FabricErrorCode::OperationExpired);
    assert_eq!(expired_claim.effect, EffectCertainty::None);
    assert_eq!(target.snapshot().expect("snapshot"), persisted);
}
