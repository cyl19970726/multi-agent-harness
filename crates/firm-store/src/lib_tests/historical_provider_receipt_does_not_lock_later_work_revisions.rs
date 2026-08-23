use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn historical_provider_receipt_does_not_lock_later_work_revisions() {
    let (root, store, run, member, peer) = work_test_fixture("historical-receipt");
    let mut assigned = unassigned_test_work(&run.id, "work-historical-receipt");
    assigned.active_member_run_id = Some(member.id.clone());
    assigned.claim_mode = WorkClaimMode::HostAssign;
    let assigned = store
        .insert_work(
            assigned,
            host_work_context("we-history-1", "history-create", "unix-ms:2"),
        )
        .expect("create assigned Work");
    let delivery = store
        .legacy_provider_work_dispatches_for_export()
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.work_id == assigned.id)
        .expect("initial delivery");
    let lease = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-history", 3, "test", 100, 100)
        .expect("lease");
    let claimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            "claim-history",
            101,
            "unix-ms:3",
        )
        .expect("claim delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
    };
    store
        .complete_work_delivery_claim(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            claimed.claim_id.as_deref().expect("claim id"),
            "native-receipt-history",
            102,
            "unix-ms:4",
        )
        .expect("provider receives revision 1");

    let mut failed_previous = member.clone();
    failed_previous.status = MemberRunStatus::Failed;
    failed_previous.finished_at = Some("unix-ms:5".into());
    store
        .compare_and_append_member_run(&member, &failed_previous)
        .expect("record runtime failure");
    let mut replacement = member.clone();
    replacement.id = "member-history-generation-2".into();
    replacement.runtime_generation += 1;
    replacement.status = MemberRunStatus::Idle;
    replacement.started_at = "unix-ms:6".into();
    replacement.finished_at = None;
    admit_replacement_for_test(&store, &replacement);

    let rebound = store
        .rebind_work(
            &assigned.id,
            assigned.version,
            &replacement.id,
            host_work_context("we-history-2", "history-rebind", "unix-ms:7"),
        )
        .expect("rebind advances Work beyond historical receipt");
    let released = store
        .release_work_as_host(
            &rebound.id,
            rebound.version,
            host_work_context("we-history-3", "history-release", "unix-ms:8"),
        )
        .expect("historical receipt must not block release of newer revision");
    let reassigned = store
        .assign_work(
            &released.id,
            released.version,
            &peer.id,
            host_work_context("we-history-4", "history-assign", "unix-ms:9"),
        )
        .expect("historical receipt must not block later assignment");
    assert_eq!(
        reassigned.active_member_run_id.as_deref(),
        Some(peer.id.as_str())
    );
    assert!(store
        .legacy_provider_work_dispatches_for_export()
        .expect("deliveries")
        .iter()
        .any(|candidate| {
            candidate.id == delivery.id
                && candidate.status == ProviderWorkDispatchStatus::ProviderReceived
                && candidate.provider_receipt_id.as_deref() == Some("native-receipt-history")
        }));
    let reassigned_delivery = store
        .legacy_provider_work_dispatches_for_export()
        .expect("deliveries")
        .into_iter()
        .find(|candidate| {
            candidate.work_id == reassigned.id
                && candidate.work_version == reassigned.version
                && candidate.recipient_member_run_id == peer.id
        })
        .expect("reassigned delivery");
    let reassigned_claim = match store
        .claim_work_delivery(
            &run.id,
            &reassigned_delivery.id,
            &peer.id,
            &lease.supervisor_id,
            lease.generation,
            "claim-reassigned-history",
            103,
            "unix-ms:10",
        )
        .expect("claim reassigned delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("reassigned delivery must be claimed"),
    };
    store
        .complete_work_delivery_claim(
            &run.id,
            &reassigned_delivery.id,
            &peer.id,
            &lease.supervisor_id,
            lease.generation,
            reassigned_claim.claim_id.as_deref().expect("claim id"),
            "native-receipt-reassigned",
            104,
            "unix-ms:11",
        )
        .expect("provider receives reassigned revision");
    let started = store
        .start_work(
            &reassigned.id,
            reassigned.version,
            &peer.id,
            member_work_context(&peer.id, "we-history-5", "history-start", "unix-ms:12"),
        )
        .expect("member advances beyond its provider receipt");
    let cancelled = store
        .cancel_work(
            &started.id,
            started.version,
            "Host no longer needs this Work",
            host_work_context("we-history-6", "history-cancel", "unix-ms:13"),
        )
        .expect("historical receipts must not block cancellation");
    assert_eq!(cancelled.resolution, Some(WorkResolution::Cancelled));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
