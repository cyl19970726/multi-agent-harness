use super::*;

    #[test]
    fn provider_start_claim_accepts_reopened_queued_generation() {
        let (store, root) = temp_store("provider-start-claim-reopened-queued");
        let created = create_two_member_team_run(&store);
        let unbound = created.member_runs[0].clone();
        let mut initial = unbound.clone();
        initial.native_session = Some(capacity_test_session());
        initial.last_event_at = Some("unix-ms:100".into());
        store
            .compare_and_append_member_run(&unbound, &initial)
            .expect("seed exact provider-native session before Close");
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "supervisor-reopened-queued",
                std::process::id(),
                "test://reopened-queued",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire Supervisor lease");
        ensure_test_runtime_fabric(&store, &created, &lease);
        let mut reopened = initial.clone();
        reopened.runtime_generation = initial.runtime_generation + 1;
        reopened.status = MemberRunStatus::Queued;
        reopened.started_at = "unix-ms:200".into();
        reopened.last_event_at = Some("unix-ms:200".into());
        store
            .compare_and_advance_member_run_generation(&initial, &reopened)
            .expect("seed reopened queued generation");
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );

        let (_, retained_session) = provider_session_for_member(&ledger, &reopened)
            .expect("Reopen reuses the machine-owned AgentSession");
        assert_eq!(
            retained_session.runtime_generation,
            initial.runtime_generation
        );
        assert_eq!(reopened.runtime_generation, initial.runtime_generation + 1);
        assert_eq!(
            retained_session
                .native_session_ref
                .as_ref()
                .map(|native| native.native_session_id.as_str()),
            reopened
                .native_session
                .as_ref()
                .map(|native| native.native_session_id.as_str()),
            "the higher Team adapter generation must retain exact provider-native history"
        );

        let claimed = claim_member_provider_start(&ledger, &reopened)
            .expect("reopened generation claims provider start");
        let MemberProviderStartClaim::Claimed(starting) = claimed else {
            panic!("reopened queued generation must become Starting");
        };
        assert_eq!(starting.runtime_generation, reopened.runtime_generation);
        assert_eq!(starting.status, MemberRunStatus::Starting);
        assert_eq!(starting.native_session, reopened.native_session);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

