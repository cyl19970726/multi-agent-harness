use super::*;

    #[test]
    fn capacity_recovery_preserves_failed_member_after_cas_conflict() {
        let (store, root) = temp_store("capacity-recovery-conflict-failed");
        let created = create_two_member_team_run(&store);
        let blocked = seed_capacity_blocked_member(&store, &created.member_runs[0]);
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "supervisor-capacity-recovery-failed",
                std::process::id(),
                "test://capacity-recovery-failed",
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
        apply_nonblocking_capacity_observation(&mut recovered, available.clone());

        let outcome = persist_capacity_recovery_with_hook(
            &ledger,
            &blocked,
            &mut recovered,
            available,
            |attempt, expected| {
                if attempt == 0 {
                    let mut failed = expected.clone();
                    failed.status = MemberRunStatus::Failed;
                    failed.finished_at = Some("unix-ms:102".into());
                    failed.last_event_at = Some("unix-ms:102".into());
                    store.compare_and_append_member_run(expected, &failed)?;
                }
                Ok(())
            },
        )
        .expect("capacity recovery reconciles conflicting Failed")
        .expect("Failed supersedes capacity recovery");
        assert_eq!(outcome.status, MemberRunStatus::Failed);
        assert_eq!(recovered.status, MemberRunStatus::Failed);
        assert_eq!(
            ledger
                .latest_member_run(&blocked.id)
                .expect("read member")
                .expect("member row")
                .status,
            MemberRunStatus::Failed
        );
        let _ = std::fs::remove_dir_all(root);
    }

