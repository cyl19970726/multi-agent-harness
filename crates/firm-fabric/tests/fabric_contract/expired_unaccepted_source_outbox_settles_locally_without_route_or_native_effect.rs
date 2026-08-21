use super::*;

#[test]
fn expired_unaccepted_source_outbox_settles_locally_without_route_or_native_effect() {
    let root = TestRoot::new("expired-unaccepted-source-outbox");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a").expect("local store");
    let predecessor = fabric_session("node-a", 2, 3);
    store.bind_gateway_session(&predecessor).unwrap();
    let mut request = operation(2, 3);
    request.source_gateway_generation = Some(2);
    request.control_plane_generation = 3;
    request.expires_at_unix_ms = 150;
    request.body_digest = json_digest(&request.body).unwrap();
    store
        .prepare_outbox(&predecessor, &request.actor, &request, 100)
        .expect("durable offline queue");

    let successor = fabric_session("node-a", 3, 4);
    store.bind_gateway_session(&successor).unwrap();
    let before = store.snapshot().unwrap();
    let early = store
        .expire_unaccepted_outbox(&successor, &request.id, 149)
        .expect_err("live operation cannot be locally expired");
    assert_eq!(early.code, FabricErrorCode::ExpectedRevisionConflict);
    assert_eq!(store.snapshot().unwrap(), before);

    let terminal = store
        .expire_unaccepted_outbox(&successor, &request.id, 150)
        .expect("expired unaccepted operation settles locally")
        .expect("local terminal result");
    assert_eq!(terminal.local_state, LocalOutboxState::Terminal);
    assert_eq!(
        terminal.terminal_receipt_ref.as_deref(),
        Some("local:not_applied:operation_expired:operation-1")
    );
    assert!(store.pending_outbox_operations().unwrap().is_empty());
    let settled = store.snapshot().unwrap();
    assert!(settled.inboxes.is_empty());
    assert!(settled.results.is_empty());

    assert_eq!(
        store
            .expire_unaccepted_outbox(&successor, &request.id, 151)
            .expect("terminal replay is stable")
            .expect("local terminal replay"),
        terminal
    );
    let stale = store
        .expire_unaccepted_outbox(&predecessor, &request.id, 151)
        .expect_err("predecessor cannot mutate successor state");
    assert_eq!(stale.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(store.snapshot().unwrap(), settled);
}
