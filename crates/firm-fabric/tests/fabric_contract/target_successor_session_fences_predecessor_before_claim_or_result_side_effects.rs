use super::*;

#[test]
fn target_successor_session_fences_predecessor_before_claim_or_result_side_effects() {
    let root = TestRoot::new("target-successor-local-fence");
    let store = NodeLocalFabricStore::open(root.path(), COMPANY, "node-b").expect("local store");
    let predecessor = fabric_session("node-b", 4, 3);
    store
        .bind_gateway_session(&predecessor)
        .expect("bind predecessor");
    let request = operation(2, 3);
    let attempt = RouteAttempt {
        id: "route-attempt:successor-fence:1".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: predecessor.gateway_generation,
        control_plane_generation: predecessor.control_plane_generation,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    store
        .persist_inbox(&predecessor, &request, &attempt, 100)
        .expect("predecessor persists before takeover");

    let successor = fabric_session("node-b", 5, 3);
    store
        .bind_gateway_session(&successor)
        .expect("bind successor");
    let before = store.snapshot().expect("snapshot after successor bind");

    for rejected in [
        store.claim_inbox(&predecessor, &request, 101).map(|_| ()),
        store
            .record_application_result(
                &predecessor,
                &request.id,
                "agentfirm.remote_fabric.probe_result.v1",
                json!({"must_not_exist": true}),
                EffectCertainty::Applied,
                101,
            )
            .map(|_| ()),
    ] {
        let error = rejected.expect_err("predecessor must be fenced under the Store lock");
        assert_eq!(error.code, FabricErrorCode::NodeStaleGeneration);
        assert_eq!(error.effect, EffectCertainty::None);
        assert_eq!(store.snapshot().expect("zero-delta snapshot"), before);
    }
    assert!(store.snapshot().unwrap().results.is_empty());
    assert_eq!(
        store.snapshot().unwrap().inboxes[&request.id].state,
        LocalInboxState::Persisted
    );
}
