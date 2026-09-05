use super::*;

pub(super) fn resolve_pi_bin() -> String {
    if let Ok(explicit) = std::env::var("PI_BIN") {
        if !explicit.trim().is_empty() {
            return explicit;
        }
    }
    let on_path = Command::new("which")
        .arg("pi")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if on_path {
        return "pi".into();
    }
    if let Some(home) = std::env::var_os("HOME") {
        // Check nvm node versions for npm global pi install.
        let nvm_bin = Path::new(&home).join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(&nvm_bin) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("bin/pi");
                if candidate.is_file() {
                    return candidate.display().to_string();
                }
            }
        }
    }
    "pi".into()
}

/// Drive one Pi Team Member through one pi RPC process and native session.
/// Retired `pi -p` print mode is not an alternative Agent Team Member mode.
pub(super) fn run_pi_team_member(
    ledger: &TeamRunLedger,
    objective: &str,
    member: &ProviderRuntimeProjection,
    context: &MemberRuntimeContext,
    transport_attempt: u64,
) -> CliResult<MemberOutcome> {
    use crate::runtime_adapter::TeamRuntimeAdapter as _;

    ledger.require_supervisor_lease()?;
    let project_id = context.project_id.as_deref();
    let project_selector = context.project_selector.as_deref();
    let cwd = &context.cwd;

    let mut member_row = member.clone();
    if let Some(profile) = member_row.provider_profile.as_mut() {
        ledger.require_supervisor_lease()?;
        apply_provider_version(profile, provider_version_output("pi").ok());
    }
    ledger.fold_event(
        TeamRunEventSourceKind::Member,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "updated",
        &format!(
            "member {} starting (pi rpc, cwd {})",
            member.name,
            cwd.display()
        ),
    )?;

    let envelope = member_work_collaboration_envelope(
        ledger,
        context.execution_space_id.as_deref(),
        project_id,
        project_selector,
        &member_row,
        None,
    )?;
    // Build the session directory path: <store_root>/pi_sessions/<member_run_id>/
    let session_base = ledger.store.root().join("pi_sessions");
    let session_dir = session_base.join(&member.id);
    std::fs::create_dir_all(&session_dir).map_err(|error| {
        CliError::Usage(format!(
            "failed to create pi session directory {}: {error}",
            session_dir.display()
        ))
    })?;

    let expected = member.clone();
    if member_row
        .provider_controls
        .reasoning_effort
        .requested
        .as_deref()
        .is_some_and(|effort| effort != "off")
    {
        member_row
            .provider_controls
            .reasoning_effort
            .mark_unsupported(
                "Pi persistent sessions replay provider thinking from native JSONL; the Team adapter forces --thinking off to satisfy the transient-only product policy",
            );
    } else if member_row
        .provider_controls
        .reasoning_effort
        .requested
        .as_deref()
        == Some("off")
    {
        member_row
            .provider_controls
            .reasoning_effort
            .mark_effective(
                Some("off".to_string()),
                "enforced by the reviewed Pi RPC launch contract",
            );
    }

    // Permission ceiling → compiled tool allowlist. The ceiling comes from
    // the canonical AgentMember trust record; the allowlist is actually
    // passed to the spawned process (a mapped-but-unlaunched string is not
    // enforcement). Full access intentionally compiles to no flag and the
    // profile records `none_verified` instead of pretending otherwise.
    let ceiling = ledger
        .store
        .all_trust_agent_members()?
        .into_iter()
        .find(|candidate| candidate.id == member_row.agent_member_id)
        .map(|record| record.permission_ceiling)
        .unwrap_or(harness_core::agentfirm_api::PermissionCeiling::WorkspaceWrite);
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    let ceiling = effective_member_permission_ceiling(&ledger.store, ceiling, &run, &member_row)?;
    let tools = crate::runtime_adapter::pi_tools_allowlist_for_ceiling(ceiling)?;
    if let Some(profile) = member_row.provider_profile.as_mut() {
        // Reuse the exact permission-aware resolution that was persisted
        // before AgentSession materialization. A generic re-finalize here
        // would resurrect FullAccess quiesce/release capabilities that the
        // durable profile intentionally marks pending, drifting the live
        // adapter composition away from the session fence.
        apply_permission_enforcement_to_profile(profile, ceiling)?;
    }

    let pi_bin = resolve_pi_bin();
    let resume_session_file = match member.native_session.as_ref() {
        Some(session) if Path::new(&session.native_session_id).is_file() => {
            Some(session.native_session_id.as_str())
        }
        Some(session) => {
            return Err(CliError::Usage(format!(
                "PI_NATIVE_SESSION_MISSING: refusing to replace missing resume session {} with a fresh session",
                session.native_session_id
            )))
        }
        None => None,
    };

    // Fence immediately before pi process start/resume.
    let process_effect =
        prepare_provider_process_effect_with_retry(ledger, &member_row, transport_attempt)?;
    let capability = collaboration_capability_envelope(
        ledger,
        &member_row,
        &process_effect.target_session,
        &context.role_action_token,
        harness_provider_pi::COLLABORATION_CAPABILITY_MECHANISM,
    )?;
    let capability_environment =
        harness_provider_pi::collaboration_agent_tool_environment(&capability)
            .map_err(|error| CliError::Usage(error.to_string()))?;
    let collaboration_env = envelope.environment(capability_environment);
    let profile = member_row.provider_profile.as_ref().ok_or_else(|| {
        CliError::Usage(format!(
            "RUNTIME_ADAPTER_PROFILE_MISSING: {} has no persisted provider profile",
            member_row.id
        ))
    })?;
    if let Err(error) = crate::runtime_adapter::preflight_profile_effect(
        profile,
        &process_effect.target_session,
        &process_effect.fence,
        crate::runtime_adapter_contract::SemanticCapability::OpenOrResume,
    ) {
        settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
        return Err(error);
    }
    let pi_client_result = pi_rpc::PiRpcClient::spawn(
        &pi_bin,
        pi_rpc::PiSpawnOptions {
            cwd,
            model: member.model.as_deref(),
            resume_session_file,
            session_dir: &session_dir,
            member_name: &member.name,
            collaboration_env: collaboration_env.as_pairs(),
            tools,
            permission_ceiling: ceiling,
        },
    );
    let pi_client = match pi_client_result {
        Ok(client) => client,
        Err(error) => {
            settle_provider_effect_not_applied(ledger, &process_effect, error.to_string())?;
            return Err(error.into());
        }
    };
    let mut adapter = pi_rpc::PiTeamRuntime::new(pi_client);
    adapter.bind_authority_session(process_effect.target_session.clone(), profile)?;
    let open_observation = match crate::runtime_adapter_contract::RuntimeAdapter::open_or_resume(
        &mut adapter,
        process_effect.fence.clone(),
        resume_session_file,
    ) {
        Ok(observation) => observation,
        Err(error) => {
            settle_provider_effect(
                ledger,
                &process_effect,
                ProviderEffectSettlement::UNPROVEN,
                None,
                Some(error.to_string()),
            )?;
            return Err(CliError::RuntimeRecoveryRequired(format!(
                "Pi open/resume could not be verified after spawn: {error}"
            )));
        }
    };
    settle_provider_effect(
        ledger,
        &process_effect,
        ProviderEffectSettlement::APPLIED_SATISFIED,
        Some(serde_json::json!({
            "provider": "pi",
            "phase": "runtime_attached",
            "observation": open_observation,
        })),
        None,
    )?;
    transition_provider_session_runtime_control(
        ledger,
        &member_row,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        harness_core::agentfirm_api::RuntimeActivity::Idle,
    )?;

    member_row.native_session = Some(native_session_ref(
        &member_row,
        adapter.native_session_locator(),
        adapter.native_locator_kind(),
    ));

    let (live_control, registration) = register_live_member_control(&member_row, &capability, 16);

    member_row.status = MemberRunStatus::Idle;
    member_row.last_event_at = Some(now_string());
    ledger.save_member_run(&expected, &member_row)?;

    // The generic provider-neutral loop owns wake → claim → cycle → settle.
    crate::runtime_adapter::run_team_member_with_adapter(
        ledger,
        objective,
        &mut member_row,
        context,
        &mut adapter,
        &live_control,
        Some(registration),
        transport_attempt,
    )
}

/// Journal a member failure on any error path (best-effort: we are already on
/// the failure path, so secondary journaling errors are dropped).
pub(super) fn journal_member_failure(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    reason: &str,
) {
    let mut failed = ledger
        .latest_member_run(&member.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| member.clone());
    let expected = failed.clone();
    failed.status = MemberRunStatus::Failed;
    failed.finished_at = Some(now_string());
    failed.last_event_at = Some(now_string());
    let _ = ledger.save_member_run(&expected, &failed);
    let _ = ledger.append_action(
        &member.id,
        "error",
        MemberActionStatus::Failed,
        "member failed",
        reason,
    );
    let _ = ledger.fold_event(
        TeamRunEventSourceKind::Member,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "updated",
        &format!("member {} failed: {reason}", member.name),
    );
}

pub(super) fn journal_member_disconnected(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    generation: u64,
    reason: &str,
) {
    let mut disconnected = ledger
        .latest_member_run(&member.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| member.clone());
    if disconnected.status == MemberRunStatus::Stopped {
        return;
    }
    let expected = disconnected.clone();
    disconnected.status = MemberRunStatus::Disconnected;
    disconnected.finished_at = None;
    disconnected.last_event_at = Some(now_string());
    let _ = ledger.save_member_run(&expected, &disconnected);
    let _ = ledger.append_action(
        &member.id,
        "disconnected",
        MemberActionStatus::Progress,
        "provider transport disconnected; supervisor will resume",
        &format!("runtime generation {generation}: {reason}"),
    );
    let _ = ledger.fold_event(
        TeamRunEventSourceKind::Member,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "updated",
        &format!(
            "member {} disconnected in runtime generation {generation}; native session retained",
            member.name
        ),
    );
}

/// Flip every queued delivery of `message` addressed to `member_id` to
/// delivered (append a new TeamMessageProjection row — the store is latest-wins) and
/// fold the delivery event.
pub(super) fn mark_message_delivered(
    ledger: &TeamRunLedger,
    message: &TeamMessageProjection,
    member_id: &str,
    member_name: &str,
    provider_receipt_id: &str,
) -> CliResult<()> {
    ledger.require_supervisor_lease()?;
    let canonical_delivery_id = message
        .evidence_refs
        .iter()
        .find_map(|reference| reference.strip_prefix(CANONICAL_MESSAGE_DELIVERY_REF));
    let canonical_execution_space = message
        .evidence_refs
        .iter()
        .find_map(|reference| reference.strip_prefix(CANONICAL_EXECUTION_SPACE_REF));
    if let (Some(delivery_id), Some(execution_space_id)) =
        (canonical_delivery_id, canonical_execution_space)
    {
        let claimed = ledger
            .store
            .fabric_message_deliveries(execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "canonical RegistryDeliveryAttempt disappeared before provider receipt: {delivery_id}"
                ))
            })?;
        let claim_id = claimed.claim_id.clone().ok_or_else(|| {
            CliError::Usage(format!(
                "canonical RegistryDeliveryAttempt {delivery_id} has no active claim"
            ))
        })?;
        let session_generation = claimed.recipient_session_generation.ok_or_else(|| {
            CliError::Usage(format!(
                "canonical delivery {delivery_id} has no recipient session generation"
            ))
        })?;
        let daemon_generation = claimed.claimed_node_daemon_generation.ok_or_else(|| {
            CliError::Usage(format!(
                "canonical delivery {delivery_id} has no NodeDaemon generation"
            ))
        })?;
        let session_id = claimed.recipient_session_id.clone().ok_or_else(|| {
            CliError::Usage(format!(
                "canonical delivery {delivery_id} has no recipient session"
            ))
        })?;
        let session = ledger
            .store
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .find(|session| {
                session.id == session_id && session.runtime_generation == session_generation
            })
            .ok_or_else(|| CliError::Usage("AGENT_SESSION_GENERATION_FENCED".into()))?;
        let received = ledger.store.record_message_provider_receipt(
            &canonical_delivery_context(
                execution_space_id,
                &session.node_daemon_id,
                "node_daemon.message_delivery.provider_received",
                format!("{claim_id}:provider-received"),
                0,
            ),
            delivery_id,
            &session.node_id,
            &session.node_daemon_id,
            daemon_generation,
            &claim_id,
            provider_receipt_id,
            &now_string(),
        )?;
        let stable_member_id = ledger
            .latest_member_run(member_id)?
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_id}")))?
            .agent_member_id;
        ledger.store.acknowledge_message_delivery(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: execution_space_id.to_string(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                    id: stable_member_id,
                },
                authority_actor: None,
                command_name: "agent_session.message_delivery.acknowledge".into(),
                idempotency_key: format!("{claim_id}:acknowledge"),
                expected_version: 0,
                request_fingerprint: None,
            },
            delivery_id,
            &now_string(),
        )?;
        ledger.fold_event(
            TeamRunEventSourceKind::Member,
            Some(member_id.to_string()),
            "message_delivery",
            delivery_id,
            "acknowledged",
            &format!(
                "canonical TeamMessageProjection {} accepted by provider for {} ({provider_receipt_id})",
                message.id, member_name
            ),
        )?;
        let _ = received;
        return Ok(());
    }
    Err(CliError::Usage(format!(
        "RETIRED_RUNTIME_READER: provider inbox item {} lacks a canonical MessageDelivery reference",
        message.id
    )))
}

/// Acknowledge one delivery and fold the state change into the TeamRun event
/// stream. This is shared by Host-facing transports so ACK is durable,
/// idempotent, and visible in the same audit trail as the message itself.
#[cfg(any())]
pub(crate) fn acknowledge_team_message(
    store: &HarnessStore,
    message_id: &str,
    member_id: &str,
) -> CliResult<TeamMessageProjection> {
    let _ = store;
    Err(CliError::Usage(format!(
        "RETIRED_RUNTIME_WRITER: legacy Team message acknowledgement for {message_id}/{member_id} is closed; use canonical MessageDelivery acknowledgement"
    )))
}

/// Where a member's ACP session runs: its pinned worktree when set, else the
/// selected project's root, else (unrouted raw-store invocation) the CLI cwd.
pub(super) fn member_spawn_cwd(
    project_context: Option<&ProjectContext>,
    run: &AgentTeamRun,
    member: &ProviderRuntimeProjection,
) -> PathBuf {
    if let Some(worktree) = &member.provider_cwd_hint {
        if !worktree.is_empty() {
            return PathBuf::from(worktree);
        }
    }
    if let Some(execution_root) = &run.execution_root {
        if !execution_root.is_empty() {
            return PathBuf::from(execution_root);
        }
    }
    if let Some(context) = project_context {
        return context.project_root.clone();
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub(super) fn git_value(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

pub(super) fn snapshot_member_workspace(
    cwd: &Path,
    project_binding_id: Option<&str>,
    project_root: Option<&Path>,
    resolution_source: &str,
) -> MemberWorkspaceSnapshot {
    let cwd = project::canonicalize_best_effort(cwd);
    let git_root = git_value(&cwd, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .map(|path| project::canonicalize_best_effort(&path));
    let project_root = project_root.map(project::canonicalize_best_effort);
    let discovery_boundary = git_root
        .filter(|root| cwd.starts_with(root))
        .or_else(|| project_root.filter(|root| cwd.starts_with(root)))
        .unwrap_or_else(|| cwd.clone());
    let mut instruction_roots = BTreeSet::new();
    let mut skill_roots = BTreeSet::new();
    for ancestor in cwd.ancestors() {
        if ["AGENTS.md", "CLAUDE.md"]
            .iter()
            .any(|name| ancestor.join(name).is_file())
        {
            instruction_roots.insert(ancestor.display().to_string());
        }
        for relative in [".agents/skills", ".codex/skills", "skills"] {
            let candidate = ancestor.join(relative);
            if candidate.is_dir() {
                skill_roots.insert(
                    project::canonicalize_best_effort(&candidate)
                        .display()
                        .to_string(),
                );
            }
        }
        if ancestor == discovery_boundary {
            break;
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for relative in [".agents/skills", ".codex/skills"] {
            let candidate = home.join(relative);
            if candidate.is_dir() {
                skill_roots.insert(
                    project::canonicalize_best_effort(&candidate)
                        .display()
                        .to_string(),
                );
            }
        }
    }
    MemberWorkspaceSnapshot {
        cwd: cwd.display().to_string(),
        project_binding_id: project_binding_id.map(ToString::to_string),
        resolution_source: Some(resolution_source.to_string()),
        git_head: git_value(&cwd, &["rev-parse", "HEAD"]),
        git_branch: git_value(&cwd, &["symbolic-ref", "--short", "HEAD"]),
        instruction_roots: instruction_roots.into_iter().collect(),
        skill_roots: skill_roots.into_iter().collect(),
    }
}
