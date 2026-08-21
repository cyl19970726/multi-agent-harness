use super::*;

#[test]
fn node_local_gateway_session_queues_exact_recoverable_operation_and_fences_predecessor() {
    let root = TestRoot::new("durable-local-session-outbox");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-a").expect("local store");
    let session = fabric_session("node-a", 2, 3);
    store
        .bind_gateway_session(&session)
        .expect("bind authenticated gateway session");
    let mut request = operation(2, 3);
    request.source_gateway_generation = Some(2);
    request.control_plane_generation = 3;
    request.body_digest = json_digest(&request.body).unwrap();
    let actor = request.actor.clone();
    let (outbox, replayed) = store
        .prepare_outbox(&session, &actor, &request, 100)
        .expect("queue exact operation");
    assert!(!replayed);
    assert_eq!(outbox.operation.as_ref(), Some(&request));
    assert_eq!(
        store.pending_outbox_operations().expect("pending queue"),
        vec![request]
    );
    let before = store.snapshot().expect("before stale bind");
    assert_eq!(
        store
            .bind_gateway_session(&fabric_session("node-a", 1, 3))
            .expect_err("predecessor gateway cannot overwrite durable session")
            .code,
        FabricErrorCode::NodeStaleGeneration
    );
    assert_eq!(store.snapshot().expect("after stale bind"), before);
    assert_eq!(store.active_session().unwrap(), Some(session));
}
