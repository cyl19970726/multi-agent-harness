use super::*;

#[test]
fn stale_work_revision_requires_current_supervisor_before_invalidation() {
    let harness = TestStore::new("work-delivery-stale-supervisor-order");
    let host = human("host");
    let team_run = seed_team(
        &harness.store,
        "work-delivery-stale-supervisor-order",
        &["member-a"],
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        true,
    );
    seed_team_work_from_run(&harness.store, &team_run, "work-stale");
    harness
        .store
        .create_trust_work_deliveries(
            &context(host, "work_delivery.create", "work-stale-delivery", 0),
            "work-event-stale",
            "work-stale",
            1,
            &["runtime-member-a".into()],
            "t2",
        )
        .expect("queue canonical WorkDelivery");

    let old = acquire_supervisor(&harness.store, &team_run, "supervisor-old");
    harness
        .store
        .release_team_supervisor_lease(&team_run.id, &old.supervisor_id, old.generation, unix_ms())
        .expect("release old Supervisor");
    let current = acquire_supervisor(&harness.store, &team_run, "supervisor-current");
    assert!(current.generation > old.generation);

    let delivery_before = harness.store.trust_work_deliveries(SPACE).unwrap();
    let member_before = harness.store.trust_member_runs(SPACE).unwrap();
    let work_before = harness.store.latest_works().unwrap();
    let provider_before = harness.store.latest_work_deliveries().unwrap();
    let operation_count_before = harness.store.canonical_operations().unwrap().len();

    for (actor, generation, claim_id) in [
        (
            service(&old.supervisor_id),
            old.generation,
            "stale-generation",
        ),
        (
            service("unauthorized-supervisor"),
            current.generation,
            "unauthorized-service",
        ),
    ] {
        assert_eq!(
            trust_code(
                harness
                    .store
                    .claim_trust_work_delivery(
                        &context(actor, "work_delivery.claim", claim_id, 0),
                        "work-event-stale:runtime-member-a",
                        delivery_claim(claim_id, generation, 1),
                        2,
                        "t3",
                    )
                    .expect_err("non-current Supervisor must lose before invalidation")
            ),
            TrustErrorCode::SupervisorGenerationFenced
        );
        assert_eq!(
            harness.store.canonical_operations().unwrap().len(),
            operation_count_before,
            "rejected Supervisor must append no CanonicalOperation"
        );
        assert_eq!(
            harness.store.trust_work_deliveries(SPACE).unwrap(),
            delivery_before,
            "rejected Supervisor must not invalidate or otherwise mutate WorkDelivery"
        );
        assert_eq!(
            harness.store.trust_member_runs(SPACE).unwrap(),
            member_before,
            "rejected Supervisor must not mutate MemberRun"
        );
        assert_eq!(
            harness.store.latest_works().unwrap(),
            work_before,
            "rejected Supervisor must not mutate Work"
        );
        assert_eq!(
            harness.store.latest_work_deliveries().unwrap(),
            provider_before,
            "rejected Supervisor must create no provider dispatch side effect"
        );
    }

    assert_eq!(
        trust_code(
            harness
                .store
                .claim_trust_work_delivery(
                    &context(
                        service(&current.supervisor_id),
                        "work_delivery.claim",
                        "current-invalidates-stale",
                        0,
                    ),
                    "work-event-stale:runtime-member-a",
                    delivery_claim("current-invalidates-stale", current.generation, 1,),
                    2,
                    "t4",
                )
                .expect_err("current Supervisor intentionally invalidates stale Work revision")
        ),
        TrustErrorCode::WorkRevisionStale
    );
    let invalidated = harness.store.trust_work_deliveries(SPACE).unwrap();
    assert_eq!(invalidated.len(), 1);
    assert_eq!(
        invalidated[0].status,
        firm_core::agentfirm_api::WorkDeliveryStatus::Invalidated
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        operation_count_before + 1,
        "only the authorized current Supervisor may persist stale-revision invalidation"
    );
}
