use super::*;

#[test]
fn node_local_journal_recovers_lost_ack_without_duplicate_native_effect() {
    let source_root = TestRoot::new("local-source-recovery");
    let request = operation(1, 1);
    let source_session = fabric_session("node-a", 1, 1);
    let source =
        NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a").expect("source store");
    source
        .bind_gateway_session(&source_session)
        .expect("bind source session");
    let before = source.snapshot().expect("source snapshot");
    source.fail_next_commit_for_test();
    let rejected = source
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            100,
        )
        .expect_err("failure before append must be effect-none");
    assert_eq!(rejected.code, FabricErrorCode::StoreUnavailable);
    assert_eq!(rejected.effect, EffectCertainty::None);
    assert_eq!(source.snapshot().expect("source snapshot"), before);

    source.fail_after_append_for_test();
    let unknown = source
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            101,
        )
        .expect_err("lost local append acknowledgement is unknown");
    assert_eq!(unknown.code, FabricErrorCode::RecoveryRequired);
    assert_eq!(unknown.effect, EffectCertainty::Unknown);
    assert_eq!(
        source
            .snapshot()
            .expect_err("source latches unavailable")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    drop(source);
    let recovered_source = NodeLocalFabricStore::open(source_root.path(), COMPANY, "node-a")
        .expect("reopen source store");
    let (_, replayed) = recovered_source
        .prepare_outbox(
            &source_session,
            &actor("fabric-client", &["fabric_submit"]),
            &request,
            102,
        )
        .expect("exact outbox replay after reopen");
    assert!(replayed);
    assert_eq!(
        recovered_source
            .snapshot()
            .expect("source snapshot")
            .outboxes
            .len(),
        1
    );
    for (company_id, node_id) in [(COMPANY, "node-b"), ("company-foreign", "node-a")] {
        let error = match NodeLocalFabricStore::open(source_root.path(), company_id, node_id) {
            Ok(_) => panic!("durably bound Node-local root must reject another authority"),
            Err(error) => error,
        };
        assert_eq!(error.code, FabricErrorCode::WrongCompany);
        assert_eq!(error.effect, EffectCertainty::None);
    }

    let target_root = TestRoot::new("local-target-recovery");
    let target =
        NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b").expect("target store");
    let target_session = fabric_session("node-b", 1, 1);
    target
        .bind_gateway_session(&target_session)
        .expect("bind target session");
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
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: 100,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    target
        .persist_inbox(&target_session, &request, &attempt, 100)
        .expect("persist target inbox");
    target
        .claim_inbox(&target_session, &request, 101)
        .expect("claim before native effect");
    assert_eq!(
        target.unresolved_operation_ids().expect("unresolved ids"),
        BTreeSet::from([request.id.clone()])
    );
    let duplicate_claim = target
        .claim_inbox(&target_session, &request, 102)
        .expect_err("duplicate claim cannot blindly repeat a native effect");
    assert_eq!(duplicate_claim.code, FabricErrorCode::RecoveryRequired);
    target.fail_after_append_for_test();
    let unknown = target
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            103,
        )
        .expect_err("native result append acknowledgement is unknown");
    assert_eq!(unknown.code, FabricErrorCode::RecoveryRequired);
    drop(target);
    let recovered_target = NodeLocalFabricStore::open(target_root.path(), COMPANY, "node-b")
        .expect("reopen target store");
    let (inbox, result, replayed) = recovered_target
        .record_application_result(
            &target_session,
            &request.id,
            "agentfirm.remote_fabric.probe_result.v1",
            json!({"reachable": true}),
            EffectCertainty::Applied,
            104,
        )
        .expect("exact native result replay after reopen");
    assert!(replayed);
    assert_eq!(inbox.state, LocalInboxState::Applied);
    assert_eq!(result.effect, EffectCertainty::Applied);
    assert!(recovered_target
        .unresolved_operation_ids()
        .expect("unresolved ids")
        .is_empty());
    assert_eq!(
        recovered_target
            .snapshot()
            .expect("target snapshot")
            .results
            .len(),
        1
    );
}
