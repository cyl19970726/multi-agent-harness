use super::*;

fn isolated_host_worktree(root: &std::path::Path) -> String {
    let project = root.join("unit-test-project");
    let worktree = root.join("host-workspace");
    std::fs::create_dir_all(&project).expect("create unit-test project");
    let run_git = |cwd: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .expect("run git fixture command");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_git(&project, &["init"]);
    run_git(
        &project,
        &["config", "user.email", "host-test@example.invalid"],
    );
    run_git(&project, &["config", "user.name", "Host Test"]);
    std::fs::write(
        project.join("README.md"),
        "managed Host workspace fixture\n",
    )
    .expect("write fixture file");
    run_git(&project, &["add", "README.md"]);
    run_git(&project, &["commit", "-m", "fixture"]);
    let worktree_arg = worktree.display().to_string();
    run_git(
        &project,
        &[
            "worktree",
            "add",
            "-b",
            "managed-host-fixture",
            &worktree_arg,
        ],
    );
    worktree_arg
}

fn host_only_run(
    store: &HarnessStore,
    provider: &str,
    mode: HostControlMode,
    provider_cwd_hint: Option<String>,
) -> CreatedTeamRun {
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
            execution_mode: Some(
                if mode == HostControlMode::ExternalInteractive {
                    EXECUTION_MODE_EXTERNAL_INTERACTIVE
                } else {
                    match provider {
                        "codex" => "codex_app_server",
                        "claude" => "claude_agent_sdk",
                        "kimi" => "kimi_acp",
                        "pi" => "pi_rpc",
                        _ => "unknown_managed",
                    }
                }
                .into(),
            ),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint,
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
        let created = host_only_run(&store, provider, HostControlMode::ExternalInteractive, None);
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
fn four_provider_managed_hosts_materialize_honest_isolated_sessions() {
    use harness_core::agentfirm_api::PermissionCeiling;

    for (provider, expected_ceiling) in [
        ("codex", PermissionCeiling::ReadOnly),
        ("claude", PermissionCeiling::ReadOnly),
        ("kimi", PermissionCeiling::FullAccess),
        ("pi", PermissionCeiling::ReadOnly),
    ] {
        let (store, root) = temp_store(&format!("managed-host-{provider}"));
        let host_workspace = (provider == "kimi").then(|| isolated_host_worktree(root.as_path()));
        let created = host_only_run(&store, provider, HostControlMode::Managed, host_workspace);
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                &format!("managed-host-{provider}"),
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
        assert_eq!(sessions.len(), 1, "managed {provider} Host session");
        assert_eq!(sessions[0].provider_kind, provider);
        assert_eq!(sessions[0].effective_permission_ceiling, expected_ceiling);
        std::fs::remove_dir_all(root).expect("remove test store");
    }
}

#[test]
fn managed_kimi_host_requires_a_workspace_distinct_from_the_team_root() {
    for alias_team_root in [false, true] {
        let label = if alias_team_root {
            "managed-host-kimi-shared-root"
        } else {
            "managed-host-kimi-no-isolation"
        };
        let (store, root) = temp_store(label);
        let provider_cwd_hint = alias_team_root.then(|| {
            let project = root.join("unit-test-project");
            std::fs::create_dir_all(&project).expect("create shared Team root");
            project.display().to_string()
        });
        let created = host_only_run(&store, "kimi", HostControlMode::Managed, provider_cwd_hint);
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                label,
                std::process::id(),
                "test://managed-host",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire test Supervisor lease");
        let error = ensure_team_message_fabric(
            &store,
            &created.team_run.id,
            &lease.execution_space_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
        )
        .expect_err("Kimi FullAccess Host without workspace isolation must fail closed");
        assert!(error
            .to_string()
            .contains("MANAGED_HOST_WORKSPACE_ISOLATION_REQUIRED"));
        assert!(store
            .fabric_agent_sessions(&lease.execution_space_id)
            .expect("read AgentSessions")
            .is_empty());
        std::fs::remove_dir_all(root).expect("remove test store");
    }
}

#[test]
fn managed_kimi_host_rejects_another_member_run_in_the_same_writable_workspace() {
    let (store, root) = temp_store("managed-host-kimi-duplicate-writer");
    let workspace = isolated_host_worktree(root.as_path());
    let members = [
        TeamMemberSpec {
            agent_member_id: "host".into(),
            name: "Host".into(),
            role: "host".into(),
            provider: "kimi".into(),
            execution_mode: Some("kimi_acp".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: Some(workspace.clone()),
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        },
        TeamMemberSpec {
            agent_member_id: "worker".into(),
            name: "Worker".into(),
            role: "implementer".into(),
            provider: "codex".into(),
            execution_mode: Some("codex_app_server".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: Some(workspace),
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        },
    ];
    let error = match create_team_run(
        &store,
        None,
        None,
        None,
        "Reject two writable drivers",
        None,
        "test",
        None,
        HostControlMode::Managed,
        None,
        None,
        None,
        None,
        &members,
    ) {
        Err(error) => error,
        Ok(_) => panic!("duplicate writable Host workspace must fail during Store admission"),
    };
    assert!(error
        .to_string()
        .contains("MANAGED_HOST_WORKSPACE_ALREADY_CLAIMED"));
    assert!(store
        .fabric_agent_sessions("unit-test-space")
        .expect("read AgentSessions")
        .is_empty());
    std::fs::remove_dir_all(root).expect("remove test store");
}
