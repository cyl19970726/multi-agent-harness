use super::*;

fn host_context(
    store: &HarnessStore,
    run_id: &str,
    event_id: &str,
    idempotency_key: &str,
) -> firm_core::WorkCommandContext {
    let actor = store.exact_team_run_host_actor(run_id).unwrap();
    firm_core::WorkCommandContext {
        event_id: event_id.into(),
        performed_by_actor: actor.clone(),
        authority_actor: Some(actor),
        causation_ref: None,
        idempotency_key: idempotency_key.into(),
        created_at: "t-cancel".into(),
        duplicate_ok: false,
    }
}

fn claimed_work_fixture(
    store: &HarnessStore,
    suffix: &str,
) -> (
    firm_core::Work,
    WorkExecutionBinding,
    CanonicalWorkDelivery,
    AgentSession,
) {
    let member_id = format!("builder-{suffix}");
    let session_id = format!("session-{suffix}");
    let membership_id = format!("membership-{suffix}");
    let work_id = format!("work-{suffix}");
    let binding_id = format!("binding-{suffix}");
    let delivery_id = format!("work-delivery:{work_id}:1");

    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                &format!("identity-{suffix}"),
                0,
            ),
            identity(&member_id),
        )
        .unwrap();
    let session = session(&session_id, &member_id);
    store
        .create_agent_session(
            &service_context("session.create", &session_id, 0),
            session.clone(),
        )
        .unwrap();
    append_runtime_team(store, "team-a", "team-run-a");
    let membership = join_runtime_membership(
        store,
        &membership_id,
        "team-a",
        &member_id,
        TeamMembershipRole::Member,
    );
    let work = insert_runtime_work(store, &work_id, "team-a", "team-run-a");
    let binding = WorkExecutionBinding {
        id: binding_id.clone(),
        work_id: work.id.clone(),
        work_revision: work.version,
        team_id: membership.team_id.clone(),
        team_membership_id: membership.id,
        agent_member_id: member_id,
        agent_session_id: session.id.clone(),
        agent_session_generation: session.runtime_generation,
        delivery_id,
        binding_generation: 1,
        status: WorkExecutionBindingStatus::Active,
        version: 1,
        created_by: actor("fixture-host"),
        bound_at: "t-bound".into(),
        ended_at: None,
    };
    store
        .bind_work_execution_fixture(
            &context("fixture-host", "work.bind", &binding_id, 0),
            binding.clone(),
        )
        .unwrap();
    let delivery = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.work_id == work.id)
        .unwrap();
    let claim_id = format!("claim-{suffix}");
    store
        .claim_work_for_provider(
            &service_context("work.claim", &claim_id, 0),
            &delivery.id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &claim_id,
            firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
            "t-claim",
        )
        .unwrap();
    let claimed = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == delivery.id)
        .unwrap();
    (work, binding, claimed, session)
}

fn received_work_fixture(
    store: &HarnessStore,
    suffix: &str,
) -> (
    firm_core::Work,
    WorkExecutionBinding,
    CanonicalWorkDelivery,
    AgentSession,
) {
    let (work, binding, claimed, session) = claimed_work_fixture(store, suffix);
    let claim_id = claimed.claim_id.clone().unwrap();
    let receipt = format!("receipt-{suffix}");
    let received = store
        .record_work_provider_receipt(
            &service_context("work.receipt", &receipt, 0),
            &claimed.id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &claim_id,
            &receipt,
            "t-received",
        )
        .unwrap()
        .projection;
    (work, binding, received, session)
}

#[test]
fn host_cancel_closes_work_but_preserves_received_delivery_and_binding() {
    let (store, root) = fabric_store();
    let (work, binding, received, session) = received_work_fixture(&store, "received");
    let before_commands = store.runtime_commands("space-test").unwrap();

    let context = host_context(
        &store,
        &work.team_run_id,
        "cancel-received",
        "cancel-received",
    );
    let cancelled = store
        .cancel_work(
            &work.id,
            work.version,
            "superseded by Host",
            context.clone(),
        )
        .expect("a provider receipt does not remove Host cancellation authority");
    assert_eq!(cancelled.phase, firm_core::WorkPhase::Closed);
    assert_eq!(
        cancelled.resolution,
        Some(firm_core::WorkResolution::Cancelled)
    );
    assert_eq!(cancelled.version, work.version + 1);
    assert_eq!(
        store
            .fabric_work_deliveries("space-test")
            .unwrap()
            .into_iter()
            .find(|row| row.id == received.id)
            .unwrap(),
        received,
        "cancellation must not rewrite provider receipt evidence"
    );
    assert_eq!(
        store
            .fabric_work_execution_bindings("space-test")
            .unwrap()
            .into_iter()
            .find(|row| row.id == binding.id)
            .unwrap(),
        binding,
        "cancellation must not synthesize binding release"
    );
    assert_eq!(
        store.runtime_commands("space-test").unwrap(),
        before_commands
    );
    assert!(store
        .work_events()
        .unwrap()
        .iter()
        .filter(|event| event.work_id == work.id)
        .all(|event| event.kind != firm_core::WorkEventKind::Started));

    let replay = store
        .cancel_work(&work.id, work.version, "superseded by Host", context)
        .expect("exact retry is idempotent");
    assert_eq!(replay, cancelled);
    let changed_retry = store
        .cancel_work(
            &work.id,
            work.version,
            "different reason",
            host_context(
                &store,
                &work.team_run_id,
                "cancel-changed",
                "cancel-received",
            ),
        )
        .expect_err("same idempotency key cannot identify a changed request");
    assert!(changed_retry.to_string().contains("IDEMPOTENCY"));

    let stale_claim = store
        .claim_work_for_provider(
            &service_context("work.claim", "stale-claim", 0),
            &received.id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            "stale-claim",
            firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
            "t-stale",
        )
        .expect_err("terminal Work revision cannot be dispatched again");
    assert!(stale_claim.to_string().contains("WORK_NOT_READY"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn host_cancel_keeps_an_unsettled_provider_claim_fail_closed() {
    let (store, root) = fabric_store();
    let (work, _binding, claimed, _session) = claimed_work_fixture(&store, "unsettled");
    assert_eq!(claimed.status, WorkDeliveryStatus::Claimed);
    let before_rejection = (
        store.latest_works().unwrap(),
        store.fabric_work_deliveries("space-test").unwrap(),
        store.fabric_work_execution_bindings("space-test").unwrap(),
        store.canonical_operations().unwrap(),
    );
    let error = store
        .cancel_work(
            &work.id,
            work.version,
            "must reconcile first",
            host_context(
                &store,
                &work.team_run_id,
                "cancel-unsettled",
                "cancel-unsettled",
            ),
        )
        .expect_err("an unsettled claim must remain fail closed");
    assert!(error.to_string().contains("RECONCILIATION_REQUIRED"));
    assert_eq!(
        (
            store.latest_works().unwrap(),
            store.fabric_work_deliveries("space-test").unwrap(),
            store.fabric_work_execution_bindings("space-test").unwrap(),
            store.canonical_operations().unwrap(),
        ),
        before_rejection
    );
    std::fs::remove_dir_all(root).unwrap();
}
