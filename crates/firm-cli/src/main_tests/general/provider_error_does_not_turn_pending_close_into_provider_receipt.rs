use super::*;

    #[test]
    fn provider_error_does_not_turn_pending_close_into_provider_receipt() {
        use harness_core::agentfirm_api::{RuntimeActivity, RuntimeResidency};

        let (store, root) = temp_store("provider-error-pending-close-is-not-receipt");
        let created = create_two_member_team_run(&store);
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "supervisor-pending-close-no-receipt",
                std::process::id(),
                "test://pending-close-no-receipt",
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
        let initial = created.member_runs[0].clone();
        let mut bound = initial.clone();
        bound.native_session = Some(capacity_test_session());
        bound.status = MemberRunStatus::Starting;
        bound.last_event_at = Some(now_string());
        ledger
            .save_member_run(&initial, &bound)
            .expect("bind provider-native session");
        transition_provider_session_runtime_control(
            &ledger,
            &bound,
            RuntimeResidency::Attached,
            RuntimeActivity::Idle,
        )
        .expect("record live adapter attachment");
        let close = latch_member_close(
            &store,
            &created.team_run.id,
            &bound.id,
            "host",
            "provider Close transport failed before receipt",
        )
        .expect("persist Close intent");

        let mut latest = ledger
            .latest_member_run(&bound.id)
            .expect("latest member")
            .expect("member exists");
        assert!(
            !reconcile_member_lifecycle_after_provider_error(&ledger, &mut latest)
                .expect("unknown Close effect stays unresolved")
        );
        let after = ledger
            .latest_member_run(&bound.id)
            .expect("member after failed Close")
            .expect("member exists");
        assert_ne!(after.status, MemberRunStatus::Stopped);
        assert!(after.coordination_is_active());
        assert_eq!(
            store
                .latest_team_member_close_request(&bound.id)
                .expect("Close request")
                .expect("Close row"),
            close,
            "operator intent remains pending until an exact provider receipt exists"
        );
        assert!(store
            .runtime_commands(&lease.execution_space_id)
            .expect("RuntimeCommands")
            .iter()
            .all(|command| command.command
                != harness_core::agentfirm_api::RuntimeCommandKind::CloseMember));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

