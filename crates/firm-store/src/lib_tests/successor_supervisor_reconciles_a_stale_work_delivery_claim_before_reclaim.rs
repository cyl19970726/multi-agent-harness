use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn successor_supervisor_reconciles_a_stale_work_delivery_claim_before_reclaim() {
    let (root, store, run, member, _) = work_test_fixture("work-delivery-reconcile");
    let mut assigned = unassigned_test_work(&run.id, "work-reconcile");
    assigned.active_member_run_id = Some(member.id.clone());
    assigned.claim_mode = WorkClaimMode::HostAssign;
    store
        .insert_work(
            assigned,
            host_work_context("we-reconcile-1", "create-reconcile", "unix-ms:2"),
        )
        .expect("create assigned Work");
    let delivery = store
        .latest_work_deliveries()
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.work_id == "work-reconcile")
        .expect("queued delivery");
    let first = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-1", 11, "test:first", 100, 10)
        .expect("first lease");
    let claimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member.id,
            &first.supervisor_id,
            first.generation,
            "claim-generation-1",
            101,
            "unix-ms:3",
        )
        .expect("first claim")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
    };
    assert_eq!(claimed.attempt, 1);

    let second = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-2", 22, "test:successor", 111, 100)
        .expect("successor lease");
    assert_eq!(second.generation, 2);
    let requeued = store
        .reconcile_stale_work_delivery_claim(
            &run.id,
            &delivery.id,
            &second.supervisor_id,
            second.generation,
            112,
            "unix-ms:4",
        )
        .expect("successor reconciles stale claim");
    assert_eq!(requeued.status, ProviderWorkDispatchStatus::Queued);
    assert_eq!(requeued.attempt, 1);
    assert!(requeued.claim_id.is_none());

    let reclaimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member.id,
            &second.supervisor_id,
            second.generation,
            "claim-generation-2",
            113,
            "unix-ms:5",
        )
        .expect("successor reclaims delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("delivery must be reclaimable"),
    };
    assert_eq!(reclaimed.attempt, 2);
    assert_eq!(reclaimed.claimed_generation, Some(second.generation));
    let received = store
        .complete_work_delivery_claim(
            &run.id,
            &delivery.id,
            &member.id,
            &second.supervisor_id,
            second.generation,
            reclaimed.claim_id.as_deref().expect("second claim id"),
            "native-receipt-reconcile",
            114,
            "unix-ms:6",
        )
        .expect("record provider receipt");
    assert_eq!(
        received.status,
        ProviderWorkDispatchStatus::ProviderReceived
    );
    assert_eq!(
        store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &second.supervisor_id,
                second.generation,
                reclaimed.claim_id.as_deref().expect("second claim id"),
                "native-receipt-reconcile",
                115,
                "unix-ms:6-retry",
            )
            .expect("same provider receipt retry is idempotent"),
        received
    );
    let different_receipt = store
        .complete_work_delivery_claim(
            &run.id,
            &delivery.id,
            &member.id,
            &second.supervisor_id,
            second.generation,
            reclaimed.claim_id.as_deref().expect("second claim id"),
            "different-native-receipt",
            116,
            "unix-ms:6-retry-2",
        )
        .expect_err("a retry cannot rewrite receipt identity");
    assert!(different_receipt
        .to_string()
        .contains("different provider receipt"));
    let third = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-3", 33, "test:third", 212, 100)
        .expect("third lease");
    let uncertain = store
        .reconcile_stale_work_delivery_claim(
            &run.id,
            &delivery.id,
            &third.supervisor_id,
            third.generation,
            213,
            "unix-ms:7",
        )
        .expect_err("provider-received delivery is never rolled back");
    assert!(uncertain.to_string().contains("cannot be requeued"));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
