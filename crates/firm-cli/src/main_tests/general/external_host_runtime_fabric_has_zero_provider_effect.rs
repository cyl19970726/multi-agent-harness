use super::*;

fn host_only_run(store: &HarnessStore, provider: &str, mode: HostControlMode) -> CreatedTeamRun {
    create_team_run(
        store,
        None,
        None,
        None,
        "Prove exact Host runtime admission",
        None,
        "test",
        (mode == HostControlMode::ExternalInteractive).then(|| "external-thread".into()),
        mode,
        None,
        None,
        None,
        None,
        &[TeamMemberSpec {
            agent_member_id: "host".into(),
            name: "Host".into(),
            role: "host".into(),
            provider: provider.into(),
            execution_mode: Some(if mode == HostControlMode::ExternalInteractive {
                EXECUTION_MODE_EXTERNAL_INTERACTIVE.into()
            } else {
                "codex_app_server".into()
            }),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        }],
    )
    .expect("create Host-only TeamRun")
}

#[test]
fn known_and_unknown_external_hosts_materialize_no_session_or_provider_effect() {
    for provider in ["codex", "owner-custom-provider"] {
        let (store, root) = temp_store(&format!("external-host-zero-effect-{provider}"));
        let created = host_only_run(&store, provider, HostControlMode::ExternalInteractive);
        let prepared = prepare_team_run_start_body(&store, &created.team_run.id, 1)
            .expect("external Host admission must not probe or admit a provider runtime");
        assert_eq!(prepared.members.len(), 1);
        assert!(prepared.members[0].is_external_interactive());
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                &format!("external-host-{provider}"),
                std::process::id(),
                "test://external-host",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire test Supervisor lease");

        ensure_test_runtime_fabric(&store, &created, &lease);

        assert!(
            store
                .fabric_agent_sessions(&lease.execution_space_id)
                .expect("read AgentSessions")
                .is_empty(),
            "external Host {provider} must not materialize an AgentSession"
        );
        assert!(
            store
                .runtime_commands(&lease.execution_space_id)
                .expect("read RuntimeCommands")
                .is_empty(),
            "external Host {provider} must not prepare a provider effect"
        );
        let host = latest_member_runs_in_append_order(&store)
            .expect("read MemberRuns")
            .into_iter()
            .find(|member| member.team_run_id == created.team_run.id)
            .expect("Host MemberRun");
        assert!(host.native_session.is_none());
        std::fs::remove_dir_all(root).expect("remove test store");
    }
}

#[test]
fn managed_host_session_is_coordination_read_only() {
    let (store, root) = temp_store("managed-host-read-only");
    let created = host_only_run(&store, "codex", HostControlMode::Managed);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "managed-host-read-only",
            std::process::id(),
            "test://managed-host",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire test Supervisor lease");

    ensure_test_runtime_fabric(&store, &created, &lease);

    let sessions = store
        .fabric_agent_sessions(&lease.execution_space_id)
        .expect("read AgentSessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(
        sessions[0].effective_permission_ceiling,
        harness_core::agentfirm_api::PermissionCeiling::ReadOnly
    );
    std::fs::remove_dir_all(root).expect("remove test store");
}
