use super::*;

    #[test]
    fn admitted_compatibility_block_recovers_into_start_machine_once() {
        let (unscoped, root) = temp_store("compatibility-block-start-recovery");
        let store = unscoped.with_provider_compatibility_scope("project-test", "store-test");
        let created = create_two_member_team_run(&store);
        let initial = created.member_runs[0].clone();
        let mut profile = team_member_provider_profile_for_mode("codex", Some("codex_app_server"));
        apply_provider_version(&mut profile, Some("9.9.9".into()));
        let mut blocked = store
            .block_member_run_for_provider_compatibility(
                &initial,
                &profile,
                compatibility_test_cause(&initial, &profile),
                "unix-ms:1",
            )
            .expect("seed compatibility-owned Blocked member");
        blocked.provider_environment_observation =
            Some(test_provider_environment_observation(&root));
        let before_workspace = store.member_runs().unwrap().last().unwrap().clone();
        store
            .compare_and_append_member_run(&before_workspace, &blocked)
            .expect("record workspace without changing typed cause");
        let action = compatibility_block_action(&blocked, &profile, 1);
        store
            .append_member_action(&action)
            .expect("record exact compatibility provenance");
        let (project_id, store_id) = store.provider_compatibility_scope().unwrap();
        store
            .admit_provider_compatibility_admission(&ProviderCompatibilityAdmission {
                id: "recovery-admission".into(),
                project_id: project_id.into(),
                store_id: store_id.into(),
                provider: profile.provider.clone(),
                execution_mode: profile.execution_mode.clone(),
                provider_version: profile.provider_version.clone().unwrap(),
                adapter_contract_version: profile.adapter_contract_version.clone().unwrap(),
                policy: ProviderCompatibilityAdmissionPolicy::Strict,
                actor: "operator".into(),
                evidence_refs: vec!["evidence:test".into()],
                admitted_at: "unix-ms:2".into(),
                lifecycle: ProviderCompatibilityAdmissionLifecycle::Active,
                predecessor_admission_id: None,
                reason: None,
            })
            .expect("admit exact tuple");

        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "compatibility-recovery-supervisor",
                std::process::id(),
                "test://compatibility-recovery",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire supervisor lease");
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let callbacks = AtomicUsize::new(0);
        assert!(matches!(
            claim_member_provider_start_with_hook(&ledger, &blocked, |_, _| {
                callbacks.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .expect("blocked member is fenced before provider start"),
            MemberProviderStartClaim::Superseded(_)
        ));
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);

        assert!(compatibility_block_matches_current_tuple(
            &blocked, &profile
        ));
        let recovery_status = compatibility_recovery_status(&store, &blocked).expect("status");
        let recovered = store
            .recover_member_run_from_provider_compatibility_block(
                &blocked,
                &profile,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                recovery_status,
                "unix-ms:2",
            )
            .expect("typed Store recovery");
        assert_eq!(recovered.status, MemberRunStatus::Queued);
        assert!(
            !compatibility_block_matches_current_tuple(&recovered, &profile),
            "recovery is idempotent because only Blocked rows are eligible"
        );

        let first = claim_member_provider_start_with_hook(&ledger, &recovered, |_, _| {
            callbacks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .expect("admitted member starts");
        let MemberProviderStartClaim::Claimed(starting) = first else {
            panic!("recovered member must enter Starting");
        };
        assert_eq!(callbacks.load(Ordering::SeqCst), 1);
        assert!(matches!(
            claim_member_provider_start_with_hook(&ledger, &starting, |_, _| {
                callbacks.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .expect("duplicate start is superseded"),
            MemberProviderStartClaim::Superseded(_)
        ));
        assert_eq!(callbacks.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

