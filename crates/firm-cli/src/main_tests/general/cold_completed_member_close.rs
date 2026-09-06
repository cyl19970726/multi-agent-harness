use super::*;

fn cold_fixture(
    label: &str,
) -> (
    HarnessStore,
    PathBuf,
    TeamSupervisorLease,
    ProviderRuntimeProjection,
) {
    let (store, root) = temp_store(label);
    let created = create_two_member_team_run(&store);
    let extra = TeamMemberSpec {
        agent_member_id: "agent-cold-extra".into(),
        name: "Extra".into(),
        role: "reviewer".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec![],
        resume_native_session_id: None,
        initial_work: None,
    };
    let space = team_run_execution_space_id(&store, &created.team_run).unwrap();
    ensure_unit_test_canonical_members(
        &store,
        &space,
        &created.team_run.agent_team_id,
        std::slice::from_ref(&extra),
    )
    .unwrap();
    add_team_run_member(&store, None, &created.team_run.id, &extra, None).unwrap();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-cold-close",
            std::process::id(),
            "test://cold-close",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).unwrap();
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members: latest_member_runs_in_append_order(&store).unwrap(),
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind driver without provider start");
    let mut completed = run.clone();
    completed.status = TeamRunStatus::Completed;
    completed.completed_at = Some(now_string());
    completed.updated_at = now_string();
    store
        .compare_and_append_team_run_lifecycle(&run, &completed)
        .expect("complete run");
    let member = created.member_runs[0].clone();
    (store, root, lease, member)
}

#[test]
fn completed_never_bound_cold_member_closes_without_fabricating_provider_receipt() {
    let (store, root, lease, member) = cold_fixture("cold-completed-close");
    let before = store.runtime_commands(&lease.execution_space_id).unwrap();
    let report = crate::completed_run_members::close_completed_run_member_coordination(
        &store,
        &member.team_run_id,
        &member,
        &lease,
        "host",
        "completed never-started lane",
    )
    .expect("Close")
    .expect("coordination path");
    assert_eq!(report["coordination_status"], "closed");
    assert_eq!(report["runtime_effect"], "never_started");
    assert_eq!(report["provider_close_receipt"], "not_fabricated");
    assert_eq!(
        store.runtime_commands(&lease.execution_space_id).unwrap(),
        before
    );
    let closed = latest_member_runs_in_append_order(&store)
        .unwrap()
        .into_iter()
        .find(|m| m.id == member.id)
        .unwrap();
    assert!(closed.native_session.is_none());
    assert_eq!(closed.runtime_generation, member.runtime_generation);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cold_close_rechecks_session_at_terminal_cas() {
    use harness_core::agentfirm_api::AgentSessionStatus;
    let (store, root, lease, member) = cold_fixture("cold-close-session-race");
    let ledger = TeamRunLedger::new(
        &store,
        &member.team_run_id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let error = close_detached_blocked_member_for_recovery_with_hooks(
        &store,
        &member.team_run_id,
        &member,
        &lease,
        "host",
        "race",
        DetachedRecoveryCloseMode::CompletedRunMember,
        |_| {
            transition_provider_session_for_member(&ledger, &member, AgentSessionStatus::Idle)?;
            Ok(())
        },
        |_| Ok(()),
    )
    .expect_err("changed Session refuses final CAS")
    .to_string();
    assert!(
        error.contains("DETACHED_MEMBER_RECOVERY_SESSION_CHANGED"),
        "{error}"
    );
    let latest = latest_member_runs_in_append_order(&store)
        .unwrap()
        .into_iter()
        .find(|m| m.id == member.id)
        .unwrap();
    assert!(latest.coordination_is_active());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn completed_recover_keeps_three_closed_members_closed_and_names_remaining_lane() {
    let (store, root, lease, member) = cold_fixture("recover-preserves-closed-roster");
    let mut closed = Vec::new();
    for original in latest_member_runs_in_append_order(&store).unwrap() {
        if original.id == member.id {
            continue;
        }
        let mut next = original.clone();
        next.coordination_status = MemberCoordinationStatus::Closed;
        next.status = MemberRunStatus::Stopped;
        next.finished_at = Some(now_string());
        next.last_event_at = Some(now_string());
        store
            .compare_and_append_member_run(&original, &next)
            .unwrap();
        closed.push(next);
    }
    assert_eq!(closed.len(), 3);
    let mut blocked = member.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some(now_string());
    store
        .compare_and_append_member_run(&member, &blocked)
        .unwrap();
    let ledger = TeamRunLedger::new(
        &store,
        &member.team_run_id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    // RecoveryRequired on a detached empty lane is the same DEV232 boundary;
    // this test's separate Cold fixture covers never-bound Close itself.
    transition_provider_session_for_member(
        &ledger,
        &blocked,
        harness_core::agentfirm_api::AgentSessionStatus::RecoveryRequired,
    )
    .unwrap();
    let result =
        team_run_recover(&store, &member.team_run_id, true).expect("recover remaining member");
    assert_eq!(result["reopened"], 0);
    assert_eq!(
        result["closed_members_not_restarted"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    for original in closed {
        let latest = latest_member_runs_in_append_order(&store)
            .unwrap()
            .into_iter()
            .find(|m| m.id == original.id)
            .unwrap();
        assert_eq!(
            latest, original,
            "recovery must not even refresh a Closed member profile"
        );
    }
    assert!(
        result["restarted_blocked_members"] == 1
            || !result["blocked_lanes_not_proven"]
                .as_array()
                .unwrap()
                .is_empty(),
        "{result}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cold_close_refuses_any_command_history_without_new_side_effects() {
    for state in ["preparing", "unknown", "settled"] {
        let (store, root, lease, member) = cold_fixture(&format!("cold-close-{state}-history"));
        let ledger = TeamRunLedger::new(
            &store,
            &member.team_run_id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let effect = prepare_provider_effect_kind(
            &ledger,
            &member,
            "cold-history",
            "test recorded release",
            harness_core::agentfirm_api::RuntimeCommandKind::ReleaseRuntime,
            "runtime.release",
            None,
        )
        .expect("durable command");
        if state != "preparing" {
            settle_provider_effect(
                &ledger,
                &effect,
                if state == "settled" {
                    ProviderEffectSettlement::APPLIED_SATISFIED
                } else {
                    ProviderEffectSettlement::UNPROVEN
                },
                Some(serde_json::json!({"fixture":state})),
                None,
            )
            .expect("settle history");
        }
        let before = durable_store_file_bytes(&store);
        let error = crate::completed_run_members::close_completed_run_member_coordination(
            &store,
            &member.team_run_id,
            &member,
            &lease,
            "host",
            "no invented receipt",
        )
        .expect_err("command history refuses never-started exception")
        .to_string();
        assert!(error.contains("DETACHED_MEMBER_RECOVERY_"), "{error}");
        assert_eq!(
            durable_store_file_bytes(&store),
            before,
            "refusal cannot create Close intent or alter member/session"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn cold_close_rechecks_new_command_under_terminal_cas_lock() {
    let (store, root, lease, member) = cold_fixture("cold-close-command-race");
    let ledger = TeamRunLedger::new(
        &store,
        &member.team_run_id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let error = close_detached_blocked_member_for_recovery_with_hooks(
        &store,
        &member.team_run_id,
        &member,
        &lease,
        "host",
        "race",
        DetachedRecoveryCloseMode::CompletedRunMember,
        |_| {
            prepare_provider_effect_kind(
                &ledger,
                &member,
                "cold-racing-command",
                "test concurrent release",
                harness_core::agentfirm_api::RuntimeCommandKind::ReleaseRuntime,
                "runtime.release",
                None,
            )?;
            Ok(())
        },
        |_| Ok(()),
    )
    .expect_err("new command refuses final CAS")
    .to_string();
    assert!(
        error.contains("DETACHED_MEMBER_RECOVERY_COLD_COMMAND_HISTORY"),
        "{error}"
    );
    let latest = latest_member_runs_in_append_order(&store)
        .unwrap()
        .into_iter()
        .find(|m| m.id == member.id)
        .unwrap();
    assert!(latest.coordination_is_active());
    std::fs::remove_dir_all(root).unwrap();
}
