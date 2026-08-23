use super::*;

#[test]
fn work_event_id_reuse_is_rejected_before_delivery_identity_can_collide() {
    let (root, store, run, member, _) = work_test_fixture("event-id-uniqueness");
    let mut first = unassigned_test_work(&run.id, "work-event-id-first");
    first.active_member_run_id = Some(member.id.clone());
    first.claim_mode = WorkClaimMode::HostAssign;
    store
        .insert_work(
            first,
            host_work_context("same-work-event", "event-first", "unix-ms:2"),
        )
        .expect("first event and delivery");
    let mut second = unassigned_test_work(&run.id, "work-event-id-second");
    second.active_member_run_id = Some(member.id.clone());
    second.claim_mode = WorkClaimMode::HostAssign;
    let error = store
        .insert_work(
            second,
            host_work_context("same-work-event", "event-second", "unix-ms:3"),
        )
        .expect_err("caller event id reuse must be rejected");
    assert!(error.to_string().contains("WORK_EVENT_ID_CONFLICT"));
    assert_eq!(store.work_operations().expect("operations").len(), 1);
    assert_eq!(
        store
            .legacy_provider_work_dispatches_for_export()
            .expect("deliveries")
            .len(),
        1
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
