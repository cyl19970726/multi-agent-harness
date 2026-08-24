use super::*;

#[test]
fn member_role_action_capability_binds_sender_to_live_supervisor_identity() {
    let (store, root) = temp_store("member-role-action-capability");
    let created = create_two_member_team_run(&store);
    let first = created.member_runs[0].clone();
    let second = created.member_runs[1].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-member-role-action",
            std::process::id(),
            "test://member-role-action",
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
        &first,
        harness_core::agentfirm_api::AgentSessionStatus::Active,
    )
    .expect("activate exact sender session");

    let first_token = "a".repeat(64);
    let second_token = "b".repeat(64);
    let first_capability = test_collaboration_capability(&store, &lease, &first, &first_token);
    let second_capability = test_collaboration_capability(&store, &lease, &second, &second_token);
    let (first_control, _first_registration) =
        register_live_member_control(&first, &first_capability, 1);
    let (second_control, _second_registration) =
        register_live_member_control(&second, &second_capability, 1);
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());
    author_test_canonical_message(
        &store,
        &created,
        &lease,
        &lease.execution_space_id,
        "bound-member-private-inbox",
        &second.agent_member_id,
        &first.agent_member_id,
        harness_core::agentfirm_api::MessageKind::Message,
        "Private exact-self inbox message",
        "bound-member-private-inbox-thread",
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );
    let forged_inbox = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::ReadInbox {
            team_run_id: created.team_run.id.clone(),
            member_run_id: second.id.clone(),
            capability_token: first_token.clone(),
            include_all: true,
        },
    )
    .expect_err("one member capability cannot read its sibling inbox");
    assert!(forged_inbox.to_string().contains("UNAUTHORIZED_ACTOR"));
    let own_inbox = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::ReadInbox {
            team_run_id: created.team_run.id.clone(),
            member_run_id: first.id.clone(),
            capability_token: first_token.clone(),
            include_all: true,
        },
    )
    .expect("exact live capability reads only its own Inbox");
    let own_inbox =
        serde_json::from_value::<Vec<TeamMessageProjection>>(own_inbox).expect("Inbox rows");
    assert_eq!(own_inbox.len(), 1);
    assert_eq!(own_inbox[0].body, "Private exact-self inbox message");
    let context = WorkCommandContext {
        event_id: "member-capability-work-created".into(),
        performed_by_actor: created
            .team_run
            .host_actor
            .clone()
            .expect("exact TeamRun Host"),
        authority_actor: None,
        causation_ref: None,
        idempotency_key: "member-capability-work-created".into(),
        created_at: now_string(),
        duplicate_ok: false,
    };
    let work = harness_application::WorkApplication::new(&store)
        .create(harness_application::CreateWorkCommand {
            work_id: "member-capability-work".into(),
            team_run_id: created.team_run.id.clone(),
            accountable_team_id: created.team_run.agent_team_id.clone(),
            title: "Prove bound member Role Action authority".into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "Only the exact live member can start it".into(),
            claim_mode: WorkClaimMode::HostAssign,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            initial_member_run_id: Some(first.id.clone()),
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context,
        })
        .expect("create assigned Work");
    let route = format!(
        "/v1/agentfirm/team-runs/{}/works/{}/start",
        created.team_run.id, work.id
    );
    let start_body = serde_json::json!({"action": "start_work"});
    let before_forgery = durable_store_file_bytes(&store);
    let forged = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::RoleAction {
            team_run_id: created.team_run.id.clone(),
            member_run_id: second.id.clone(),
            capability_token: first_token.clone(),
            path: route.clone(),
            expected_version: work.version,
            idempotency_key: "forged-sibling-work-start".into(),
            body: start_body.clone(),
            confirmed_action: None,
        },
    )
    .expect_err("one member capability cannot select its sibling identity");
    assert!(forged.to_string().contains("UNAUTHORIZED_ACTOR"));
    assert_eq!(
        durable_store_file_bytes(&store),
        before_forgery,
        "rejected identity forgery must have byte-zero durable side effects"
    );

    dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::RoleAction {
            team_run_id: created.team_run.id.clone(),
            member_run_id: first.id.clone(),
            capability_token: first_token.clone(),
            path: route,
            expected_version: work.version,
            idempotency_key: "valid-member-work-start".into(),
            body: start_body,
            confirmed_action: None,
        },
    )
    .expect("exact live capability performs canonical Work Role Action");
    let started = store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|candidate| candidate.id == work.id)
        .expect("started Work");
    assert_eq!(started.phase, WorkPhase::Active);
    assert_eq!(
        started.active_member_run_id.as_deref(),
        Some(first.id.as_str())
    );
    let registered_control = LIVE_MEMBER_CONTROLS
        .get()
        .expect("live registry")
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&first.id)
        .cloned()
        .expect("registered exact member control");
    assert_ne!(registered_control.capability_fingerprint, first_token);
    assert_eq!(
        registered_control.capability_fingerprint,
        first_capability.fingerprint
    );
    assert!(!format!("{first_capability:?}").contains(&first_token));

    let active_daemon = store
        .latest_node_daemon_lease(&created.team_run.execution_node_id)
        .expect("read NodeDaemon lease")
        .expect("active NodeDaemon lease");
    store
        .release_node_daemon_lease(
            &active_daemon.node_id,
            &active_daemon.daemon_id,
            active_daemon.generation,
            &active_daemon.instance_id,
            current_unix_ms_u64(),
        )
        .expect("release original NodeDaemon lease");
    store
        .acquire_node_daemon_lease(
            &active_daemon.node_id,
            "successor-node-daemon",
            "successor-node-daemon-instance",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire successor NodeDaemon lease");
    let before_stale_daemon = durable_store_file_bytes(&store);
    let stale_daemon = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::ReadInbox {
            team_run_id: created.team_run.id.clone(),
            member_run_id: first.id.clone(),
            capability_token: first_token.clone(),
            include_all: true,
        },
    )
    .expect_err("capability cannot cross a successor NodeDaemon lease");
    assert!(stale_daemon
        .to_string()
        .contains("NODE_DAEMON_GENERATION_FENCED"));
    assert_eq!(
        durable_store_file_bytes(&store),
        before_stale_daemon,
        "stale NodeDaemon capability must have byte-zero durable side effects"
    );

    let current_first = latest_member_runs_in_append_order(&store)
        .expect("read current MemberRun")
        .into_iter()
        .find(|member| member.id == first.id)
        .expect("current exact MemberRun");
    let mut successor_member = current_first.clone();
    successor_member.runtime_generation += 1;
    successor_member.last_event_at = Some(now_string());
    store
        .compare_and_advance_member_run_generation(&current_first, &successor_member)
        .expect("advance MemberRun generation");
    let before_stale_member = durable_store_file_bytes(&store);
    let stale_member = dispatch_local_live_member_control(
        &store,
        &lease.supervisor_id,
        lease.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::ReadInbox {
            team_run_id: created.team_run.id.clone(),
            member_run_id: first.id.clone(),
            capability_token: first_token.clone(),
            include_all: true,
        },
    )
    .expect_err("capability cannot cross a successor MemberRun generation");
    assert!(stale_member
        .to_string()
        .contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(
        durable_store_file_bytes(&store),
        before_stale_member,
        "stale MemberRun capability must have byte-zero durable side effects"
    );
    store
        .release_team_supervisor_lease(
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms_u64(),
        )
        .expect("release capability-signing Supervisor");
    let successor = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "successor-supervisor",
            std::process::id(),
            "test://successor-role-action",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire successor Supervisor");
    let before_stale_supervisor = durable_store_file_bytes(&store);
    let stale_supervisor = dispatch_local_live_member_control(
        &store,
        &successor.supervisor_id,
        successor.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::ReadInbox {
            team_run_id: created.team_run.id.clone(),
            member_run_id: first.id.clone(),
            capability_token: first_token,
            include_all: true,
        },
    )
    .expect_err("capability cannot cross its frozen Supervisor generation");
    assert!(stale_supervisor
        .to_string()
        .contains("stale runtime binding"));
    assert_eq!(
        durable_store_file_bytes(&store),
        before_stale_supervisor,
        "stale Supervisor capability must have byte-zero durable side effects"
    );
    assert!(matches!(
        first_control.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert!(matches!(
        second_control.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    std::fs::remove_dir_all(root).expect("cleanup");
}
