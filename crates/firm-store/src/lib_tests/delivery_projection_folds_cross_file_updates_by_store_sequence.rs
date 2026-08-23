use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn delivery_projection_folds_cross_file_updates_by_store_sequence() {
    let (root, store, run, member, _) = work_test_fixture("delivery-fold-sequence");
    let mut assigned = unassigned_test_work(&run.id, "work-fold-sequence");
    assigned.active_member_run_id = Some(member.id.clone());
    assigned.claim_mode = WorkClaimMode::HostAssign;
    let assigned = store
        .insert_work(
            assigned,
            host_work_context("we-fold-1", "fold-create", "unix-ms:2"),
        )
        .expect("create assigned Work");
    let delivery = store
        .legacy_provider_work_dispatches_for_export()
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.work_id == assigned.id)
        .expect("initial delivery");
    let first = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-fold-1", 4, "test", 100, 10)
        .expect("first lease");
    let claimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member.id,
            &first.supervisor_id,
            first.generation,
            "claim-fold",
            101,
            // Caller timestamps are deliberately non-monotonic. The
            // Store sequence, not this string, is authoritative.
            "unix-ms:999",
        )
        .expect("claim delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
    };
    assert_eq!(claimed.status, ProviderWorkDispatchStatus::Claimed);
    let successor = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-fold-2", 5, "test", 111, 100)
        .expect("successor lease");
    store
        .reconcile_stale_work_delivery_claim(
            &run.id,
            &delivery.id,
            &successor.supervisor_id,
            successor.generation,
            112,
            "unix-ms:998",
        )
        .expect("standalone update requeues delivery");
    let released = store
        .release_work_as_host(
            &assigned.id,
            assigned.version,
            host_work_context("we-fold-2", "fold-release", "unix-ms:1"),
        )
        .expect("embedded update invalidates the later-requeued delivery");
    assert_eq!(released.version, 2);
    let projected = store
        .legacy_provider_work_dispatches_for_export()
        .expect("project deliveries")
        .into_iter()
        .find(|candidate| candidate.id == delivery.id)
        .expect("delivery remains as evidence");
    assert_eq!(projected.status, ProviderWorkDispatchStatus::Invalidated);
    let standalone_updates = store
        .read_jsonl::<ProviderWorkDispatchUpdate>("work_delivery_updates.jsonl")
        .expect("standalone updates");
    let embedded_updates = store
        .work_operations()
        .expect("operations")
        .into_iter()
        .flat_map(|operation| operation.delivery_updates)
        .collect::<Vec<_>>();
    assert!(standalone_updates
        .iter()
        .all(|update| update.update_sequence > 0));
    assert!(embedded_updates
        .iter()
        .all(|update| update.update_sequence > 0));
    assert!(
        embedded_updates
            .iter()
            .map(|update| update.update_sequence)
            .max()
            .expect("embedded sequence")
            > standalone_updates
                .iter()
                .map(|update| update.update_sequence)
                .max()
                .expect("standalone sequence")
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
