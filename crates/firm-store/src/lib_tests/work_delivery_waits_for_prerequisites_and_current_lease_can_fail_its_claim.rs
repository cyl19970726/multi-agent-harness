use super::*;

#[test]
#[ignore = "legacy Work acceptance route is retired; canonical exact-candidate acceptance is covered by member_execution_trust"]
fn work_delivery_waits_for_prerequisites_and_current_lease_can_fail_its_claim() {
    let (root, store, run, member_a, member_b) = work_test_fixture("work-delivery-ready");
    let prerequisite = store
        .insert_work(
            unassigned_test_work(&run.id, "work-prerequisite"),
            host_work_context("we-ready-1", "ready-create-prereq", "unix-ms:2"),
        )
        .expect("create prerequisite");
    let claimed_prerequisite = store
        .claim_work(
            &prerequisite.id,
            prerequisite.version,
            &member_b.id,
            member_work_context(
                &member_b.id,
                "we-ready-2",
                "ready-claim-prereq",
                "unix-ms:3",
            ),
        )
        .expect("claim prerequisite");

    let mut dependent = unassigned_test_work(&run.id, "work-dependent");
    dependent.claim_mode = WorkClaimMode::HostAssign;
    dependent.active_member_run_id = Some(member_a.id.clone());
    dependent.prerequisite_work_ids = vec![prerequisite.id.clone()];
    let dependent = store
        .insert_work(
            dependent,
            host_work_context("we-ready-3", "ready-create-dependent", "unix-ms:4"),
        )
        .expect("create dependent");
    let delivery = store
        .latest_work_deliveries()
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.work_id == dependent.id)
        .expect("dependent delivery");
    let lease = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-ready", 7, "test", 100, 100)
        .expect("lease");
    assert_eq!(
        store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-before-ready",
                101,
                "unix-ms:5",
            )
            .expect("not ready is not an error"),
        WorkDeliveryClaimResult::NotQueued
    );

    let submitted = store
        .submit_work(
            &prerequisite.id,
            claimed_prerequisite.version,
            &member_b.id,
            "prerequisite complete",
            Vec::new(),
            vec!["check://ready".into()],
            member_work_context(
                &member_b.id,
                "we-ready-4",
                "ready-submit-prereq",
                "unix-ms:6",
            ),
        )
        .expect("submit prerequisite");
    store
        .accept_work(
            &submitted.id,
            submitted.version,
            host_work_context("we-ready-5", "ready-accept-prereq", "unix-ms:7"),
        )
        .expect("accept prerequisite");

    let claimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member_a.id,
            &lease.supervisor_id,
            lease.generation,
            "claim-after-ready",
            102,
            "unix-ms:8",
        )
        .expect("claim ready delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("delivery must now be claimable"),
    };
    let failed = store
        .fail_work_delivery_claim(
            &run.id,
            &delivery.id,
            &member_a.id,
            &lease.supervisor_id,
            lease.generation,
            claimed.claim_id.as_deref().expect("claim id"),
            "provider transport exited before receipt",
            103,
            "unix-ms:9",
        )
        .expect("current lease fails claim");
    assert_eq!(failed.status, ProviderWorkDispatchStatus::Failed);
    assert_eq!(
        failed.failure_reason.as_deref(),
        Some("provider transport exited before receipt")
    );
    assert_eq!(
        store
            .fail_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "provider transport exited before receipt",
                104,
                "unix-ms:10",
            )
            .expect("same failure retry is idempotent"),
        failed
    );
    let retried = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member_a.id,
            &lease.supervisor_id,
            lease.generation,
            "claim-after-transport-failure",
            105,
            "unix-ms:11",
        )
        .expect("failed pre-receipt delivery remains retryable")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => {
            panic!("failed pre-receipt delivery must be retryable")
        }
    };
    assert_eq!(retried.status, ProviderWorkDispatchStatus::Claimed);
    assert_eq!(retried.attempt, 2);
    assert_eq!(
        retried.claim_id.as_deref(),
        Some("claim-after-transport-failure")
    );
    assert!(retried.failure_reason.is_none());
    std::fs::remove_dir_all(root).expect("remove temp store");
}
