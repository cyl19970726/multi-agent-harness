use super::*;
use harness_core::CurrentWorkDraft;

#[test]
fn supervisor_claims_and_records_provider_receipt_for_canonical_work_delivery() {
    let (store, root) = temp_store("canonical-supervisor-work-delivery");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let work = store
        .insert_work(
            {
                let mut draft = CurrentWorkDraft::new(
                    "canonical-supervisor-work".into(),
                    created.team_run.id.clone(),
                    created.team_run.agent_team_id.clone(),
                    "Deliver canonical Work".into(),
                    "Exercise NodeDaemon ProviderWorkDispatch wiring".into(),
                    "Provider receipt is canonical".into(),
                    WorkClaimMode::HostAssign,
                    WorkPriority::Normal,
                    compatibility_team_actor("host", "test"),
                    "unix-ms:3".into(),
                );
                draft.owner_member_id = Some(member.agent_member_id.clone());
                draft.active_member_run_id = Some(member.id.clone());
                draft.eligible_member_ids = vec![member.agent_member_id.clone()];
                draft.into_work()
            },
            WorkCommandContext {
                event_id: "canonical-supervisor-work-created".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-work-create".into(),
                created_at: "unix-ms:3".into(),
                duplicate_ok: false,
            },
        )
        .expect("create assigned Work");
    store
        .create_trust_work_deliveries(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: "test-host".into(),
                },
                authority_actor: None,
                command_name: "test.work_delivery.create".into(),
                idempotency_key: "canonical-supervisor-work-delivery".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            "canonical-supervisor-work-event",
            &work.id,
            work.version,
            std::slice::from_ref(&member.id),
            "unix-ms:4",
        )
        .expect("create canonical ProviderWorkDispatch");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "canonical-work-supervisor",
            std::process::id(),
            "test://canonical-work-supervisor",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("claim canonical Work")
        .expect("one canonical Work claim");
    ledger
        .complete_work_delivery(&claimed, "provider-work-receipt")
        .expect("record canonical provider receipt");
    let delivery = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical WorkDelivery fabric")
        .into_iter()
        .find(|delivery| delivery.work_id == work.id)
        .expect("canonical delivery");
    assert_eq!(
        delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        delivery.provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    assert!(
        store
            .trust_work_deliveries("unit-test-space")
            .expect("historical Wave4A WorkDelivery rows")
            .into_iter()
            .filter(|delivery| delivery.work_id == work.id)
            .all(|delivery| {
                delivery.status == harness_core::agentfirm_api::WorkDeliveryStatus::Queued
                    && delivery.provider_receipt_id.is_none()
            }),
        "provider runtime must not advance the historical Wave4A ledger"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn supervisor_skips_not_ready_delivery_and_claims_ready_predecessor() {
    let (store, root) = temp_store("canonical-supervisor-ready-work-delivery");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let make_work = |id: &str, created_at: &str, prerequisites: Vec<String>| {
        let mut draft = CurrentWorkDraft::new(
            id.into(),
            created.team_run.id.clone(),
            created.team_run.agent_team_id.clone(),
            id.into(),
            "Exercise readiness-aware delivery selection".into(),
            "Only authoritative-ready Work reaches provider claim".into(),
            WorkClaimMode::HostAssign,
            WorkPriority::Normal,
            compatibility_team_actor("host", "test"),
            created_at.into(),
        );
        draft.owner_member_id = Some(member.agent_member_id.clone());
        draft.active_member_run_id = Some(member.id.clone());
        draft.eligible_member_ids = vec![member.agent_member_id.clone()];
        draft.prerequisite_work_ids = prerequisites;
        store
            .insert_work(
                draft.into_work(),
                WorkCommandContext {
                    event_id: format!("{id}-created"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-create"),
                    created_at: created_at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("create assigned Work")
    };
    let predecessor = make_work("z-ready-predecessor", "unix-ms:3", Vec::new());
    let dependent = make_work(
        "a-not-ready-dependent",
        "unix-ms:4",
        vec![predecessor.id.clone()],
    );
    for (index, work) in [&predecessor, &dependent].into_iter().enumerate() {
        store
            .create_trust_work_deliveries(
                &harness_core::agentfirm_api::MutationContext {
                    execution_space_id: "unit-test-space".into(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: "test-host".into(),
                    },
                    authority_actor: None,
                    command_name: "test.work_delivery.create".into(),
                    idempotency_key: format!("ready-selection-delivery-{index}"),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                &format!("ready-selection-event-{index}"),
                &work.id,
                work.version,
                std::slice::from_ref(&member.id),
                "unix-ms:5",
            )
            .expect("create canonical WorkDelivery");
    }
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "ready-selection-supervisor",
            std::process::id(),
            "test://ready-selection-supervisor",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );

    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("select one ready Work")
        .expect("ready predecessor is claimable");
    assert_eq!(claimed.work.id, predecessor.id);
    let deliveries = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical deliveries");
    assert!(
        deliveries
            .iter()
            .all(|delivery| delivery.work_id != dependent.id),
        "not-ready Work must not receive an execution binding or fabric delivery"
    );
    let historical_delivery = store
        .latest_work_deliveries()
        .expect("compatibility delivery projection")
        .into_iter()
        .find(|delivery| delivery.work_id == dependent.id)
        .expect("not-ready compatibility delivery remains visible");
    assert_eq!(
        historical_delivery.status,
        ProviderWorkDispatchStatus::Queued
    );
    assert_eq!(historical_delivery.attempt, 0);
    assert!(historical_delivery.claim_id.is_none());
    assert!(historical_delivery.provider_receipt_id.is_none());
    let ready_compatibility = ledger
        .queued_works_for(&member.id)
        .expect("readiness-filtered compatibility queue");
    assert_eq!(ready_compatibility.len(), 1);
    assert_eq!(ready_compatibility[0].0.id, predecessor.id);
    std::fs::remove_dir_all(root).expect("cleanup");
}
