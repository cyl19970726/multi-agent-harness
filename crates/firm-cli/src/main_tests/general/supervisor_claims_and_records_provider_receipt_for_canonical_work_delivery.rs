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
                    "Exercise NodeDaemon canonical delivery wiring".into(),
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
    let current = store
        .current_work_deliveries_for_team_run(&created.team_run.id)
        .expect("current canonical WorkDelivery view");
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].delivery_id, delivery.id);
    assert_eq!(
        current[0].status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        current[0].provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    assert_eq!(current[0].attempt, 1);
    assert_eq!(
        current[0].authority,
        harness_application::CurrentWorkDeliveryAuthority::CanonicalTrust
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

    let ready_current = ledger
        .queued_works_for(&member.id)
        .expect("readiness-filtered current queue");
    assert!(
        ready_current.is_empty(),
        "no current delivery exists before a canonical binding is created"
    );

    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("select one ready Work")
        .expect("ready predecessor is claimable");
    assert_eq!(claimed.work.id, predecessor.id);
    ledger
        .fail_unreceived_work_claims_for(&member.id, "focused-negative-ack")
        .expect("settle exact canonical claim as failed");
    let deliveries = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical deliveries");
    let predecessor_delivery = deliveries
        .iter()
        .find(|delivery| delivery.work_id == predecessor.id)
        .expect("predecessor canonical delivery");
    assert_eq!(
        predecessor_delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::Failed
    );
    assert_eq!(
        predecessor_delivery.failure_code.as_deref(),
        Some("provider-negative-ack:focused-negative-ack")
    );
    assert!(
        deliveries
            .iter()
            .all(|delivery| delivery.work_id != dependent.id),
        "not-ready Work must not receive an execution binding or fabric delivery"
    );
    let current_views = store
        .current_work_deliveries_for_team_run(&created.team_run.id)
        .expect("current canonical delivery views");
    assert!(
        current_views
            .iter()
            .all(|delivery| delivery.work_id != dependent.id),
        "an unready Work must not appear in the canonical delivery projection"
    );
    let ready_after_claim = ledger
        .queued_works_for(&member.id)
        .expect("readiness-filtered current queue");
    assert!(ready_after_claim.is_empty());
    std::fs::remove_dir_all(root).expect("cleanup");
}
