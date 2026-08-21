use super::*;

#[test]
fn capacity_recovery_applies_close_latched_before_successful_cas_returns() {
    let (store, root) = temp_store("capacity-recovery-post-cas-close");
    let created = create_two_member_team_run(&store);
    let blocked = seed_capacity_blocked_member(&store, &created.member_runs[0]);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-capacity-recovery-post-cas",
            std::process::id(),
            "test://capacity-recovery-post-cas",
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
    let available = capacity_test_snapshot(ProviderCapacityState::Available);
    let mut recovered = blocked.clone();
    recover_capacity_origin_block(&mut recovered);
    recovered.provider_capacity = Some(available.clone());
    recovered.last_event_at = Some("unix-ms:101".into());

    let outcome = persist_capacity_recovery_with_hook(
        &ledger,
        &blocked,
        &mut recovered,
        available,
        |attempt, _| {
            if attempt == 0 {
                latch_member_close(
                    &store,
                    &created.team_run.id,
                    &blocked.id,
                    "host",
                    "close lands without mutating ProviderRuntimeProjection",
                )?;
            }
            Ok(())
        },
    )
    .expect("capacity recovery reconciles post-CAS Close")
    .expect("Close supersedes capacity recovery");
    assert_eq!(outcome.status, MemberRunStatus::Stopped);
    assert_eq!(recovered.status, MemberRunStatus::Stopped);
    assert!(recovered.coordination_is_closed());
    let close = store
        .latest_team_member_close_request(&blocked.id)
        .expect("read Close")
        .expect("Close row");
    assert_eq!(close.status, TeamMemberCloseStatus::Applied);
    let _ = std::fs::remove_dir_all(root);
}
