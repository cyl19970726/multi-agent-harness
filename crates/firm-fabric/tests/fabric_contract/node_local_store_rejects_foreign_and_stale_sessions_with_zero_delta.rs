use super::*;

#[test]
fn node_local_store_rejects_foreign_and_stale_sessions_with_zero_delta() {
    let source_root = TestRoot::new("local-session-fence-source");
    let source =
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a").expect("source store");
    let request = operation(7, 3);
    let before = source.snapshot().expect("source snapshot");
    for hostile in [
        fabric_session("node-b", 7, 3),
        fabric_session("node-a", 6, 3),
        fabric_session("node-a", 7, 2),
    ] {
        let error = source
            .prepare_outbox(
                &hostile,
                &actor("fabric-client", &["fabric_submit"]),
                &request,
                100,
            )
            .expect_err("foreign or stale source session must fail closed");
        assert!(matches!(
            error.code,
            FabricErrorCode::SourceMismatch | FabricErrorCode::NodeStaleGeneration
        ));
        assert_eq!(error.effect, EffectCertainty::None);
        assert_eq!(source.snapshot().expect("source snapshot"), before);
    }
    let error = source
        .prepare_outbox(
            &fabric_session("node-a", 7, 3),
            &actor("sibling-agent", &["fabric_submit"]),
            &request,
            100,
        )
        .expect_err("wire actor cannot differ from authenticated source actor");
    assert_eq!(error.code, FabricErrorCode::UnauthorizedActor);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(source.snapshot().expect("source snapshot"), before);

    let target_root = TestRoot::new("local-session-fence-target");
    let target =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let attempt = RouteAttempt {
        id: "route-attempt:operation-1:1".into(),
        company_id: COMPANY.into(),
        operation_id: request.id.clone(),
        attempt_no: 1,
        target_node_id: "node-b".into(),
        target_gateway_generation: 9,
        control_plane_generation: 3,
        route_seq: 1,
        ordering_seq: 1,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let before = target.snapshot().expect("target snapshot");
    for hostile in [
        fabric_session("node-a", 9, 3),
        fabric_session("node-b", 8, 3),
        fabric_session("node-b", 9, 2),
    ] {
        let error = target
            .persist_inbox(&hostile, &request, &attempt, 100)
            .expect_err("foreign or stale target session must fail closed");
        assert!(matches!(
            error.code,
            FabricErrorCode::SourceMismatch | FabricErrorCode::NodeStaleGeneration
        ));
        assert_eq!(error.effect, EffectCertainty::None);
        assert_eq!(target.snapshot().expect("target snapshot"), before);
    }
    let mut hostile_body = request.clone();
    hostile_body.body["authority"] = json!("browser-selected-host");
    hostile_body.body_digest = json_digest(&hostile_body.body).expect("hostile body digest");
    let error = target
        .persist_inbox(
            &fabric_session("node-b", 9, 3),
            &hostile_body,
            &attempt,
            100,
        )
        .expect_err("target independently rejects an unregistered body shape");
    assert_eq!(error.code, FabricErrorCode::InvalidPayload);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(target.snapshot().expect("target snapshot"), before);
}
