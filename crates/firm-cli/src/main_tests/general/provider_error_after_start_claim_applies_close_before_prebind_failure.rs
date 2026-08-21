use super::*;

    #[test]
    fn provider_error_after_start_claim_applies_close_before_prebind_failure() {
        let (store, root) = temp_store("provider-error-after-start-claim-close");
        let created = create_two_member_team_run(&store);
        let member = created.member_runs[0].clone();
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "supervisor-post-claim-close",
                std::process::id(),
                "test://post-claim-close",
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
        let starting =
            match claim_member_provider_start(&ledger, &member).expect("claim provider start") {
                MemberProviderStartClaim::Claimed(starting) => starting,
                _ => panic!("active idle member must claim Starting"),
            };
        assert_eq!(starting.status, MemberRunStatus::Starting);
        assert!(starting.native_session.is_none());

        let close = latch_member_close(
            &store,
            &created.team_run.id,
            &member.id,
            "host",
            "Close lands after Start linearized but before provider bind",
        )
        .expect("latch Close");
        mark_member_coordination_closed(&store, &created.team_run.id, &member.id)
            .expect("close coordination after claim");
        let mut latest = ledger
            .latest_member_run(&member.id)
            .expect("latest member")
            .expect("member exists");
        assert!(
            reconcile_member_lifecycle_after_provider_error(&ledger, &mut latest)
                .expect("provider Err path applies Close")
        );
        assert_eq!(latest.coordination_status, MemberCoordinationStatus::Closed);
        assert_eq!(latest.status, MemberRunStatus::Stopped);
        assert!(latest.native_session.is_none());
        let applied = store
            .latest_team_member_close_request(&member.id)
            .expect("close request")
            .expect("close row");
        assert_eq!(applied.id, close.id);
        assert_eq!(applied.status, TeamMemberCloseStatus::Applied);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

