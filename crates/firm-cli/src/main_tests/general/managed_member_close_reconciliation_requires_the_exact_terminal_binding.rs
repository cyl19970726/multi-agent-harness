use super::*;

#[cfg(unix)]
#[test]
fn managed_member_close_reconciliation_requires_the_exact_terminal_binding() {
    use harness_core::agentfirm_api::{
        AgentSessionStatus, RuntimeActivity, RuntimeCommandKind, RuntimeResidency,
    };

    #[derive(Clone, Copy, Debug)]
    enum Case {
        Exact,
        MissingCommand,
        WrongCommand,
        WrongMemberGeneration,
        WrongNativeSession,
        WrongDriver,
        WrongCloseRequest,
        AttachedResidency,
    }

    fn observe(case: Case) -> bool {
        let (store, root) = temp_store(&format!("managed-close-{case:?}"));
        let created = create_two_member_team_run(&store);
        let initial = created.member_runs[0].clone();
        let mut bound = initial.clone();
        bound.native_session = Some(capacity_test_session());
        bound.last_event_at = Some("unix-ms:close-bound".into());
        store
            .compare_and_append_member_run(&initial, &bound)
            .expect("bind close fixture native session");

        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                &format!("managed-close-{case:?}"),
                std::process::id(),
                "test://managed-close",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire close fixture Supervisor");
        ensure_test_runtime_fabric(&store, &created, &lease);
        if !matches!(case, Case::WrongDriver) {
            let run =
                latest_team_run(&store, &created.team_run.id).expect("read close fixture TeamRun");
            let members = latest_member_runs_in_append_order(&store)
                .expect("read close fixture members")
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
            .expect("bind close fixture TeamSupervisor driver");
        }
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
            .expect("make close fixture AgentSession idle");

        let close = latch_member_close(
            &store,
            &created.team_run.id,
            &bound.id,
            "host",
            "deterministic managed Close fixture",
        )
        .expect("latch close fixture");

        if !matches!(case, Case::MissingCommand) {
            let command = if matches!(case, Case::WrongCommand) {
                RuntimeCommandKind::ReleaseRuntime
            } else {
                RuntimeCommandKind::CloseMember
            };
            let source_record_id = if matches!(case, Case::WrongCloseRequest) {
                "different-close-request:idle:close-runtime".to_string()
            } else {
                format!("{}:idle:close-runtime", close.id)
            };
            let effect = prepare_provider_effect_kind(
                &ledger,
                &bound,
                &source_record_id,
                "deterministic close reconciliation fixture",
                command,
                if command == RuntimeCommandKind::CloseMember {
                    "member.close"
                } else {
                    "runtime.release"
                },
                None,
            )
            .expect("prepare close fixture RuntimeCommand");
            settle_provider_effect(
                &ledger,
                &effect,
                true,
                Some(serde_json::json!({"fixture": format!("{case:?}")})),
                None,
            )
            .expect("settle close fixture RuntimeCommand");
        }
        if matches!(case, Case::AttachedResidency) {
            transition_provider_session_runtime_control(
                &ledger,
                &bound,
                RuntimeResidency::Attached,
                RuntimeActivity::Idle,
            )
            .expect("leave close fixture runtime attached");
        }

        let mut stopped = bound.clone();
        stopped.coordination_status = MemberCoordinationStatus::Closed;
        stopped.status = MemberRunStatus::Stopped;
        stopped.finished_at = Some("unix-ms:close-stopped".into());
        stopped.last_event_at = Some("unix-ms:close-stopped".into());
        store
            .compare_and_append_member_run(&bound, &stopped)
            .expect("project close fixture stopped");
        store
            .complete_team_member_close(
                &created.team_run.id,
                &bound.id,
                &close.id,
                "unix-ms:close-applied",
            )
            .expect("apply close fixture");

        let mut supplied = bound;
        if matches!(case, Case::WrongMemberGeneration) {
            supplied.runtime_generation += 1;
        }
        if matches!(case, Case::WrongNativeSession) {
            supplied
                .native_session
                .as_mut()
                .expect("bound fixture native session")
                .native_session_id = "different-native-session".into();
        }
        let observed = managed_member_runtime_close_is_settled(&store, &supplied)
            .expect("evaluate durable managed Close postcondition");
        std::fs::remove_dir_all(root).expect("cleanup close fixture");
        observed
    }

    assert!(observe(Case::Exact));
    let rejected = [
        Case::MissingCommand,
        Case::WrongCommand,
        Case::WrongMemberGeneration,
        Case::WrongNativeSession,
        Case::WrongDriver,
        Case::WrongCloseRequest,
        Case::AttachedResidency,
    ]
    .map(|case| (case, observe(case)));
    assert!(
        rejected.iter().all(|(_, settled)| !settled),
        "non-exact Close facts were accepted: {rejected:?}"
    );
}
