use super::*;

#[test]
fn expired_ordering_tombstone_survives_replay_and_successor_before_valid_next() {
    let target_root = TestRoot::new("expired-ordering-successor");
    let target_local =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let first_session = fabric_session("node-b", 4, 3);
    target_local
        .bind_gateway_session(&first_session)
        .expect("bind first target session");

    let mut first = operation(2, 3);
    first.id = "operation-expired-order-1".into();
    first.idempotency_key = "expired-order-1".into();
    first.ordering_key = "runtime:ordered-session".into();
    first.expires_at_unix_ms = 100;
    let first_attempt = RouteAttempt {
        id: "attempt-expired-order-1".into(),
        company_id: COMPANY.into(),
        operation_id: first.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: first_session.gateway_generation,
        control_plane_generation: first_session.control_plane_generation,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Sent,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 90,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let (tombstone, replayed) = target_local
        .consume_expired_ordering_tombstone(&first_session, &first, &first_attempt, 101)
        .expect("consume expired sequence one");
    assert!(!replayed);
    assert_eq!(tombstone.ordering_seq, 1);
    let (_, replayed) = target_local
        .consume_expired_ordering_tombstone(&first_session, &first, &first_attempt, 102)
        .expect("exact tombstone replay");
    assert!(replayed);

    let mut successor = first_session.clone();
    successor.gateway_generation += 1;
    successor.node_daemon_generation += 1;
    target_local
        .bind_gateway_session(&successor)
        .expect("bind successor after tombstone");
    let mut second = operation(2, 3);
    second.id = "operation-valid-order-2".into();
    second.idempotency_key = "valid-order-2".into();
    second.ordering_key = first.ordering_key.clone();
    let second_attempt = RouteAttempt {
        id: "attempt-valid-order-2".into(),
        company_id: COMPANY.into(),
        operation_id: second.id.clone(),
        attempt_no: 2,
        target_node_id: "node-b".into(),
        target_gateway_generation: successor.gateway_generation,
        control_plane_generation: successor.control_plane_generation,
        route_seq: 2,
        ordering_seq: 2,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 103,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    target_local
        .persist_inbox(&successor, &second, &second_attempt, 104)
        .expect("valid sequence two persists after replay and successor reconnect");
    let state = target_local.snapshot().expect("final target state");
    assert_eq!(state.ordering_tombstones.len(), 1);
    assert_eq!(state.inboxes.len(), 1);
    assert_eq!(state.persisted_ordering_sequences[&second.ordering_key], 2);
    assert!(!state.inboxes.contains_key(&first.id));
    assert!(!state.results.contains_key(&first.id));
}
