use super::*;

#[test]
fn locally_invalid_supervisor_rejects_close_with_current_durable_generation() {
    let (store, root) = temp_store("local-latch-close-fence");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-local-latch",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire current Supervisor lease");
    let events_before = store
        .legacy_team_run_events()
        .expect("events before rejected Close");

    let error = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &AtomicBool::new(false),
        &Mutex::new(()),
        LiveMemberControlRequest::Close {
            team_run_id: created.team_run.id.clone(),
            member_run_id: member.id.clone(),
            reason: "must stay fenced".into(),
            requested_by: "test".into(),
        },
    )
    .expect_err("lost local latch must reject Close");

    assert!(
        error.is_supervisor_lease_lost(),
        "unexpected error: {error}"
    );
    assert!(
        store
            .team_member_close_requests()
            .expect("close requests")
            .is_empty(),
        "locally invalid generation persisted Close"
    );
    assert_eq!(
        store
            .legacy_team_run_events()
            .expect("events after rejected Close"),
        events_before,
        "authority-rejected Close emitted a lifecycle side effect"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
