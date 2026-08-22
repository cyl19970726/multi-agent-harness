use super::*;

/// Parse one `--member name:role:provider[/mode][:model][@path1,path2][#brief]` spec.
///
/// The brief is split off FIRST and is free text: it may contain `@` and `:`,
/// which the identity grammar would otherwise consume. Without it the only way
/// to brief a member is the run-level objective, which is then delivered
/// verbatim to every member of a multi-lane run.
///
/// `/mode` names the execution mode (`app-server`, `acp`, `agent-sdk`
/// shortcuts or the literal mode id). `external_interactive` declares a
/// user-driven external session: Harness spawns no provider process for it,
/// and the exact environment-bound member polls its own inbox via the legacy
/// `team-run inbox` hook path. Managed provider members use `member inbox`.
pub(super) fn parse_team_member_spec(raw: &str) -> CliResult<TeamMemberSpec> {
    let (raw, inline_work) = match raw.split_once('#') {
        Some((head, brief)) if !brief.trim().is_empty() => (head, Some(brief.trim().to_string())),
        _ => (raw, None),
    };
    let (identity, owned_paths) = match raw.split_once('@') {
        Some((identity, paths)) => (
            identity,
            paths
                .split(',')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        None => (raw, Vec::new()),
    };
    let parts: Vec<&str> = identity.split(':').collect();
    if parts.len() < 3 || parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
        return Err(CliError::Usage(format!(
            "invalid --member `{raw}` (expected name:role:provider[/mode][:model][@path1,path2][#brief])"
        )));
    }
    let (provider, execution_mode) = match parts[2].split_once('/') {
        Some((provider, mode)) if !provider.is_empty() && !mode.is_empty() => (
            provider.to_string(),
            Some(match mode {
                "app-server" | "app_server" => "codex_app_server".to_string(),
                "exec" => "codex_exec".to_string(),
                "acp" => "kimi_acp".to_string(),
                "cli" if provider == "claude" => "claude_cli".to_string(),
                "agent-sdk" | "agent_sdk" if provider == "claude" => "claude_agent_sdk".to_string(),
                other => other.to_string(),
            }),
        ),
        _ => (parts[2].to_string(), None),
    };
    Ok(TeamMemberSpec {
        agent_member_id: parts[0].to_string(),
        name: parts[0].to_string(),
        role: parts[1].to_string(),
        provider,
        execution_mode,
        model: parts
            .get(3)
            .map(|model| model.to_string())
            .filter(|model| !model.is_empty()),
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths,
        resume_native_session_id: None,
        initial_work: inline_work,
    })
}

pub(super) fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    let resolved = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    Some(project::canonicalize_best_effort(&resolved))
}

pub(super) fn git_worktree_root(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let value = raw.trim();
    (!value.is_empty()).then(|| project::canonicalize_best_effort(Path::new(value)))
}

/// Resolve and validate an explicit TeamRun/member workspace override. A
/// registered project may execute at its own root or at any Git worktree that
/// shares its git common directory, including Codex worktrees outside the
/// project path. Raw-store compatibility mode has no registered project
/// identity, so it can only require an existing directory.
pub(super) fn validate_workspace_override(
    project_context: Option<&ProjectContext>,
    raw: &str,
    label: &str,
) -> CliResult<String> {
    if raw.trim().is_empty() {
        return Err(CliError::Usage(format!("{label} must not be empty")));
    }
    let base = project_context
        .map(|context| context.project_root.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let requested = PathBuf::from(raw);
    let requested = if requested.is_absolute() {
        requested
    } else {
        base.join(requested)
    };
    let resolved = project::canonicalize_best_effort(&requested);
    if !resolved.is_dir() {
        return Err(CliError::Usage(format!(
            "{label} is not an existing directory: {}",
            resolved.display()
        )));
    }
    if let Some(context) = project_context {
        let project_root = project::canonicalize_best_effort(&context.project_root);
        let allowed = resolved == project_root
            || (git_worktree_root(&resolved).as_ref() == Some(&resolved)
                && git_common_dir(&resolved)
                    .zip(git_common_dir(&project_root))
                    .is_some_and(|(candidate, project)| candidate == project));
        if !allowed {
            return Err(CliError::Usage(format!(
                "{label} must be the selected project root {} or a Git worktree sharing its git common directory",
                project_root.display()
            )));
        }
    }
    Ok(resolved.display().to_string())
}

pub(super) fn default_execution_root(project_context: Option<&ProjectContext>) -> String {
    let root = project_context
        .map(|context| context.project_root.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    project::canonicalize_best_effort(&root)
        .display()
        .to_string()
}

/// Everything `team-run create` journals, returned so the CLI/HTTP layers can
/// render it.
pub(super) struct CreatedTeamRun {
    pub(super) team_run: AgentTeamRun,
    pub(super) member_runs: Vec<ProviderRuntimeProjection>,
    pub(super) works: Vec<Work>,
}

pub(super) fn build_member_run_for_team(
    project_context: Option<&ProjectContext>,
    team_run_id: &str,
    member: &TeamMemberSpec,
) -> CliResult<ProviderRuntimeProjection> {
    let profile =
        team_member_provider_profile_for_mode(&member.provider, member.execution_mode.as_deref());
    let native_session =
        member
            .resume_native_session_id
            .as_ref()
            .map(|session_id| NativeSessionRef {
                provider: member.provider.clone(),
                execution_mode: profile.execution_mode.clone(),
                native_session_id: session_id.clone(),
                native_locator_kind: match member.provider.as_str() {
                    "codex" => "codex_rollout",
                    "kimi" => "kimi_code_session",
                    "claude" => "claude_project_session",
                    _ => "provider_native",
                }
                .to_string(),
                provider_version: profile.provider_version.clone(),
                adapter_contract_version: profile
                    .adapter_contract_version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                availability: NativeSessionAvailability::Unknown,
                supports_resume: profile.supports_resume,
                last_verified_at: None,
                parent_native_session_id: Some(session_id.clone()),
            });
    Ok(ProviderRuntimeProjection {
        id: generated_id("member-run"),
        team_run_id: team_run_id.to_string(),
        slot_id: None,
        agent_member_id: member.agent_member_id.clone(),
        name: member.name.clone(),
        role: member.role.clone(),
        provider: member.provider.clone(),
        model: member.model.clone(),
        provider_controls: ProviderExecutionControls::requested(
            member.model.clone(),
            member.effort.clone(),
            member.service_tier.clone(),
        ),
        provider_profile: Some(profile),
        // Capacity is observed at start, never assumed at creation. An absent
        // snapshot is honestly unknown, not available.
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: MemberCoordinationStatus::Active,
        runtime_generation: 1,
        status: MemberRunStatus::Idle,
        native_session,
        provider_cwd_hint: member
            .provider_cwd_hint
            .as_deref()
            .map(|value| {
                validate_workspace_override(project_context, value, "member provider_cwd_hint")
            })
            .transpose()?,
        provider_environment_observation: None,
        owned_paths: member.owned_paths.clone(),
        started_at: now_string(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    })
}

/// Convert the ledger-facing NativeSessionRef into the trust-fabric
/// agentfirm_api shape. Only the binding fields cross this boundary; the
/// provider stream itself never enters the trust store.
pub(super) fn agentfirm_native_session_ref(
    session: &NativeSessionRef,
) -> harness_core::agentfirm_api::NativeSessionRef {
    harness_core::agentfirm_api::NativeSessionRef {
        provider: session.provider.clone(),
        execution_mode: session.execution_mode.clone(),
        native_session_id: session.native_session_id.clone(),
        native_locator_kind: session.native_locator_kind.clone(),
        provider_version: session.provider_version.clone(),
        adapter_contract_version: session.adapter_contract_version.clone(),
        availability: match session.availability {
            NativeSessionAvailability::Available => {
                harness_core::agentfirm_api::NativeSessionAvailability::Available
            }
            NativeSessionAvailability::Stale => {
                harness_core::agentfirm_api::NativeSessionAvailability::Stale
            }
            NativeSessionAvailability::Missing => {
                harness_core::agentfirm_api::NativeSessionAvailability::Missing
            }
            NativeSessionAvailability::Incompatible => {
                harness_core::agentfirm_api::NativeSessionAvailability::Incompatible
            }
            NativeSessionAvailability::Unknown => {
                harness_core::agentfirm_api::NativeSessionAvailability::Unknown
            }
        },
        supports_resume: session.supports_resume,
        last_verified_at: session.last_verified_at.clone(),
        parent_native_session_id: session.parent_native_session_id.clone(),
    }
}

pub(super) fn agentfirm_native_session_identity_matches(
    left: Option<&harness_core::agentfirm_api::NativeSessionRef>,
    right: Option<&harness_core::agentfirm_api::NativeSessionRef>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.provider == right.provider
                && left.execution_mode == right.execution_mode
                && left.native_session_id == right.native_session_id
                && left.native_locator_kind == right.native_locator_kind
                && left.provider_version == right.provider_version
                && left.adapter_contract_version == right.adapter_contract_version
        }
        _ => false,
    }
}

pub(super) fn canonical_member_run_admission(
    execution_space_id: &str,
    runtime: &ProviderRuntimeProjection,
) -> CanonicalMemberRunAdmission {
    let native_session = runtime
        .native_session
        .as_ref()
        .map(agentfirm_native_session_ref);
    let run = harness_core::agentfirm_api::MemberRun {
        id: runtime.id.clone(),
        agent_member_id: runtime.agent_member_id.clone(),
        team_run_id: runtime.team_run_id.clone(),
        role_snapshot: runtime.role.clone(),
        provider_profile_snapshot: runtime
            .provider_profile
            .as_ref()
            .map(|profile| format!("{}/{}", profile.provider, profile.execution_mode)),
        requested_controls: serde_json::json!({
            "model": runtime.provider_controls.model.requested,
            "reasoning_effort": runtime.provider_controls.reasoning_effort.requested,
            "service_tier": runtime.provider_controls.service_tier.requested,
        }),
        effective_controls: serde_json::json!({
            "model": runtime.provider_controls.model.effective,
            "reasoning_effort": runtime.provider_controls.reasoning_effort.effective,
            "service_tier": runtime.provider_controls.service_tier.effective,
        }),
        coordination_status: match runtime.coordination_status {
            MemberCoordinationStatus::Active => {
                harness_core::agentfirm_api::MemberCoordinationStatus::Active
            }
            MemberCoordinationStatus::Closed => {
                harness_core::agentfirm_api::MemberCoordinationStatus::Closed
            }
            MemberCoordinationStatus::Retired => {
                harness_core::agentfirm_api::MemberCoordinationStatus::Retired
            }
        },
        runtime_status: harness_core::agentfirm_api::MemberRuntimeStatus::Idle,
        runtime_generation: runtime.runtime_generation,
        workspace_binding_id: None,
        native_session,
        version: 1,
        started_at: runtime.started_at.clone(),
        last_event_at: runtime.last_event_at.clone(),
        finished_at: runtime.finished_at.clone(),
    };
    CanonicalMemberRunAdmission {
        context: harness_core::agentfirm_api::MutationContext {
            execution_space_id: execution_space_id.to_string(),
            authenticated_actor: harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::Service,
                id: "node-daemon:team-run-create".into(),
            },
            authority_actor: Some(harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: runtime.agent_member_id.clone(),
            }),
            command_name: "team_run.materialize_member_run".into(),
            idempotency_key: format!("team-run-member-run:{}", runtime.id),
            expected_version: 0,
            request_fingerprint: None,
        },
        run,
    }
}

pub(super) fn created_team_run_json(created: &CreatedTeamRun) -> serde_json::Value {
    let host_member_run = created.member_runs.iter().find(|member| {
        created
            .team_run
            .host_actor
            .as_ref()
            .is_some_and(|host| member.agent_member_id == host.id)
    });
    let managed = created.team_run.host_control_mode == HostControlMode::Managed;
    serde_json::json!({
        "team_run": created.team_run,
        "member_runs": created.member_runs,
        "works": created.works,
        "host_runtime": {
            "mode": if managed { "managed" } else { "external_interactive" },
            "host_member_run_id": host_member_run.map(|member| member.id.as_str()),
            "delivery_guarantee": if managed { "daemon_managed" } else { "pull_only" },
            "runtime_residency": if managed { "managed_member_run" } else { "detached_user_driven" },
            "warning": (!managed).then_some("External Host must read or wait for inbox updates"),
        },
    })
}

/// Persist a new team run: the AgentTeamRun (status planning), one idle
/// ProviderRuntimeProjection per member, an optional initial Work for each explicitly supplied
/// member brief, and a folded TeamRunEvent per created entity. A run-level
/// objective is context, never an implicit duplicate assignment to every
/// member. Shared by the `team-run create` CLI
/// arm and POST /v1/team-runs. `previous_run_id` records explicit retry
/// lineage. It must remain inside the same Mission/Team relation; historical
/// direct-Wave retries additionally remain inside that exact Wave.
#[cfg(test)]
pub(super) fn ensure_legacy_unit_test_team_binding(
    store: &HarnessStore,
    host_spec: Option<&TeamMemberSpec>,
) -> CliResult<(ProjectContext, String)> {
    // This compatibility fixture is compiled only into the `firm` unit-test
    // binary. Production and integration-test command paths must always supply
    // their real Project Binding and AgentTeam explicitly.
    const MISSION_ID: &str = "unit-test-mission";
    const TEAM_ID: &str = "unit-test-agent-team";
    const NODE_ID: &str = "00000000-0000-4000-8000-000000000001";
    const PROJECT_ID: &str = "unit-test-project";
    const SPACE_ID: &str = "unit-test-space";

    let now = "unix-ms:1".to_string();
    if !store
        .latest_missions()?
        .iter()
        .any(|mission| mission.id == MISSION_ID)
    {
        store.append_mission(&Mission {
            id: MISSION_ID.to_string(),
            title: "Unit-test mission".to_string(),
            objective: "Exercise legacy unit fixtures through the canonical Team contract"
                .to_string(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Planned,
            legacy_wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            completed_at: None,
        })?;
    }
    if !store
        .latest_execution_nodes()?
        .iter()
        .any(|node| node.id == NODE_ID)
    {
        store.insert_execution_node(&ExecutionNode {
            id: NODE_ID.to_string(),
            display_name: "unit-test-node".to_string(),
            status: ExecutionNodeStatus::Active,
            created_at: now.clone(),
            updated_at: now.clone(),
        })?;
    }
    let registration_exists =
        store
            .latest_node_project_registrations()?
            .iter()
            .any(|registration| {
                registration.node_id == NODE_ID
                    && registration.execution_space_id == SPACE_ID
                    && registration.project_binding_id == PROJECT_ID
                    && registration.status == NodeProjectRegistrationStatus::Active
            });
    if !registration_exists {
        store.register_node_project(
            &NodeProjectRegistration {
                node_id: NODE_ID.to_string(),
                execution_space_id: SPACE_ID.to_string(),
                project_binding_id: PROJECT_ID.to_string(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
            SPACE_ID,
        )?;
    }
    let creator = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: "unit-test-fixture".into(),
    };
    if !store
        .trust_agent_members(SPACE_ID)?
        .iter()
        .any(|member| member.id == "host")
    {
        let host_provider = host_spec
            .map(|member| member.provider.as_str())
            .unwrap_or("codex");
        store.create_trust_agent_member(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: SPACE_ID.into(),
                authenticated_actor: creator.clone(),
                authority_actor: None,
                command_name: "unit_test.agent_member.create".into(),
                idempotency_key: "unit-test-member:host".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            harness_core::agentfirm_api::AgentMember {
                id: "host".into(),
                name: "Host".into(),
                description: "canonical unit-test Host AgentMember".into(),
                role: "host".into(),
                capabilities: Vec::new(),
                skill_refs: Vec::new(),
                provider_profile_ref: Some(host_provider.into()),
                model_preference: None,
                workspace_policy: "managed-worktree".into(),
                permission_ceiling: if matches!(host_provider, "kimi" | "pi") {
                    harness_core::agentfirm_api::PermissionCeiling::FullAccess
                } else {
                    harness_core::agentfirm_api::PermissionCeiling::WorkspaceWrite
                },
                organization_status:
                    harness_core::agentfirm_api::AgentMemberOrganizationStatus::Active,
                version: 1,
                created_by: creator.clone(),
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        )?;
    }
    if !latest_teams(store)?.contains_key(TEAM_ID) {
        let team = AgentTeam {
            id: TEAM_ID.to_string(),
            name: "Unit-test AgentTeam".to_string(),
            description: "Canonical binding for pre-Wave-3 unit fixtures".to_string(),
            node_id: NODE_ID.to_string(),
            status: AgentTeamStatus::Active,
            revision: 1,
            legacy_mission_id: Some(MISSION_ID.to_string()),
            trashed_at: None,
            mission_id: MISSION_ID.to_string(),
            host_agent_id: "host".to_string(),
            member_ids: Vec::new(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        store.create_agent_team(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: SPACE_ID.into(),
                authenticated_actor: creator.clone(),
                authority_actor: None,
                command_name: "unit_test.agent_team.create".into(),
                idempotency_key: "unit-test-agent-team".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            team,
            vec![harness_core::agentfirm_api::TeamMembership {
                id: format!("membership:{TEAM_ID}:host"),
                team_id: TEAM_ID.into(),
                agent_member_id: "host".into(),
                node_id: NODE_ID.into(),
                role: harness_core::agentfirm_api::TeamMembershipRole::Host,
                state: harness_core::agentfirm_api::TeamMembershipStatus::Active,
                membership_generation: 1,
                default_subscription_refs: Vec::new(),
                created_by: creator,
                revision: 1,
                joined_at: now.clone(),
                left_at: None,
            }],
        )?;
    }

    let project_root = store.root().join("unit-test-project");
    std::fs::create_dir_all(&project_root)?;
    Ok((
        ProjectContext {
            id: PROJECT_ID.to_string(),
            project_root,
            store_root: store.root().to_path_buf(),
            kind: ProjectKind::Repo,
            is_git_repo: false,
        },
        TEAM_ID.to_string(),
    ))
}

#[cfg(test)]
pub(super) fn ensure_unit_test_canonical_members(
    store: &HarnessStore,
    execution_space_id: &str,
    team_id: &str,
    members: &[TeamMemberSpec],
) -> CliResult<()> {
    let creator = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: "unit-test-fixture".into(),
    };
    let existing = store
        .trust_agent_members(execution_space_id)?
        .into_iter()
        .map(|member| member.id)
        .collect::<BTreeSet<_>>();
    let team = store
        .agent_teams(execution_space_id)?
        .into_iter()
        .find(|team| team.id == team_id);
    let mut existing_memberships = store
        .fabric_team_memberships(execution_space_id)?
        .into_iter()
        .filter(|membership| {
            membership.team_id == team_id
                && membership.state == harness_core::agentfirm_api::TeamMembershipStatus::Active
        })
        .map(|membership| membership.agent_member_id)
        .collect::<BTreeSet<_>>();
    for member in members {
        let now = "unix-ms:1".to_string();
        if !existing.contains(&member.agent_member_id) {
            store.create_trust_agent_member(
                &harness_core::agentfirm_api::MutationContext {
                    execution_space_id: execution_space_id.to_string(),
                    authenticated_actor: creator.clone(),
                    authority_actor: None,
                    command_name: "unit_test.agent_member.create".into(),
                    idempotency_key: format!(
                        "unit-test-member:{execution_space_id}:{}",
                        member.agent_member_id
                    ),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                harness_core::agentfirm_api::AgentMember {
                    id: member.agent_member_id.clone(),
                    name: member.name.clone(),
                    description: "canonical unit-test AgentMember".into(),
                    role: member.role.clone(),
                    capabilities: Vec::new(),
                    skill_refs: Vec::new(),
                    provider_profile_ref: Some(member.provider.clone()),
                    model_preference: member.model.clone(),
                    workspace_policy: "managed-worktree".into(),
                    permission_ceiling: if matches!(member.provider.as_str(), "kimi" | "pi") {
                        // Pi RPC has no filesystem containment for write/edit.
                        // Unit-test Team fixtures mirror the explicit trusted
                        // development policy; Kimi's callback bridge likewise
                        // admits only an exact full-access ceiling.
                        harness_core::agentfirm_api::PermissionCeiling::FullAccess
                    } else {
                        harness_core::agentfirm_api::PermissionCeiling::WorkspaceWrite
                    },
                    organization_status:
                        harness_core::agentfirm_api::AgentMemberOrganizationStatus::Active,
                    version: 1,
                    created_by: creator.clone(),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                },
            )?;
        }
        if let Some(team) = team
            .as_ref()
            .filter(|_| !existing_memberships.contains(&member.agent_member_id))
        {
            store.join_team_membership(
                &harness_core::agentfirm_api::MutationContext {
                    execution_space_id: execution_space_id.to_string(),
                    authenticated_actor: creator.clone(),
                    authority_actor: None,
                    command_name: "unit_test.team_membership.join".into(),
                    idempotency_key: format!(
                        "unit-test-membership:{execution_space_id}:{team_id}:{}",
                        member.agent_member_id
                    ),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                harness_core::agentfirm_api::TeamMembership {
                    id: format!("membership:{team_id}:{}", member.agent_member_id),
                    team_id: team_id.into(),
                    agent_member_id: member.agent_member_id.clone(),
                    node_id: team.node_id.clone(),
                    role: if member.agent_member_id == team.host_agent_id {
                        harness_core::agentfirm_api::TeamMembershipRole::Host
                    } else {
                        harness_core::agentfirm_api::TeamMembershipRole::Member
                    },
                    state: harness_core::agentfirm_api::TeamMembershipStatus::Active,
                    membership_generation: 1,
                    default_subscription_refs: Vec::new(),
                    created_by: creator.clone(),
                    revision: 1,
                    joined_at: now,
                    left_at: None,
                },
            )?;
            existing_memberships.insert(member.agent_member_id.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn ensure_unit_test_canonical_team(
    store: &HarnessStore,
    execution_space_id: &str,
    source_team: &AgentTeam,
    members: &[TeamMemberSpec],
) -> CliResult<()> {
    ensure_unit_test_canonical_members(store, execution_space_id, &source_team.id, members)?;
    if store
        .agent_teams(execution_space_id)?
        .iter()
        .any(|team| team.id == source_team.id)
    {
        return Ok(());
    }
    if !members
        .iter()
        .any(|member| member.agent_member_id == source_team.host_agent_id)
    {
        return Err(CliError::Usage(format!(
            "unit-test Team {} requires its exact Host AgentMember fixture",
            source_team.id
        )));
    }
    let creator = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: "unit-test-fixture".into(),
    };
    let timestamp = format!("unix-ms:{execution_space_id}");
    let team = AgentTeam {
        revision: 1,
        status: AgentTeamStatus::Active,
        trashed_at: None,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
        ..source_team.clone()
    };
    let memberships = members
        .iter()
        .map(|member| harness_core::agentfirm_api::TeamMembership {
            id: format!("membership:{}:{}", source_team.id, member.agent_member_id),
            team_id: source_team.id.clone(),
            agent_member_id: member.agent_member_id.clone(),
            node_id: source_team.node_id.clone(),
            role: if member.agent_member_id == source_team.host_agent_id {
                harness_core::agentfirm_api::TeamMembershipRole::Host
            } else {
                harness_core::agentfirm_api::TeamMembershipRole::Member
            },
            state: harness_core::agentfirm_api::TeamMembershipStatus::Active,
            membership_generation: 1,
            default_subscription_refs: Vec::new(),
            created_by: creator.clone(),
            revision: 1,
            joined_at: timestamp.clone(),
            left_at: None,
        })
        .collect();
    store.create_agent_team(
        &harness_core::agentfirm_api::MutationContext {
            execution_space_id: execution_space_id.to_string(),
            authenticated_actor: creator,
            authority_actor: None,
            command_name: "unit_test.agent_team.create".into(),
            idempotency_key: format!("unit-test-team:{execution_space_id}:{}", source_team.id),
            expected_version: 0,
            request_fingerprint: None,
        },
        team,
        memberships,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn create_team_run(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    execution_space_id: Option<&str>,
    requested_execution_root: Option<String>,
    objective: &str,
    budget_limit_usd: Option<f64>,
    host_surface: &str,
    host_thread_id: Option<String>,
    host_control_mode: HostControlMode,
    previous_run_id: Option<String>,
    agent_team_id: Option<String>,
    mission_id: Option<String>,
    wave_id: Option<String>,
    members: &[TeamMemberSpec],
) -> CliResult<CreatedTeamRun> {
    if objective.trim().is_empty() {
        return Err(CliError::Usage(
            "team-run objective must not be empty".to_string(),
        ));
    }
    if host_surface.trim().is_empty() {
        return Err(CliError::Usage(
            "team-run host surface must not be empty".to_string(),
        ));
    }
    if host_thread_id
        .as_ref()
        .is_some_and(|id| id.trim().is_empty())
        || previous_run_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
    {
        return Err(CliError::Usage(
            "host_thread_id and previous_run_id must not be empty when supplied".to_string(),
        ));
    }
    if budget_limit_usd.is_some_and(|budget| !budget.is_finite() || budget < 0.0) {
        return Err(CliError::Usage(
            "team-run budget must be a finite non-negative number".to_string(),
        ));
    }
    if members.is_empty() {
        return Err(CliError::Usage(
            "agent_team runs require at least one member".to_string(),
        ));
    }
    #[cfg(test)]
    let legacy_test_binding = if project_context.is_none()
        && agent_team_id.is_none()
        && mission_id.is_none()
        && wave_id.is_none()
    {
        Some(ensure_legacy_unit_test_team_binding(
            store,
            members
                .iter()
                .find(|member| member.agent_member_id == "host"),
        )?)
    } else {
        None
    };
    #[cfg(test)]
    let project_context = project_context.or_else(|| {
        legacy_test_binding
            .as_ref()
            .map(|(project_context, _)| project_context)
    });
    #[cfg(test)]
    let agent_team_id = agent_team_id.or_else(|| {
        legacy_test_binding
            .as_ref()
            .map(|(_, agent_team_id)| agent_team_id.clone())
    });
    #[cfg(test)]
    let execution_space_id = execution_space_id.unwrap_or("unit-test-space");
    #[cfg(not(test))]
    let execution_space_id = execution_space_id.ok_or_else(|| {
        CliError::Usage("an Execution Space is required for every AgentTeamRun".to_string())
    })?;
    let execution_root = match requested_execution_root {
        Some(root) => validate_workspace_override(project_context, &root, "execution_root")?,
        None => default_execution_root(project_context),
    };
    #[cfg(test)]
    if legacy_test_binding.is_some() {
        ensure_unit_test_canonical_members(
            store,
            execution_space_id,
            agent_team_id
                .as_deref()
                .expect("legacy unit-test binding supplies a team"),
            members,
        )?;
    }
    let mut member_names = std::collections::HashSet::new();
    for member in members {
        if member.name.trim().is_empty()
            || member.role.trim().is_empty()
            || member.provider.trim().is_empty()
            || member
                .model
                .as_ref()
                .is_some_and(|model| model.trim().is_empty())
        {
            return Err(CliError::Usage(
                "team member name, role, and provider must not be empty".to_string(),
            ));
        }
        if !member_names.insert(member.name.as_str()) {
            return Err(CliError::Usage(format!(
                "duplicate team member name: {}",
                member.name
            )));
        }
        if member.owned_paths.iter().any(|path| path.trim().is_empty()) {
            return Err(CliError::Usage(format!(
                "team member {} has an empty owned path",
                member.name
            )));
        }
        if let Some(provider_cwd_hint) = member.provider_cwd_hint.as_deref() {
            validate_workspace_override(
                project_context,
                provider_cwd_hint,
                "member provider_cwd_hint",
            )?;
        }
        validate_team_member_execution_mode(member)?;
        validate_team_member_identity(store, member)?;
    }
    if mission_id.is_some() || wave_id.is_some() {
        return Err(CliError::Usage(
            "mission_id and wave_id were removed from AgentTeamRun; select the required agent_team_id and derive Mission through AgentTeam"
                .to_string(),
        ));
    }
    let agent_team_id = agent_team_id.ok_or_else(|| {
        CliError::Usage("agent_team_id is required for every AgentTeamRun".to_string())
    })?;
    let team = latest_teams(store)?
        .remove(&agent_team_id)
        .ok_or_else(|| CliError::Usage(format!("team not found: {agent_team_id}")))?;
    if team.status != AgentTeamStatus::Active {
        return Err(CliError::Usage(format!(
            "team {agent_team_id} is not active"
        )));
    }
    for member in members {
        if team.host_agent_id != member.agent_member_id
            && !team.member_ids.contains(&member.agent_member_id)
        {
            return Err(CliError::Usage(format!(
                "AgentMember {} is not part of AgentTeam {}",
                member.agent_member_id, team.id
            )));
        }
    }
    let project_context = project_context.ok_or_else(|| {
        CliError::Usage("a project binding is required for every AgentTeamRun".to_string())
    })?;
    // Retry lineage never crosses the stable Team identity.
    if let Some(previous) = previous_run_id.as_deref() {
        let previous = latest_team_run(store, previous)?;
        if previous.agent_team_id != agent_team_id {
            return Err(CliError::Usage(format!(
                "previous run {} is not for the same agent team",
                previous.id
            )));
        }
    }
    let run_id = generated_id("team-run");
    let mut member_runs = Vec::new();
    let mut member_run_ids = Vec::new();
    for member in members {
        let member_run = build_member_run_for_team(Some(project_context), &run_id, member)?;
        member_run_ids.push(member_run.id.clone());
        member_runs.push(member_run);
    }
    let host_members = member_runs
        .iter()
        .filter(|runtime| runtime.agent_member_id == team.host_agent_id)
        .collect::<Vec<_>>();
    let [host_member] = host_members.as_slice() else {
        return Err(CliError::Usage(format!(
            "AgentTeam {} requires exactly one Host MemberRun; found {}",
            team.id,
            host_members.len()
        )));
    };
    match host_control_mode {
        HostControlMode::Managed if host_member.is_external_interactive() => {
            return Err(CliError::Usage(
                "managed Host must use the canonical Team provider runtime".to_string(),
            ));
        }
        HostControlMode::ExternalInteractive if !host_member.is_external_interactive() => {
            return Err(CliError::Usage(
                "external_interactive Host must use an external_interactive MemberRun".to_string(),
            ));
        }
        _ => {}
    }
    if host_control_mode == HostControlMode::Managed && host_thread_id.is_some() {
        return Err(CliError::Usage(
            "managed Host uses its exact AgentSession; host_thread_id is external-only".to_string(),
        ));
    }
    let team_run = AgentTeamRun {
        id: run_id.clone(),
        agent_team_id,
        execution_node_id: team.node_id,
        project_binding_id: project_context.id.clone(),
        previous_run_id,
        host_surface: host_surface.to_string(),
        host_thread_id,
        host_actor: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: team.host_agent_id.clone(),
            display_name: Some(host_member.name.clone()),
            authn_source: Some("team_membership:host".to_string()),
        }),
        host_control_mode,
        objective: objective.to_string(),
        execution_root: Some(execution_root),
        status: TeamRunStatus::Planning,
        member_run_ids,
        budget_limit_usd,
        created_at: now_string(),
        updated_at: now_string(),
        completed_at: None,
    };

    // A freshly-generated run id has no current projection or events yet, so
    // its first guarded event sequence is exactly 1. Do not route creation
    // through the current-event reader before the combined admission commits.
    let mut seq = 1;
    let canonical_member_runs = member_runs
        .iter()
        .map(|runtime| canonical_member_run_admission(execution_space_id, runtime))
        .collect::<Vec<_>>();
    store_conflict_as_usage(store.create_team_run_with_member_runs_from_agent_team(
        &team_run,
        execution_space_id,
        &member_runs,
        &canonical_member_runs,
    ))?;
    append_team_run_event(
        store,
        &run_id,
        seq,
        TeamRunEventSourceKind::Host,
        None,
        "team_run",
        &team_run.id,
        "created",
        &format!("team run created: {objective}"),
    )?;
    seq += 1;

    let mut works = Vec::new();
    // `member_runs` is built from `members` in order above, so zip pairs each
    // ProviderRuntimeProjection with the spec that produced it and its optional initial Work.
    for (member, member_run) in members.iter().zip(&member_runs) {
        append_team_run_event(
            store,
            &run_id,
            seq,
            TeamRunEventSourceKind::Host,
            Some(member_run.id.clone()),
            "member_run",
            &member_run.id,
            "created",
            &format!(
                "member {} ({}/{}) joined",
                member_run.name, member_run.role, member_run.provider
            ),
        )?;
        seq += 1;

        if let Some(brief) = member.initial_work.as_deref() {
            let now = now_string();
            let work = store.insert_work(
                Work {
                    id: generated_id("work"),
                    team_run_id: run_id.clone(),
                    accountable_team_id: None,
                    assignee_membership_id: None,
                    created_by_member_id: None,
                    parent_work_id: None,
                    title: format!("{}: {}", member_run.name, member_run.role),
                    context_markdown: format!("Team objective:\n\n{objective}"),
                    completion_criteria_markdown: brief.to_string(),
                    phase: WorkPhase::Open,
                    condition: WorkCondition::Normal,
                    resolution: None,
                    owner_member_id: None,
                    active_member_run_id: Some(member_run.id.clone()),
                    claim_mode: WorkClaimMode::HostAssign,
                    eligible_member_ids: Vec::new(),
                    prerequisite_work_ids: Vec::new(),
                    priority: WorkPriority::Normal,
                    created_by_actor: team_run.host_actor.clone().ok_or_else(|| {
                        CliError::Usage("TeamRun has no exact Host AgentMember actor".to_string())
                    })?,
                    result_summary: None,
                    blocker_reason: None,
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    version: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                WorkCommandContext {
                    event_id: generated_id("work-event"),
                    performed_by_actor: team_run.host_actor.clone().ok_or_else(|| {
                        CliError::Usage("TeamRun has no exact Host AgentMember actor".to_string())
                    })?,
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: generated_id("work-command"),
                    created_at: now,
                    duplicate_ok: false,
                },
            )?;
            append_team_run_event(
                store,
                &run_id,
                seq,
                TeamRunEventSourceKind::Host,
                Some(member_run.id.clone()),
                "work",
                &work.id,
                "created",
                &format!("initial Work created for {}", member_run.name),
            )?;
            seq += 1;
            works.push(work);
        }
    }

    Ok(CreatedTeamRun {
        team_run,
        member_runs,
        works,
    })
}

pub(super) fn add_team_run_member(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    team_run_id: &str,
    member: &TeamMemberSpec,
    initial_work: Option<&str>,
) -> CliResult<(AgentTeamRun, ProviderRuntimeProjection, Option<Work>)> {
    validate_team_member_execution_mode(member)?;
    validate_team_member_identity(store, member)?;
    if initial_work.is_some_and(|value| value.trim().is_empty()) {
        return Err(CliError::Usage(
            "new member initial Work must not be empty".to_string(),
        ));
    }
    let current = latest_team_run(store, team_run_id)?;
    if !matches!(
        current.status,
        TeamRunStatus::Planning | TeamRunStatus::Running | TeamRunStatus::Waiting
    ) {
        return Err(CliError::Usage(format!(
            "team run {team_run_id} is {} and cannot accept a member",
            serde_snake_label(&current.status)
        )));
    }
    if latest_member_runs_in_append_order(store)?
        .into_iter()
        .any(|existing| existing.team_run_id == team_run_id && existing.name == member.name)
    {
        return Err(CliError::Usage(format!(
            "team run {team_run_id} already has a member named {}",
            member.name
        )));
    }
    let member_run = build_member_run_for_team(project_context, team_run_id, member)?;
    let execution_space_id = team_run_execution_space_id(store, &current)?;
    let mut next = current.clone();
    next.member_run_ids.push(member_run.id.clone());
    next.updated_at = now_string();
    let canonical_member_run = canonical_member_run_admission(&execution_space_id, &member_run);
    store_conflict_as_usage(store.admit_member_run_with_canonical(
        &current,
        &next,
        &member_run,
        &execution_space_id,
        &canonical_member_run,
    ))?;
    append_team_run_event(
        store,
        team_run_id,
        next_team_run_seq(store, team_run_id)?,
        TeamRunEventSourceKind::Host,
        Some(member_run.id.clone()),
        "member_run",
        &member_run.id,
        "created",
        &format!(
            "member {} ({}/{}) joined an existing run",
            member_run.name, member_run.role, member_run.provider
        ),
    )?;
    let work = initial_work
        .map(|brief| {
            store_conflict_as_usage(store.insert_work(
                Work {
                    id: generated_id("work"),
                    team_run_id: team_run_id.to_string(),
                    accountable_team_id: None,
                    assignee_membership_id: None,
                    created_by_member_id: None,
                    parent_work_id: None,
                    title: format!("{}: {}", member_run.name, member_run.role),
                    context_markdown: String::new(),
                    completion_criteria_markdown: brief.trim().to_string(),
                    phase: WorkPhase::Open,
                    condition: WorkCondition::Normal,
                    resolution: None,
                    owner_member_id: None,
                    active_member_run_id: Some(member_run.id.clone()),
                    claim_mode: WorkClaimMode::HostAssign,
                    eligible_member_ids: Vec::new(),
                    prerequisite_work_ids: Vec::new(),
                    priority: WorkPriority::Normal,
                    created_by_actor: next.host_actor.clone().ok_or_else(|| {
                        CliError::Usage("TeamRun has no exact Host AgentMember actor".to_string())
                    })?,
                    result_summary: None,
                    blocker_reason: None,
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    version: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                WorkCommandContext {
                    event_id: generated_id("work-event"),
                    performed_by_actor: next.host_actor.clone().ok_or_else(|| {
                        CliError::Usage("TeamRun has no exact Host AgentMember actor".to_string())
                    })?,
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: generated_id("work-command"),
                    created_at: now_string(),
                    duplicate_ok: false,
                },
            ))
        })
        .transpose()?;
    Ok((next, member_run, work))
}

pub(super) fn rename_team_run_member(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    name: &str,
) -> CliResult<ProviderRuntimeProjection> {
    if name.trim().is_empty() {
        return Err(CliError::Usage(
            "member display name must not be empty".to_string(),
        ));
    }
    let run = latest_team_run(store, team_run_id)?;
    team_run_execution_space_id(store, &run)?;
    if !run.member_run_ids.iter().any(|id| id == member_run_id) {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    let members = latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == team_run_id)
        .collect::<Vec<_>>();
    if members
        .iter()
        .any(|member| member.id != member_run_id && member.name == name)
    {
        return Err(CliError::Usage(format!(
            "team run {team_run_id} already has a member named {name}"
        )));
    }
    let mut member = members
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    if member.name == name {
        return Ok(member);
    }
    let expected = member.clone();
    let previous_name = member.name.clone();
    member.name = name.to_string();
    member.last_event_at = Some(now_string());
    store_conflict_as_usage(store.compare_and_append_member_run(&expected, &member))?;
    append_team_run_event(
        store,
        team_run_id,
        next_team_run_seq(store, team_run_id)?,
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "renamed",
        &format!("member {previous_name} renamed to {name}"),
    )?;
    Ok(member)
}

pub(super) fn deactivate_team_run_member(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    reason: &str,
) -> CliResult<ProviderRuntimeProjection> {
    if reason.trim().is_empty() {
        return Err(CliError::Usage(
            "member deactivation reason must not be empty".to_string(),
        ));
    }
    let run = latest_team_run(store, team_run_id)?;
    team_run_execution_space_id(store, &run)?;
    if !run.member_run_ids.iter().any(|id| id == member_run_id) {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    let mut member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    if member.coordination_is_retired() {
        return Ok(member);
    }
    if matches!(
        member.status,
        MemberRunStatus::Starting | MemberRunStatus::Running
    ) {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} is {}; interrupt its provider turn first, then deactivate it",
            serde_snake_label(&member.status)
        )));
    }
    if !member.is_external_interactive()
        && member.coordination_is_active()
        && member.native_session.is_some()
    {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} still has a managed runtime/session binding; close it first so the adapter process is released, then deactivate it"
        )));
    }
    let expected = member.clone();
    let now = now_string();
    member.coordination_status = MemberCoordinationStatus::Retired;
    member.status = MemberRunStatus::Stopped;
    member.last_event_at = Some(now.clone());
    member.finished_at = Some(now);
    store_conflict_as_usage(store.compare_and_append_member_run(&expected, &member))?;
    append_team_run_event(
        store,
        team_run_id,
        next_team_run_seq(store, team_run_id)?,
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "deactivated",
        &format!("member {} deactivated: {reason}", member.name),
    )?;
    Ok(member)
}
