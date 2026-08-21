use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn host_rebind_fences_old_runtime_and_preserves_provider_receipt_evidence() {
    let (root, store, run, member, peer) = work_test_fixture("work-rebind-runtime");
    let mut assigned = unassigned_test_work(&run.id, "work-rebind");
    assigned.claim_mode = WorkClaimMode::HostAssign;
    assigned.active_member_run_id = Some(member.id.clone());
    let assigned = store
        .insert_work(
            assigned,
            host_work_context("we-rebind-1", "rebind-create", "unix-ms:2"),
        )
        .expect("create assigned Work");
    let delivery = store
        .latest_work_deliveries()
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.work_id == assigned.id)
        .expect("initial delivery");
    let lease = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-rebind", 9, "test", 100, 100)
        .expect("lease");
    let claimed = match store
        .claim_work_delivery(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            "claim-rebind",
            101,
            "unix-ms:3",
        )
        .expect("claim initial delivery")
    {
        WorkDeliveryClaimResult::Claimed(delivery) => delivery,
        WorkDeliveryClaimResult::NotQueued => panic!("initial delivery must be queued"),
    };
    store
        .complete_work_delivery_claim(
            &run.id,
            &delivery.id,
            &member.id,
            &lease.supervisor_id,
            lease.generation,
            claimed.claim_id.as_deref().expect("claim id"),
            "provider-receipt-before-crash",
            102,
            "unix-ms:4",
        )
        .expect("provider receipt");
    let started = store
        .start_work(
            &assigned.id,
            assigned.version,
            &member.id,
            member_work_context(&member.id, "we-rebind-2", "rebind-start", "unix-ms:5"),
        )
        .expect("start before runtime crash");

    let mut failed_previous = member.clone();
    failed_previous.status = MemberRunStatus::Failed;
    failed_previous.finished_at = Some("unix-ms:6".into());
    store
        .compare_and_append_member_run(&member, &failed_previous)
        .expect("record previous runtime failure");

    let mut replacement = member.clone();
    replacement.id = "member-a-generation-2".into();
    replacement.runtime_generation = member.runtime_generation + 1;
    replacement.status = MemberRunStatus::Idle;
    replacement.started_at = "unix-ms:7".into();
    replacement.finished_at = None;
    admit_replacement_for_test(&store, &replacement);
    let owner_mismatch = store
        .rebind_work(
            &started.id,
            started.version,
            &peer.id,
            host_work_context("ignored", "rebind-peer", "unix-ms:8"),
        )
        .expect_err("Host cannot change stable owner through rebind");
    assert!(owner_mismatch.to_string().contains("OWNER_MISMATCH"));
    let rebound = store
        .rebind_work(
            &started.id,
            started.version,
            &replacement.id,
            host_work_context("we-rebind-3", "rebind-runtime", "unix-ms:9"),
        )
        .expect("Host rebinds stable owner to replacement runtime");
    assert_eq!(rebound.phase, WorkPhase::Active);
    assert_eq!(rebound.owner_member_id, started.owner_member_id);
    assert_eq!(
        rebound.active_member_run_id.as_deref(),
        Some(replacement.id.as_str())
    );
    let deliveries = store.latest_work_deliveries().expect("deliveries");
    assert!(deliveries.iter().any(|candidate| {
        candidate.id == delivery.id
            && candidate.status == ProviderWorkDispatchStatus::ProviderReceived
            && candidate.provider_receipt_id.as_deref() == Some("provider-receipt-before-crash")
    }));
    let replacement_delivery = deliveries
        .iter()
        .find(|candidate| {
            candidate.work_id == rebound.id
                && candidate.work_version == rebound.version
                && candidate.recipient_member_run_id == replacement.id
        })
        .expect("fresh delivery for replacement");
    assert!(matches!(
        store
            .claim_work_delivery(
                &run.id,
                &replacement_delivery.id,
                &replacement.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-replacement",
                103,
                "unix-ms:11",
            )
            .expect("in-progress revision is deliverable"),
        WorkDeliveryClaimResult::Claimed(_)
    ));
    let fenced = store
        .submit_work(
            &started.id,
            started.version,
            &member.id,
            "stale runtime result",
            Vec::new(),
            Vec::new(),
            member_work_context(&member.id, "ignored", "stale-submit", "unix-ms:12"),
        )
        .expect_err("old runtime version is fenced");
    assert!(fenced.to_string().contains("VERSION_CONFLICT"));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
