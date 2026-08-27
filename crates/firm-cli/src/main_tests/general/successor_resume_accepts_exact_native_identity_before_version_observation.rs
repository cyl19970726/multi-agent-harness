use super::*;

fn exact_codex_native_session(provider_version: Option<&str>) -> NativeSessionRef {
    NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "thread-successor-resume".into(),
        native_locator_kind: "codex_rollout".into(),
        provider_version: provider_version.map(str::to_string),
        adapter_contract_version: "codex-app-server-v1".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("unix-ms:99".into()),
        parent_native_session_id: None,
    }
}

#[test]
fn successor_resume_accepts_exact_native_identity_before_version_observation() {
    let (store, root) = temp_store("successor-resume-native-identity");
    let first = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &first.team_run.id,
            "supervisor-successor-resume",
            std::process::id(),
            "test://successor-resume",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire first Supervisor lease");
    ensure_test_runtime_fabric(&store, &first, &lease);

    let ledger = TeamRunLedger::new(
        &store,
        &first.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let expected = first.member_runs[0].clone();
    let mut settled = expected.clone();
    settled.native_session = Some(exact_codex_native_session(Some("0.148.0-alpha.9")));
    settled.status = MemberRunStatus::Idle;
    settled.last_event_at = Some("unix-ms:99".into());
    ledger
        .save_member_run(&expected, &settled)
        .expect("settle exact provider-native session");

    let project = ProjectContext {
        id: first.team_run.project_binding_id.clone(),
        project_root: root.clone(),
        store_root: store.root().to_path_buf(),
        kind: ProjectKind::Repo,
        is_git_repo: false,
    };
    let specs = first
        .member_runs
        .iter()
        .map(|member| TeamMemberSpec {
            agent_member_id: member.agent_member_id.clone(),
            name: member.name.clone(),
            role: member.role.clone(),
            provider: member.provider.clone(),
            execution_mode: member
                .provider_profile
                .as_ref()
                .map(|profile| profile.execution_mode.clone()),
            model: member.model.clone(),
            effort: None,
            service_tier: None,
            provider_cwd_hint: Some(root.to_string_lossy().into_owned()),
            owned_paths: member.owned_paths.clone(),
            resume_native_session_id: (member.id == settled.id)
                .then(|| "thread-successor-resume".into()),
            initial_work: None,
        })
        .collect::<Vec<_>>();
    let successor = create_team_run(
        &store,
        Some(&project),
        Some("unit-test-space"),
        Some(root.to_string_lossy().into_owned()),
        "Resume the standing AgentMember",
        None,
        "test",
        None,
        HostControlMode::Managed,
        Some(first.team_run.id.clone()),
        Some(first.team_run.agent_team_id.clone()),
        None,
        None,
        &specs,
    )
    .expect("create successor TeamRun");
    let resumed = successor
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == settled.agent_member_id)
        .expect("resumed MemberRun");
    assert_eq!(
        resumed
            .native_session
            .as_ref()
            .and_then(|session| session.provider_version.as_deref()),
        None,
        "TeamRun creation cannot claim a provider version before start preflight observes it"
    );

    let mut successor_members = successor.member_runs.clone();
    let successor_resume = successor_members
        .iter_mut()
        .find(|member| member.agent_member_id == settled.agent_member_id)
        .expect("successor resume member");
    successor_resume
        .provider_profile
        .as_mut()
        .expect("provider profile")
        .provider_version = Some("0.148.0-alpha.9".into());
    let expected_admission = expected_agentfirm_native_session_ref(successor_resume)
        .expect("preflight supplies the exact admission identity");
    assert_eq!(
        expected_admission.provider_version.as_deref(),
        Some("0.148.0-alpha.9")
    );
    let body = PreparedTeamRunBody {
        run_id: successor.team_run.id.clone(),
        objective: successor.team_run.objective.clone(),
        run: successor.team_run.clone(),
        members: successor_members,
    };
    ensure_team_runtime_fabric(
        &store,
        &body,
        "unit-test-space",
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )
    .expect("exact native identity resumes before provider-version observation is refreshed");

    let observed = agentfirm_native_session_ref(
        settled
            .native_session
            .as_ref()
            .expect("settled native session"),
    );
    assert!(agentfirm_native_session_identity_matches(
        Some(&observed),
        Some(&expected_admission)
    ));
    let mut wrong_native_id = expected_admission.clone();
    wrong_native_id.native_session_id = "thread-foreign".into();
    assert!(!agentfirm_native_session_identity_matches(
        Some(&observed),
        Some(&wrong_native_id)
    ));
    let mut wrong_contract = expected_admission.clone();
    wrong_contract.adapter_contract_version = "foreign-contract".into();
    assert!(!agentfirm_native_session_identity_matches(
        Some(&observed),
        Some(&wrong_contract)
    ));
    let mut wrong_version = expected_admission;
    wrong_version.provider_version = Some("0.149.0".into());
    assert!(!agentfirm_native_session_identity_matches(
        Some(&observed),
        Some(&wrong_version)
    ));

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn resumed_session_materialization_and_readmission_share_preflight_identity() {
    let (store, root) = temp_store("resume-materialization-native-identity");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-resume-materialization",
            std::process::id(),
            "test://resume-materialization",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");

    let mut members = created.member_runs.clone();
    let resumed = members
        .first_mut()
        .expect("managed member for resume materialization");
    resumed.native_session = Some(exact_codex_native_session(None));
    resumed
        .provider_profile
        .as_mut()
        .expect("provider profile")
        .provider_version = None;
    let mut body = PreparedTeamRunBody {
        run_id: created.team_run.id.clone(),
        objective: created.team_run.objective.clone(),
        run: created.team_run.clone(),
        members,
    };

    ensure_team_runtime_fabric(
        &store,
        &body,
        "unit-test-space",
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )
    .expect("message-path materialization accepts the versionless preflight identity");
    let precreated = store
        .fabric_agent_sessions("unit-test-space")
        .expect("read materialized AgentSessions")
        .into_iter()
        .find(|session| session.agent_member_id == body.members[0].agent_member_id)
        .expect("precreated resumed AgentSession");
    assert_eq!(
        precreated
            .native_session_ref
            .as_ref()
            .and_then(|session| session.provider_version.as_deref()),
        None
    );

    body.members[0]
        .provider_profile
        .as_mut()
        .expect("provider profile")
        .provider_version = Some("0.148.0-alpha.9".into());

    ensure_team_runtime_fabric(
        &store,
        &body,
        "unit-test-space",
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )
    .expect("start preflight enriches the exact precreated AgentSession");
    let enriched = store
        .fabric_agent_sessions("unit-test-space")
        .expect("read enriched AgentSessions")
        .into_iter()
        .find(|session| session.agent_member_id == body.members[0].agent_member_id)
        .expect("enriched resumed AgentSession");
    assert_eq!(
        enriched
            .native_session_ref
            .as_ref()
            .and_then(|session| session.provider_version.as_deref()),
        Some("0.148.0-alpha.9")
    );

    ensure_team_runtime_fabric(
        &store,
        &body,
        "unit-test-space",
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )
    .expect("readmission before provider settlement preserves enriched identity");

    let mut conflicting = body;
    conflicting.members[0]
        .provider_profile
        .as_mut()
        .expect("provider profile")
        .provider_version = Some("0.149.0".into());
    let error = ensure_team_runtime_fabric(
        &store,
        &conflicting,
        "unit-test-space",
        &lease.node_daemon_id,
        lease.node_daemon_generation,
    )
    .expect_err("an observed provider-version conflict remains fail-closed");
    assert!(error
        .to_string()
        .contains("AGENT_SESSION_RECOVERY_REQUIRED"));

    std::fs::remove_dir_all(root).expect("cleanup");
}
