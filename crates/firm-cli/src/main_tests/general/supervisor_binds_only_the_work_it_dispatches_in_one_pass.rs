use super::*;
use harness_core::CurrentWorkDraft;

/// #729: one coordination pass dispatches exactly one Work, so it may bind
/// exactly one Work. A binding minted for a Work the pass will not claim
/// leaves a `queued` WorkDelivery frozen against runtime facts the member's
/// next provider round invalidates: the member gets `DELIVERY_NOT_DISPATCHED`,
/// and the delivery is only released
/// (`WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM`) and re-minted at the next
/// round boundary.
#[test]
fn supervisor_binds_only_the_work_it_dispatches_in_one_pass() {
    let (store, root) = temp_store("canonical-supervisor-one-binding-per-pass");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let membership = store
        .fabric_team_memberships("unit-test-space")
        .expect("Team memberships")
        .into_iter()
        .find(|membership| {
            membership.team_id == created.team_run.agent_team_id
                && membership.agent_member_id == member.agent_member_id
        })
        .expect("exact member TeamMembership");
    let assign_work = |suffix: &str, title: &str, created_at: &str| {
        let work = store
            .insert_work(
                {
                    let mut draft = CurrentWorkDraft::new(
                        format!("canonical-one-binding-{suffix}"),
                        created.team_run.id.clone(),
                        created.team_run.agent_team_id.clone(),
                        title.into(),
                        "Two ready Works are assigned before the first dispatch".into(),
                        "Provider receipt is canonical".into(),
                        WorkClaimMode::HostAssign,
                        WorkPriority::Normal,
                        compatibility_team_actor("host", "test"),
                        created_at.into(),
                    );
                    draft.eligible_member_ids = vec![member.agent_member_id.clone()];
                    draft.into_work()
                },
                WorkCommandContext {
                    event_id: format!("canonical-one-binding-{suffix}-created"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("canonical-one-binding-{suffix}-create"),
                    created_at: created_at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("create unassigned Work");
        store
            .assign_work_to_membership(
                &work.id,
                work.version,
                &membership.id,
                "unit-test-space",
                WorkCommandContext {
                    event_id: format!("canonical-one-binding-{suffix}-assigned"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("canonical-one-binding-{suffix}-assign"),
                    created_at: created_at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("assign stable TeamMembership responsibility")
    };
    // Both Works exist and are assigned before the Supervisor's first pass —
    // the pre-start shape from the #729 dogfood run.
    let first = assign_work("first", "First ready Work", "unix-ms:3");
    let second = assign_work("second", "Second ready Work", "unix-ms:4");

    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "canonical-one-binding-supervisor",
            std::process::id(),
            "test://canonical-one-binding-supervisor",
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
    assert_eq!(
        claimed.work.id, first.id,
        "the oldest ready Work of equal priority is dispatched first"
    );
    let bindings = store
        .fabric_work_execution_bindings("unit-test-space")
        .expect("canonical WorkExecutionBinding");
    assert_eq!(
        bindings.len(),
        1,
        "a pass that dispatches one Work must bind exactly one Work: {bindings:?}"
    );
    assert_eq!(bindings[0].work_id, first.id);
    let deliveries = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical WorkDelivery fabric");
    assert_eq!(
        deliveries.len(),
        1,
        "the pass must not mint a WorkDelivery it will not dispatch: {deliveries:?}"
    );
    assert_eq!(deliveries[0].work_id, first.id);
    assert_eq!(
        deliveries[0].status,
        harness_core::agentfirm_api::WorkDeliveryStatus::Claimed
    );
    assert!(deliveries[0]
        .claim_id
        .as_deref()
        .is_some_and(|claim| !claim.trim().is_empty()));

    // The still-undispatched Work is bound and claimed by the next pass, at
    // binding generation 1 — no released-before-claim generation is burned.
    ledger
        .complete_work_delivery(&claimed, "provider-work-receipt")
        .expect("record canonical provider receipt");
    let next = claim_canonical_work_for_member(&ledger, &member)
        .expect("second pass claims the remaining Work")
        .expect("one canonical Work claim");
    assert_eq!(next.work.id, second.id);
    assert_eq!(next.delivery.id, format!("work-delivery:{}:1", second.id));
    assert!(next
        .delivery
        .claim_id
        .as_deref()
        .is_some_and(|claim| !claim.trim().is_empty()));
    let deliveries = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical WorkDelivery fabric");
    assert_eq!(
        deliveries.len(),
        2,
        "one delivery per dispatch: {deliveries:?}"
    );
    assert!(
        !deliveries
            .iter()
            .any(|delivery| delivery.failure_code.as_deref()
                == Some("WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM")),
        "no delivery may be released before it was ever dispatched: {deliveries:?}"
    );

    // A repeat scan is idempotent: the exact active binding is reused, never
    // duplicated, and the already-claimed delivery is not claimed twice.
    assert!(
        claim_canonical_work_for_member(&ledger, &member)
            .expect("repeat scheduler scan is safe")
            .is_none(),
        "an already-dispatched delivery must not be claimed a second time"
    );
    let bindings = store
        .fabric_work_execution_bindings("unit-test-space")
        .expect("canonical WorkExecutionBinding");
    assert_eq!(
        bindings
            .iter()
            .filter(|binding| binding.status
                == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active)
            .count(),
        2,
        "one Active binding per dispatched Work: {bindings:?}"
    );
    drop(root);
}
