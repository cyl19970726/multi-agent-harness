use super::*;

#[test]
fn offline_source_outbox_rebinds_only_after_empty_current_generation_reconcile() {
    let root = TestRoot::new("offline-source-successor-rebind");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a").expect("local store");
    let predecessor = fabric_session("node-a", 2, 3);
    store.bind_gateway_session(&predecessor).unwrap();
    let mut request = operation(2, 3);
    request.source_gateway_generation = Some(2);
    request.control_plane_generation = 3;
    request.body_digest = json_digest(&request.body).unwrap();
    store
        .prepare_outbox(&predecessor, &request.actor, &request, 100)
        .expect("durable offline queue");

    let successor = fabric_session("node-a", 3, 4);
    store.bind_gateway_session(&successor).unwrap();
    let rebound = store
        .rebind_unaccepted_outbox(&successor, &request.id, &[])
        .expect("empty current-generation reconciliation proves pre-acceptance rebind is safe");
    assert_eq!(rebound.source_gateway_generation, Some(3));
    assert_eq!(rebound.control_plane_generation, 4);
    assert_eq!(
        store.pending_outbox_operations().unwrap(),
        vec![rebound.clone()]
    );

    let accepted = RouteReceipt {
        id: "accepted-receipt".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        target_node_id: "node-b".into(),
        target_gateway_generation: 9,
        control_plane_generation: 4,
        route_seq: 1,
        kind: ReceiptKind::ControlPlaneAccepted,
        application_effect: None,
        result_schema: None,
        result: None,
        result_digest: None,
        error: None,
        created_at_unix_ms: 101,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let before = store.snapshot().unwrap();
    let error = store
        .rebind_unaccepted_outbox(&successor, &request.id, std::slice::from_ref(&accepted))
        .expect_err("accepted route truth cannot be rebound");
    assert_eq!(error.code, FabricErrorCode::IdempotencyConflict);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().unwrap(), before);

    store
        .mark_outbox_receipt(&successor, &accepted)
        .expect("Control Plane acceptance becomes sole route truth");
    let accepted_before_expiry = store.snapshot().unwrap();
    assert_eq!(
        store
            .expire_unaccepted_outbox(&successor, &request.id, 1_000)
            .expect("accepted operation is left to reconciliation"),
        None
    );
    assert_eq!(store.snapshot().unwrap(), accepted_before_expiry);
}
