use super::*;

#[test]
fn active_cancellation_is_only_a_source_request_and_target_host_decision() {
    let test = TestStore::new("cancel");
    let _target = active_delegation(&test.store);
    let auth = authority();
    let request = DelegationCancellationRequest {
        id: "cancel-request-1".into(),
        delegation_id: "delegation-1".into(),
        expected_delegation_revision: 3,
        requested_by: auth.source_host.clone(),
        reason: "source priorities changed".into(),
        state: CancellationRequestState::Pending,
        target_host_decision_ref: None,
        revision: 1,
        created_at: "2026-08-11T00:00:03Z".into(),
        updated_at: "2026-08-11T00:00:03Z".into(),
    };
    let before = test.store.collaboration_operations().unwrap();
    let mut hostile = request.clone();
    hostile.requested_by = actor(ActorKind::AgentMember, "member-a");
    assert!(test
        .store
        .request_delegation_cancellation(
            &context(
                hostile.requested_by.clone(),
                "delegation.cancel.request",
                "cancel-hostile",
                3,
            ),
            &hostile,
            &auth,
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let request_context = context(
        auth.source_host.clone(),
        "delegation.cancel.request",
        "cancel-1",
        3,
    );
    let requested = test
        .store
        .request_delegation_cancellation(&request_context, &request, &auth)
        .expect("source Host cancellation request");
    assert_eq!(
        requested.projection.state,
        DelegationState::CancellationRequested
    );
    assert_eq!(requested.projection.terminal_outcome, None);
    let replay = test
        .store
        .request_delegation_cancellation(&request_context, &request, &auth)
        .expect("exact cancellation request replay");
    assert!(replay.replayed);
    assert_eq!(replay.operation, requested.operation);
    let route = test
        .store
        .delegation_cancel_request_operation(
            &context(
                auth.source_host.clone(),
                "delegation.cancel.route",
                "cancel-route-1",
                4,
            ),
            &request,
        )
        .expect("central pending cancellation builds target route");
    assert_eq!(route.kind, RoutedBusinessKind::DelegationCancelRequest);
    assert_eq!(route.expected_revision, 3);

    let decision = DelegationCancellationDecision {
        id: "cancel-decision-1".into(),
        cancellation_request_id: request.id.clone(),
        expected_request_revision: 1,
        decision: CancellationDecisionKind::Accept,
        decided_by_target_host: auth.target_host.clone(),
        native_work_event_ref: "work-event-b-cancelled".into(),
        reason: "target Work quiesced".into(),
        created_at: "2026-08-11T00:00:04Z".into(),
    };
    let cancelled = test
        .store
        .decide_delegation_cancellation(
            &context(
                auth.target_host.clone(),
                "delegation.cancel.decide",
                "cancel-decision-1",
                4,
            ),
            "delegation-1",
            &request.id,
            &decision,
            &auth,
            &placement(13),
        )
        .expect("target Host cancellation decision");
    assert_eq!(cancelled.projection.state, DelegationState::Terminal);
    assert_eq!(
        cancelled.projection.terminal_outcome,
        Some(DelegationTerminalOutcome::Cancelled)
    );
    assert_eq!(cancelled.projection.source_work_ref.work_revision, 9);
    let request_projection = test
        .store
        .collaboration_cancellation_requests("company-1", "delegation-1")
        .unwrap()
        .pop()
        .expect("cancellation request projection");
    assert_eq!(request_projection.state, CancellationRequestState::Accepted);
    assert_eq!(request_projection.revision, 2);
    assert_eq!(
        request_projection.target_host_decision_ref.as_deref(),
        Some("cancel-decision-1")
    );
}
