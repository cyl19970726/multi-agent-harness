use super::*;

#[test]
fn closed_host_and_peer_workspaces_keep_exact_history_with_offline_reader() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};
    let (store, _root) = temp_store("closed-host-workspace-history");
    let created = create_two_member_team_run(&store);
    let mut bound_members = Vec::new();
    for initial in &created.member_runs {
        let mut bound = initial.clone();
        let mut native = capacity_test_session();
        native.native_session_id = format!("history-{}", initial.id);
        bound.native_session = Some(native);
        store
            .compare_and_append_member_run(initial, &bound)
            .expect("bind native history");
        bound_members.push(bound);
    }
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "history-supervisor",
            std::process::id(),
            "test://history",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("test lease; no daemon process is started");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("TeamRun");
    bind_team_runtime_supervisor(
        &store,
        &PreparedTeamRunBody {
            run_id: run.id.clone(),
            objective: run.objective.clone(),
            run: run.clone(),
            members: bound_members.clone(),
        },
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind driver provenance");
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    for member in &mut bound_members {
        transition_provider_session_for_member(&ledger, member, AgentSessionStatus::Idle)
            .expect("idle");
        transition_provider_session_runtime_control(
            &ledger,
            member,
            RuntimeResidency::Attached,
            RuntimeActivity::Idle,
        )
        .expect("attached fixture");
        settle_provider_attempt_release(&ledger, member).expect("detached fixture");
        let mut idle = member.clone();
        idle.status = MemberRunStatus::Idle;
        ledger
            .save_member_run(member, &idle)
            .expect("idle MemberRun");
        *member = idle;
    }
    let mut completed = run.clone();
    completed.status = TeamRunStatus::Completed;
    completed.completed_at = Some(now_string());
    completed.updated_at = now_string();
    store
        .compare_and_append_team_run_lifecycle(&run, &completed)
        .expect("completed TeamRun");
    for member in &bound_members {
        crate::completed_run_members::close_completed_run_member_coordination(
            &store,
            &run.id,
            member,
            &lease,
            "host",
            "history read regression",
        )
        .expect("coordination close")
        .expect("closed");
    }
    assert!(
        store
            .host_runtime_binding(&run.id, current_unix_ms_u64())
            .is_err(),
        "closed Host must not acquire live control authority"
    );
    let identity = crate::role_views_api::ReadIdentity {
        actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Human,
            id: "test-reader".into(),
        },
        authority_actors: vec![],
        local_operator: true,
    };
    let before = durable_store_file_bytes(&store);
    for member in &bound_members {
        let view = crate::role_views_api::agent_workspace_view(
            &lease.execution_space_id,
            &store,
            &member.id,
            &crate::role_views_api::Query::default(),
            Some(&identity),
        )
        .expect("closed MemberRun RoleView");
        assert_eq!(
            view["data"]["selected_agent"]["current_member_run_ref"],
            member.id
        );
        assert_eq!(
            view["data"]["current_session"]["native_session_ref"]["native_session_id"],
            member.native_session.as_ref().unwrap().native_session_id,
            "{view}"
        );
        assert_eq!(
            view["data"]["persisted_session_projection"]["reason_code"],
            "node_daemon_read_unavailable",
            "a missing reader is not a missing identity: {view}"
        );
        assert_eq!(
            view["allowed_actions"],
            serde_json::json!([]),
            "read does not grant actions"
        );
    }
    assert_eq!(
        durable_store_file_bytes(&store),
        before,
        "history projection is read-only"
    );
}
