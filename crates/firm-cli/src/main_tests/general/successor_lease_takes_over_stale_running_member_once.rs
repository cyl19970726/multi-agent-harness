use super::*;

    #[test]
    fn successor_lease_takes_over_stale_running_member_once() {
        let (store, root) = temp_store("successor-takes-over-stale-running-member");
        let created = create_two_member_team_run(&store);
        let initial = created.member_runs[0].clone();
        let first_lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "stale-supervisor",
                std::process::id(),
                "test://stale-supervisor",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire stale Supervisor lease");
        let mut stale_running = initial.clone();
        stale_running.status = MemberRunStatus::Running;
        stale_running.native_session = Some(capacity_test_session());
        stale_running.last_event_at = Some("unix-ms:1".into());
        store
            .compare_and_append_member_run(&initial, &stale_running)
            .expect("seed Running row owned by stale Supervisor");
        store
            .release_team_supervisor_lease(
                &created.team_run.id,
                &first_lease.supervisor_id,
                first_lease.generation,
                current_unix_ms_u64(),
            )
            .expect("release stale Supervisor lease");
        let successor_lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "successor-supervisor",
                std::process::id(),
                "test://successor-supervisor",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire successor Supervisor lease");
        assert!(successor_lease.generation > first_lease.generation);
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &successor_lease.supervisor_id,
            successor_lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let snapshot = test_provider_environment_observation(&root);

        let published = match prepare_member_workspace_for_spawn(&ledger, &stale_running, &snapshot)
            .expect("successor publishes workspace before attaching stale runtime")
        {
            PreSpawnWorkspacePreparation::Ready(member) => *member,
            _ => panic!("stale Running member must remain spawnable by its successor"),
        };
        assert_eq!(published.status, MemberRunStatus::Running);
        assert_eq!(published.native_session, stale_running.native_session);

        let claimed = claim_member_provider_start(&ledger, &published)
            .expect("successor claims stale provider lifecycle");
        let MemberProviderStartClaim::Claimed(starting) = claimed else {
            panic!("successor must move stale Running member back to Starting");
        };
        assert_eq!(starting.status, MemberRunStatus::Starting);
        assert_eq!(starting.native_session, stale_running.native_session);

        assert!(matches!(
            claim_member_provider_start(&ledger, &starting)
                .expect("current-owner duplicate start is a local supersession"),
            MemberProviderStartClaim::Superseded(_)
        ));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

