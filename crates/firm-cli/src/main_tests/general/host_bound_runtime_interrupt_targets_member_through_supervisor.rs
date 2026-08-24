use super::*;

#[test]
fn host_bound_runtime_interrupt_targets_member_through_supervisor() {
    let (store, root) = temp_store("host-bound-runtime-interrupt");
    let created = create_two_member_team_run(&store);
    let host = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "host")
        .expect("exact managed Host")
        .clone();
    let target = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "agent-builder-a")
        .expect("target Member")
        .clone();
    let sibling = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "agent-builder-b")
        .expect("sibling Member")
        .clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-host-runtime-interrupt",
            std::process::id(),
            "test://host-runtime-interrupt",
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
    transition_provider_session_for_member(
        &ledger,
        &host,
        harness_core::agentfirm_api::AgentSessionStatus::Active,
    )
    .expect("activate Host session");
    transition_provider_session_for_member(
        &ledger,
        &target,
        harness_core::agentfirm_api::AgentSessionStatus::Active,
    )
    .expect("activate target session");
    transition_provider_session_for_member(
        &ledger,
        &sibling,
        harness_core::agentfirm_api::AgentSessionStatus::Active,
    )
    .expect("activate sibling session");
    let mut running_target = target.clone();
    let mut target_profile = team_member_provider_profile("codex");
    apply_provider_version(&mut target_profile, Some("0.148.0-alpha.9".into()));
    running_target.provider_profile = Some(target_profile);
    running_target.status = MemberRunStatus::Running;
    running_target.last_event_at = Some(now_string());
    ledger
        .save_member_run(&target, &running_target)
        .expect("project target running turn");

    let host_token = "a".repeat(64);
    let target_token = "b".repeat(64);
    let sibling_token = "c".repeat(64);
    let host_capability = test_collaboration_capability(&store, &lease, &host, &host_token);
    let target_capability =
        test_collaboration_capability(&store, &lease, &running_target, &target_token);
    let sibling_capability =
        test_collaboration_capability(&store, &lease, &sibling, &sibling_token);
    let (_host_receiver, _host_registration) =
        register_live_member_control(&host, &host_capability, 1);
    let (target_receiver, _target_registration) =
        register_live_member_control(&running_target, &target_capability, 1);
    let (_sibling_receiver, _sibling_registration) =
        register_live_member_control(&sibling, &sibling_capability, 1);
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());
    let space_id = store
        .trust_member_run_scope(&running_target.id)
        .expect("read target scope")
        .expect("target is canonical");
    let target_version = store
        .trust_member_runs(&space_id)
        .expect("read canonical MemberRuns")
        .into_iter()
        .find(|member| member.id == running_target.id)
        .expect("canonical target MemberRun")
        .version;
    let route = format!("/v1/agentfirm/member-runs/{}/interrupt", running_target.id);
    let body = serde_json::json!({
        "action": "interrupt_member_run",
        "reason": "prove exact Host-authorized DSH interruption",
    });

    let before_sibling = durable_store_file_bytes(&store);
    let sibling_attempt = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::RoleAction {
            team_run_id: created.team_run.id.clone(),
            member_run_id: sibling.id.clone(),
            capability_token: sibling_token,
            path: route.clone(),
            expected_version: target_version,
            idempotency_key: "sibling-cannot-interrupt".into(),
            body: body.clone(),
            confirmed_action: None,
        },
    )
    .expect_err("ordinary Member cannot interrupt its sibling");
    assert!(
        sibling_attempt.to_string().contains("UNAUTHORIZED_ACTOR"),
        "unexpected sibling rejection: {sibling_attempt}"
    );
    assert_eq!(
        durable_store_file_bytes(&store),
        before_sibling,
        "rejected sibling Interrupt has byte-zero durable side effects"
    );

    let target_control = std::thread::spawn(move || {
        let command = target_receiver.recv().expect("receive exact Interrupt");
        let MemberControlCommand::Interrupt {
            reason,
            requested_by,
            reply,
        } = command
        else {
            panic!("Host Role Action must route one Interrupt command");
        };
        assert_eq!(reason, "prove exact Host-authorized DSH interruption");
        assert_eq!(requested_by, "host");
        reply
            .send(Ok(serde_json::json!({
                "provider_receipt_id": "deepseek-interrupt:test",
                "native_session_id": "star-test-session",
            })))
            .expect("settle test provider Interrupt");
    });
    let result = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::RoleAction {
            team_run_id: created.team_run.id.clone(),
            member_run_id: host.id.clone(),
            capability_token: host_token,
            path: route,
            expected_version: target_version,
            idempotency_key: "host-interrupts-member".into(),
            body,
            confirmed_action: None,
        },
    )
    .expect("exact bound Host interrupts target Member through Supervisor");
    assert_eq!(result["provider_receipt_id"], "deepseek-interrupt:test");
    assert_eq!(result["native_session_id"], "star-test-session");
    target_control.join().expect("target control receiver");
    std::fs::remove_dir_all(root).expect("cleanup");
}
