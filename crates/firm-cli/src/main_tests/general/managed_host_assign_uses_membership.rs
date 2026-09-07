use super::*;

#[test]
fn managed_host_assign_uses_membership_through_the_authenticated_handler() {
    let (store, _root) = temp_store("managed-host-membership-assign");
    let created = create_two_member_team_run(&store);
    let host = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "host")
        .expect("managed Host");
    let worker = &created.member_runs[0];
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-managed-assign",
            std::process::id(),
            "test://managed-assign",
            current_unix_ms_u64(),
            60_000,
        )
        .unwrap();
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let host_token = "a".repeat(64);
    let worker_token = "b".repeat(64);
    let mut registrations = Vec::new();
    for (member, token) in [(host, &host_token), (worker, &worker_token)] {
        transition_provider_session_for_member(
            &ledger,
            member,
            harness_core::agentfirm_api::AgentSessionStatus::Active,
        )
        .unwrap();
        let capability = test_collaboration_capability(&store, &lease, member, token);
        registrations.push(register_live_member_control(member, &capability, 1));
    }
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());
    let act = |member: &ProviderRuntimeProjection,
               token: &str,
               suffix: &str,
               version: u64,
               key: &str,
               body: serde_json::Value| {
        dispatch_local_live_member_control(
            &store,
            &lease.supervisor_id,
            lease.generation,
            &supervisor_valid,
            &authority_gate,
            LiveMemberControlRequest::RoleAction {
                team_run_id: created.team_run.id.clone(),
                member_run_id: member.id.clone(),
                capability_token: token.into(),
                path: format!(
                    "/v1/agentfirm/team-runs/{}/works{suffix}",
                    created.team_run.id
                ),
                expected_version: version,
                idempotency_key: key.into(),
                body,
                confirmed_action: None,
            },
        )
    };
    act(
        host,
        &host_token,
        "",
        0,
        "create-managed-assignment",
        serde_json::json!({
            "action": "create_work", "work_id": "managed-assignment",
            "title": "Assign through the bound Host CLI",
            "completion_criteria_markdown": "exact membership owns the Work",
            "claim_mode": "host_assign"
        }),
    )
    .expect("authenticated managed Host creates Work");
    let membership = store
        .fabric_team_memberships(&lease.execution_space_id)
        .unwrap()
        .into_iter()
        .find(|membership| {
            membership.team_id == created.team_run.agent_team_id
                && membership.agent_member_id == worker.agent_member_id
        })
        .unwrap();
    let args = vec!["--membership-id".to_string(), membership.id.clone()];
    let intent = bound_member_work_assignment_intent(&args).unwrap();
    let suffix = "/managed-assignment/assign";
    let before = durable_store_file_bytes(&store);
    act(
        worker,
        &worker_token,
        suffix,
        1,
        "worker-denied",
        intent.clone(),
    )
    .expect_err("CLI payload does not grant an ordinary worker Host authority");
    act(
        host,
        &host_token,
        suffix,
        1,
        "old-wire-denied",
        serde_json::json!({"action":"assign_work", "member_run_id":worker.id}),
    )
    .expect_err("the old CLI wire shape cannot enter the canonical handler");
    assert_eq!(durable_store_file_bytes(&store), before);
    let assigned = act(host, &host_token, suffix, 1, "host-assign", intent.clone())
        .expect("the actual CLI intent passes the authenticated assignment handler");
    assert_eq!(
        assigned["projection"]["assignee_membership_id"],
        membership.id
    );
    assert_eq!(
        assigned["projection"]["owner_member_id"],
        worker.agent_member_id
    );
    assert_eq!(assigned["resulting_version"], 2);
    let after = durable_store_file_bytes(&store);
    let replay = act(host, &host_token, suffix, 1, "host-assign", intent.clone()).unwrap();
    assert_eq!(replay["replayed"], true);
    act(host, &host_token, suffix, 1, "stale-version", intent)
        .expect_err("assignment still enforces CAS");
    assert_eq!(durable_store_file_bytes(&store), after);
}

#[test]
fn managed_host_assign_refuses_runtime_selectors_instead_of_reinterpreting_them() {
    for tokens in [
        vec![],
        vec!["--member-run-id", "member-run-1"],
        vec!["--membership-id", "membership-1", "--member-run-id"],
    ] {
        let args = tokens.into_iter().map(str::to_string).collect::<Vec<_>>();
        let error = bound_member_work_assignment_intent(&args).unwrap_err();
        assert!(error.to_string().contains("--membership-id"));
    }
}
