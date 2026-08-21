use super::*;

    #[test]
    fn provider_start_claim_reconciles_close_latched_between_read_and_cas() {
        let (store, root) = temp_store("provider-start-claim-close-race");
        let created = create_two_member_team_run(&store);
        let member = created.member_runs[0].clone();
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "supervisor-start-claim-close",
                std::process::id(),
                "test://start-claim-close",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire Supervisor lease");
        ensure_test_runtime_fabric(&store, &created, &lease);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let store_root = store.root().to_path_buf();
        let run_id = created.team_run.id.clone();
        let supervisor_id = lease.supervisor_id.clone();
        let generation = lease.generation;
        let scheduled = member.clone();
        let worker = std::thread::spawn(move || {
            let worker_store = HarnessStore::new(store_root);
            let ledger = TeamRunLedger::new(
                &worker_store,
                &run_id,
                &supervisor_id,
                generation,
                Arc::new(AtomicBool::new(true)),
            );
            claim_member_provider_start_with_hook(&ledger, &scheduled, |attempt, _| {
                if attempt == 0 {
                    worker_barrier.wait();
                    worker_barrier.wait();
                }
                Ok(())
            })
        });

        barrier.wait();
        let close = latch_member_close(
            &store,
            &created.team_run.id,
            &member.id,
            "host",
            "latch lands after claim read",
        )
        .expect("latch Close without yet changing coordination");
        barrier.wait();
        let outcome = worker
            .join()
            .expect("claim worker")
            .expect("post-claim Close reconciliation");
        assert!(matches!(outcome, MemberProviderStartClaim::Superseded(_)));
        let latest = latest_member_runs_in_append_order(&store)
            .expect("latest members")
            .into_iter()
            .find(|candidate| candidate.id == member.id)
            .expect("member");
        assert_eq!(latest.coordination_status, MemberCoordinationStatus::Closed);
        assert_eq!(latest.status, MemberRunStatus::Stopped);
        let applied = store
            .latest_team_member_close_request(&member.id)
            .expect("close request")
            .expect("close row");
        assert_eq!(applied.id, close.id);
        assert_eq!(applied.status, TeamMemberCloseStatus::Applied);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

