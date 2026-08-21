use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn release_clears_safe_open_ownership_and_rejects_an_in_flight_delivery() {
    let (root, store, run, member, _) = work_test_fixture("work-release");
    let mut assigned = unassigned_test_work(&run.id, "work-release-safe");
    assigned.active_member_run_id = Some(member.id.clone());
    assigned.claim_mode = WorkClaimMode::HostAssign;
    let assigned = store
        .insert_work(
            assigned,
            host_work_context("we-release-1", "create-release-1", "unix-ms:2"),
        )
        .expect("create assigned Work");
    let released = store
        .release_work(
            &assigned.id,
            assigned.version,
            &member.id,
            member_work_context(&member.id, "we-release-2", "release-owner", "unix-ms:3"),
        )
        .expect("owner releases queued Work");
    assert_eq!(released.phase, WorkPhase::Open);
    assert!(released.owner_member_id.is_none());
    assert!(released.active_member_run_id.is_none());
    assert!(store
        .latest_work_deliveries()
        .expect("deliveries")
        .iter()
        .any(|delivery| {
            delivery.work_id == released.id
                && delivery.status == ProviderWorkDispatchStatus::Invalidated
        }));

    let mut in_flight = unassigned_test_work(&run.id, "work-release-in-flight");
    in_flight.active_member_run_id = Some(member.id.clone());
    in_flight.claim_mode = WorkClaimMode::HostAssign;
    let in_flight = store
        .insert_work(
            in_flight,
            host_work_context("we-release-3", "create-release-2", "unix-ms:4"),
        )
        .expect("create second assigned Work");
    let delivery = store
        .latest_work_deliveries()
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.work_id == in_flight.id)
        .expect("queued delivery");
    let lease = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-1", 11, "test:release", 100, 100)
        .expect("lease");
    let claimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            "claim-release",
            101,
            "unix-ms:5",
        )
        .expect("claim delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
    };
    let error = store
        .release_work_as_host(
            &in_flight.id,
            in_flight.version,
            host_work_context("we-release-4", "release-host", "unix-ms:6"),
        )
        .expect_err("in-flight Work cannot be released");
    assert!(error.to_string().contains("RECONCILIATION_REQUIRED"));

    let _received = store
        .complete_work_delivery_claim(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            claimed.claim_id.as_deref().expect("claim id"),
            "native-receipt-release",
            102,
            "unix-ms:7",
        )
        .expect("record provider receipt");
    let received_error = store
        .release_work_as_host(
            &in_flight.id,
            in_flight.version,
            host_work_context("we-release-5", "release-received", "unix-ms:8"),
        )
        .expect_err("provider-received Work cannot be released");
    assert!(received_error
        .to_string()
        .contains("RECONCILIATION_REQUIRED"));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
