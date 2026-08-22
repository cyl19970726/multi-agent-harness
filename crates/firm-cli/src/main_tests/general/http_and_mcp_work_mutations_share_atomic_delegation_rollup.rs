#[cfg(any())]
#[test]
fn http_and_mcp_work_mutations_share_atomic_delegation_rollup() {
    let (store, root) = temp_store("delegation-surface-rollup");
    let source_run = create_two_member_team_run(&store);
    let (project_context, _) =
        ensure_legacy_unit_test_team_binding(&store).expect("unit-test project binding");
    store
        .insert_mission(&Mission {
            id: "surface-target-mission".into(),
            title: "Surface target Mission".into(),
            objective: "Prove every public Work surface shares delegation roll-up".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Running,
            legacy_wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        })
        .expect("insert target Mission");
    store
        .insert_agent_team_with_unique_mission(&AgentTeam {
            id: "surface-target-team".into(),
            name: "Surface target Team".into(),
            description: "Independent target Team for delegation surface tests".into(),
            mission_id: "surface-target-mission".into(),
            host_agent_id: "host".into(),
            node_id: "00000000-0000-4000-8000-000000000001".into(),
            status: AgentTeamStatus::Active,
            member_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert target AgentTeam");
    let target_member = TeamMemberSpec {
        agent_member_id: "agent-target-builder".into(),
        name: "TargetBuilder".into(),
        role: "builder".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec!["crates/target".into()],
        resume_native_session_id: None,
        initial_work: None,
    };
    ensure_unit_test_canonical_members(
        &store,
        "unit-test-space",
        "surface-target-team",
        std::slice::from_ref(&target_member),
    )
    .expect("register target canonical AgentMember");
    let target_run = create_team_run(
        &store,
        Some(&project_context),
        Some("unit-test-space"),
        None,
        "Execute the delegated surface outcome",
        None,
        "test",
        None,
        HostControlMode::Managed,
        None,
        Some("surface-target-team".into()),
        None,
        None,
        &[target_member],
    )
    .expect("create target TeamRun");
    let source_value = create_team_work_value(
        &store,
        &source_run.team_run.id,
        &serde_json::json!({
            "id": "surface-source-work",
            "title": "Delegate one surface-owned outcome",
            "completion_criteria_markdown": "Target Team reports the result"
        }),
    )
    .expect("create source Work");
    let source: Work = serde_json::from_value(source_value).expect("decode source Work");
    let source_member_id = source_run.member_runs[0].id.clone();
    let source = store
        .claim_work(
            &source.id,
            source.version,
            &source_member_id,
            WorkCommandContext {
                event_id: "surface-source-claim".into(),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::ProviderRuntimeProjection,
                    id: source_member_id.clone(),
                    display_name: None,
                    authn_source: Some("bound-runtime:test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "surface-source-claim-command".into(),
                created_at: "unix-ms:3".into(),
                duplicate_ok: false,
            },
        )
        .expect("claim source Work with a durable owner");
    let create_request = serde_json::json!({
        "source_team_run_id": source_run.team_run.id,
        "source_work_id": source.id,
        "expected_version": source.version,
        "target_agent_team_id": target_run.team_run.agent_team_id,
        "target_title": "Execute delegated surface outcome",
        "target_completion_criteria_markdown": "HTTP and MCP lifecycle changes roll up",
        "event_id": "surface-delegation-create",
        "idempotency_key": "surface-delegation-create-command"
    });
    let created = create_work_delegation_value(&store, &create_request)
        .expect("create cross-Team delegation");
    let retried = create_work_delegation_value(&store, &create_request)
        .expect("omitted entity ids are stable across an exact retry");
    assert_eq!(retried, created);
    let delegation_id = created["delegation"]["id"]
        .as_str()
        .expect("generated delegation id")
        .to_string();
    let target: Work =
        serde_json::from_value(created["target_work"].clone()).expect("decode target Work");
    let target_member_id = target_run.member_runs[0].id.clone();
    let member_context = |event_id: &str, idempotency_key: &str| WorkCommandContext {
        event_id: event_id.into(),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: target_member_id.clone(),
            display_name: None,
            authn_source: Some("bound-runtime:test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: idempotency_key.into(),
        created_at: "unix-ms:4".into(),
        duplicate_ok: false,
    };
    let mut target = store
        .claim_work(
            &target.id,
            target.version,
            &target_member_id,
            member_context("surface-target-claim", "surface-target-claim-command"),
        )
        .expect("claim target Work");

    let blocked = mutate_team_work_value(
        &store,
        &target.team_run_id,
        &target.id,
        "block",
        &serde_json::json!({
            "expected_version": target.version,
            "reason": "HTTP-visible dependency",
            "event_id": "surface-http-block",
            "idempotency_key": "surface-http-block-command"
        }),
    )
    .expect("HTTP shared mutation blocks target Work");
    target = serde_json::from_value(blocked).expect("decode blocked target Work");
    let delegation = store
        .latest_work_delegations()
        .expect("delegations after HTTP block")
        .into_iter()
        .find(|delegation| delegation.id == delegation_id)
        .expect("surface delegation");
    assert_eq!(delegation.state, WorkDelegationState::Blocked);

    let resumed = mutate_team_work_value(
        &store,
        &target.team_run_id,
        &target.id,
        "resume",
        &serde_json::json!({
            "expected_version": target.version,
            "resolution": "HTTP dependency resolved",
            "event_id": "surface-http-resume",
            "idempotency_key": "surface-http-resume-command"
        }),
    )
    .expect("HTTP shared mutation resumes target Work");
    target = serde_json::from_value(resumed).expect("decode resumed target Work");

    let resolved = ResolvedStore {
        root: root.clone(),
        source: StoreSource::StoreFlag,
        project_selection_explicit: false,
        context: None,
        execution_space_context: None,
    };
    let response = mcp::call_tool(
        &store,
        &resolved,
        &serde_json::json!({
            "name": "team_run_work_block",
            "arguments": {
                "team_run_id": target.team_run_id,
                "work_id": target.id,
                "expected_version": target.version,
                "reason": "MCP-visible dependency",
                "event_id": "surface-mcp-block",
                "idempotency_key": "surface-mcp-block-command"
            }
        }),
    )
    .expect("dispatch MCP Work block");
    assert_eq!(response["isError"], false, "MCP response: {response}");
    let delegation = store
        .latest_work_delegations()
        .expect("delegations after MCP block")
        .into_iter()
        .find(|delegation| delegation.id == delegation_id)
        .expect("surface delegation");
    assert_eq!(delegation.state, WorkDelegationState::Blocked);
    assert_eq!(
        delegation.blocker_reason.as_deref(),
        Some("MCP-visible dependency")
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
