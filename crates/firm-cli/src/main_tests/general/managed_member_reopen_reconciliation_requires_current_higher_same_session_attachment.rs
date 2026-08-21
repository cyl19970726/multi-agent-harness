use super::*;

#[cfg(unix)]
#[test]
fn managed_member_reopen_reconciliation_requires_current_higher_same_session_attachment() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};

    #[derive(Clone, Copy, Debug)]
    enum Case {
        Exact,
        StaleGeneration,
        WrongNativeSession,
        WrongDriver,
        StaleSupervisorGeneration,
        DetachedResidency,
    }

    fn observe(case: Case) -> Option<ProviderRuntimeProjection> {
        let (store, root) = temp_store(&format!("managed-reopen-{case:?}"));
        let created = create_two_member_team_run(&store);
        let initial = created.member_runs[0].clone();
        let mut bound = initial.clone();
        bound.native_session = Some(capacity_test_session());
        bound.last_event_at = Some("unix-ms:reopen-bound".into());
        store
            .compare_and_append_member_run(&initial, &bound)
            .expect("bind reopen fixture native session");

        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                &format!("managed-reopen-{case:?}"),
                std::process::id(),
                "test://managed-reopen",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire reopen fixture Supervisor");
        ensure_test_runtime_fabric(&store, &created, &lease);
        if !matches!(case, Case::WrongDriver) {
            let run =
                latest_team_run(&store, &created.team_run.id).expect("read reopen fixture TeamRun");
            let members = latest_member_runs_in_append_order(&store)
                .expect("read reopen fixture members")
                .into_iter()
                .filter(|member| member.team_run_id == run.id)
                .collect();
            let body = PreparedTeamRunBody {
                run_id: run.id.clone(),
                objective: run.objective.clone(),
                run,
                members,
            };
            bind_team_runtime_supervisor(
                &store,
                &body,
                &lease.execution_space_id,
                &lease.node_daemon_id,
                &lease.supervisor_id,
                lease.generation,
            )
            .expect("bind reopen fixture TeamSupervisor driver");
        }
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
            .expect("make reopen fixture AgentSession idle");
        if !matches!(case, Case::DetachedResidency) {
            transition_provider_session_runtime_control(
                &ledger,
                &bound,
                RuntimeResidency::Attached,
                RuntimeActivity::Idle,
            )
            .expect("attach reopen fixture runtime");
        }
        if matches!(case, Case::StaleSupervisorGeneration) {
            let now = current_unix_ms_u64();
            store
                .release_team_supervisor_lease(
                    &created.team_run.id,
                    &lease.supervisor_id,
                    lease.generation,
                    now,
                )
                .expect("release stale reopen fixture Supervisor");
            let replacement = store
                .acquire_test_supervisor_lease(
                    &created.team_run.id,
                    "managed-reopen-replacement",
                    std::process::id(),
                    "test://managed-reopen-replacement",
                    now + 1,
                    60_000,
                )
                .expect("acquire replacement reopen fixture Supervisor");
            assert!(replacement.generation > lease.generation);
        }

        let mut closed = bound.clone();
        closed.coordination_status = MemberCoordinationStatus::Closed;
        closed.status = MemberRunStatus::Stopped;
        closed.finished_at = Some("unix-ms:reopen-closed".into());
        closed.last_event_at = Some("unix-ms:reopen-closed".into());
        store
            .compare_and_append_member_run(&bound, &closed)
            .expect("close predecessor fixture generation");
        let mut reopened = closed.clone();
        reopened.runtime_generation += 1;
        reopened.coordination_status = MemberCoordinationStatus::Active;
        reopened.status = MemberRunStatus::Idle;
        reopened.started_at = "unix-ms:reopen-started".into();
        reopened.finished_at = None;
        reopened.last_event_at = Some("unix-ms:reopen-started".into());
        if matches!(case, Case::WrongNativeSession) {
            reopened
                .native_session
                .as_mut()
                .expect("bound fixture native session")
                .native_session_id = "replacement-native-session".into();
        }
        store
            .compare_and_advance_member_run_generation(&closed, &reopened)
            .expect("advance reopen fixture generation");

        let supplied = if matches!(case, Case::StaleGeneration) {
            &closed
        } else {
            &reopened
        };
        let observed = managed_member_runtime_reopen_is_settled(&store, supplied)
            .expect("evaluate durable managed Reopen postcondition");
        std::fs::remove_dir_all(root).expect("cleanup reopen fixture");
        observed
    }

    let exact = observe(Case::Exact).expect("exact Reopen must reconcile");
    assert_eq!(exact.runtime_generation, 2);
    let rejected = [
        Case::StaleGeneration,
        Case::WrongNativeSession,
        Case::WrongDriver,
        Case::StaleSupervisorGeneration,
        Case::DetachedResidency,
    ]
    .map(|case| (case, observe(case).is_some()));
    assert!(
        rejected.iter().all(|(_, settled)| !settled),
        "non-exact Reopen facts were accepted: {rejected:?}"
    );
}
