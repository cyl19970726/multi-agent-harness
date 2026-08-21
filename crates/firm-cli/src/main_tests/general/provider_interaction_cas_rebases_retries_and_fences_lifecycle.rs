use super::*;

    #[test]
    fn provider_interaction_cas_rebases_retries_and_fences_lifecycle() {
        let (store, root) = temp_store("provider-interaction-cas");
        let created = create_two_member_team_run(&store);
        let ledger = TeamRunLedger::without_supervisor(&store, &created.team_run.id);
        let initial = created.member_runs[0].clone();
        let mut running = initial.clone();
        running.status = MemberRunStatus::Running;
        store
            .compare_and_append_member_run(&initial, &running)
            .expect("seed provider round");

        // The callback receives the same round snapshot twice. Each cycle must
        // refetch latest rather than reuse that stale snapshot.
        for _ in 0..2 {
            let waiting = match transition_provider_interaction_member(
                &ledger,
                &running,
                MemberRunStatus::Waiting,
            )
            .expect("enter Waiting")
            {
                ProviderInteractionMemberTransition::Applied(member) => member,
                ProviderInteractionMemberTransition::LifecycleSuperseded => {
                    panic!("active member cannot be lifecycle-superseded")
                }
            };
            assert_eq!(waiting.status, MemberRunStatus::Waiting);
            let resumed = match transition_provider_interaction_member(
                &ledger,
                &waiting,
                MemberRunStatus::Running,
            )
            .expect("resume Running")
            {
                ProviderInteractionMemberTransition::Applied(member) => member,
                ProviderInteractionMemberTransition::LifecycleSuperseded => {
                    panic!("active member cannot be lifecycle-superseded")
                }
            };
            assert_eq!(resumed.status, MemberRunStatus::Running);
        }

        let mut stale_idle = running.clone();
        stale_idle.status = MemberRunStatus::Idle;
        assert!(store
            .compare_and_append_member_run(&running, &stale_idle)
            .is_err());
        let rebased = refresh_member_after_provider_callbacks(&ledger, &running)
            .expect("terminal driver rebases after callbacks");
        let mut idle = rebased.clone();
        idle.status = MemberRunStatus::Idle;
        store
            .compare_and_append_member_run(&rebased, &idle)
            .expect("terminal rebase writes Idle from latest");
        running = idle.clone();
        running.status = MemberRunStatus::Running;
        store
            .compare_and_append_member_run(&idle, &running)
            .expect("start next provider round");

        // Inject an exact CAS loss after the helper's read. A same-generation
        // rename is safe drift: retry must preserve it and still enter Waiting.
        let mut injected = false;
        let waiting = match transition_provider_interaction_member_with_hook(
            &ledger,
            &running,
            MemberRunStatus::Waiting,
            |attempt, observed| {
                if attempt == 0 && !injected {
                    injected = true;
                    let mut renamed = observed.clone();
                    renamed.name = "RenamedDuringCallback".into();
                    store_conflict_as_usage(
                        store.compare_and_append_member_run(observed, &renamed),
                    )?;
                }
                Ok(())
            },
        )
        .expect("bounded retry converges")
        {
            ProviderInteractionMemberTransition::Applied(member) => member,
            ProviderInteractionMemberTransition::LifecycleSuperseded => {
                panic!("rename is not lifecycle supersession")
            }
        };
        assert_eq!(waiting.name, "RenamedDuringCallback");
        assert_eq!(waiting.status, MemberRunStatus::Waiting);

        // Close wins while the provider is waiting. A delayed resolution gets
        // a cancellation outcome and must not write Running over Stopped.
        let mut closed = waiting.clone();
        closed.coordination_status = MemberCoordinationStatus::Closed;
        closed.status = MemberRunStatus::Stopped;
        closed.finished_at = Some("unix-ms:closed".into());
        store
            .compare_and_append_member_run(&waiting, &closed)
            .expect("close waiting member");
        let superseded =
            transition_provider_interaction_member(&ledger, &waiting, MemberRunStatus::Running)
                .expect("late resolution is handled");
        assert!(matches!(
            superseded,
            ProviderInteractionMemberTransition::LifecycleSuperseded
        ));
        assert_eq!(
            ledger
                .latest_member_run(&closed.id)
                .unwrap()
                .unwrap()
                .status,
            MemberRunStatus::Stopped
        );

        // A reopened generation is different execution authority. The old
        // callback cannot resume it even when the stable ProviderRuntimeProjection id is reused.
        let mut reopened = closed.clone();
        reopened.coordination_status = MemberCoordinationStatus::Active;
        reopened.runtime_generation += 1;
        reopened.status = MemberRunStatus::Idle;
        reopened.finished_at = None;
        store
            .compare_and_advance_member_run_generation(&closed, &reopened)
            .expect("reopen next generation");
        assert!(transition_provider_interaction_member(
            &ledger,
            &waiting,
            MemberRunStatus::Running
        )
        .is_err());

        let second_initial = created.member_runs[1].clone();
        let mut second_running = second_initial.clone();
        second_running.status = MemberRunStatus::Running;
        store
            .compare_and_append_member_run(&second_initial, &second_running)
            .expect("seed second provider round");
        let second_waiting = match transition_provider_interaction_member(
            &ledger,
            &second_running,
            MemberRunStatus::Waiting,
        )
        .expect("second member waits")
        {
            ProviderInteractionMemberTransition::Applied(member) => member,
            ProviderInteractionMemberTransition::LifecycleSuperseded => unreachable!(),
        };
        let mut retired = second_waiting.clone();
        retired.coordination_status = MemberCoordinationStatus::Retired;
        retired.status = MemberRunStatus::Stopped;
        retired.finished_at = Some("unix-ms:retired".into());
        store
            .compare_and_append_member_run(&second_waiting, &retired)
            .expect("retire waiting member");
        assert!(matches!(
            transition_provider_interaction_member(
                &ledger,
                &second_waiting,
                MemberRunStatus::Running
            )
            .expect("late retired resolution is handled"),
            ProviderInteractionMemberTransition::LifecycleSuperseded
        ));
        let retired_latest = ledger.latest_member_run(&retired.id).unwrap().unwrap();
        assert_eq!(
            retired_latest.coordination_status,
            MemberCoordinationStatus::Retired
        );
        assert_eq!(retired_latest.status, MemberRunStatus::Stopped);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

