use super::*;

#[test]
fn capacity_block_applies_close_latched_before_successful_cas_returns() {
    let (store, root) = temp_store("capacity-block-post-cas-close");
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-capacity-block-post-cas",
            std::process::id(),
            "test://capacity-block-post-cas",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let exhausted = capacity_test_snapshot(ProviderCapacityState::Exhausted);
    let mut blocked = initial.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.provider_capacity = Some(exhausted.clone());
    blocked.last_event_at = Some("unix-ms:101".into());

    let outcome = persist_capacity_block_with_hook(
        &ledger,
        &initial,
        &mut blocked,
        exhausted,
        |attempt, _| {
            if attempt == 0 {
                latch_member_close(
                    &store,
                    &created.team_run.id,
                    &initial.id,
                    "host",
                    "close lands without mutating ProviderRuntimeProjection",
                )?;
            }
            Ok(())
        },
    )
    .expect("capacity block reconciles post-CAS Close")
    .expect("Close supersedes capacity block");
    assert_eq!(outcome.status, MemberRunStatus::Stopped);
    assert_eq!(blocked.status, MemberRunStatus::Stopped);
    assert!(blocked.coordination_is_closed());
    assert_eq!(
        store
            .latest_team_member_close_request(&initial.id)
            .expect("read Close")
            .expect("Close row")
            .status,
        TeamMemberCloseStatus::Applied
    );
    let _ = std::fs::remove_dir_all(root);
}
