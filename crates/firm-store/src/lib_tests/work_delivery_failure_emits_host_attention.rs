use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn work_delivery_failure_emits_host_attention() {
    let (root, store, run, member, _) = work_test_fixture("work-wdf-ha");
    let mut assigned = unassigned_test_work(&run.id, "work-wdf-ha-1");
    assigned.active_member_run_id = Some(member.id.clone());
    assigned.claim_mode = WorkClaimMode::HostAssign;
    let assigned = store
        .insert_work(
            assigned,
            host_work_context("we-wdf-1", "create-wdf-ha", "unix-ms:2"),
        )
        .expect("create assigned Work");
    let delivery = store
        .legacy_provider_work_dispatches_for_export()
        .expect("deliveries")
        .into_iter()
        .find(|d| d.work_id == assigned.id)
        .expect("delivery");
    let lease = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-wdf", 7, "test", 100, 100)
        .expect("lease");
    let claimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            "claim-wdf",
            100,
            "unix-ms:3",
        )
        .expect("claim")
    {
        WorkDeliveryClaimResult::Claimed(d) => d,
        _ => panic!("delivery must be claimed"),
    };
    let failed = store
        .fail_work_delivery_claim(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            claimed.claim_id.as_deref().expect("claim id"),
            "provider crash",
            101,
            "unix-ms:4",
        )
        .expect("fail delivery");
    assert_eq!(failed.status, ProviderWorkDispatchStatus::Failed);
    let attentions = store.host_attentions().expect("host attentions");
    let wdf = attentions
        .iter()
        .find(|a| a.work_id == assigned.id && a.kind == HostAttentionKind::WorkDeliveryFailed);
    assert!(
        wdf.is_some(),
        "must emit WorkDeliveryFailed for failed delivery claim"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
