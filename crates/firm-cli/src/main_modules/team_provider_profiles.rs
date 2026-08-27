use super::*;

/// One member spec for team-run creation, parsed from either the CLI
/// `--member name:role:provider[/mode][:model][@path1,path2]` spelling or the
/// HTTP JSON body. `/mode` selects the execution mode; the driven Agent Team
/// modes are `codex_app_server`, `kimi_acp`, `claude_agent_sdk`,
/// `pi_rpc`, `deepseek_sdk`, and
/// `external_interactive` declares the user's own already-open interactive
/// session that Harness never spawns or drives (it polls its own inbox).
#[derive(Clone)]
pub(super) struct TeamMemberSpec {
    pub(super) agent_member_id: String,
    pub(super) name: String,
    pub(super) role: String,
    pub(super) provider: String,
    pub(super) execution_mode: Option<String>,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) service_tier: Option<String>,
    pub(super) provider_cwd_hint: Option<String>,
    pub(super) owned_paths: Vec<String>,
    pub(super) resume_native_session_id: Option<String>,
    /// This member's own brief. `objective` is the run-level intent; when every
    /// member's assignment is seeded from it, a multi-lane objective is
    /// delivered verbatim to everyone and each member reads every other
    /// member's brief on its first tokens -- the one cost a member is
    /// guaranteed to pay and cannot amortise. Measured on a 2-lane review run:
    /// both members received the full objective including the other's
    /// questions. None keeps the historical behaviour.
    pub(super) initial_work: Option<String>,
}

pub(super) fn parse_host_runtime_mode(value: Option<&str>) -> CliResult<HostControlMode> {
    match value.unwrap_or("managed") {
        "managed" => Ok(HostControlMode::Managed),
        "external_interactive" | "external" => Ok(HostControlMode::ExternalInteractive),
        other => Err(CliError::Usage(format!(
            "unknown host_runtime_mode `{other}` (managed|external_interactive)"
        ))),
    }
}

/// Apply the Team-level Host ownership mode to the one Host AgentMember spec.
/// Ordinary members keep their independently selected runtime modes.
pub(super) fn apply_host_runtime_mode(
    store: &HarnessStore,
    execution_space_id: &str,
    team_id: &str,
    members: &mut [TeamMemberSpec],
    mode: HostControlMode,
) -> CliResult<()> {
    let team = store
        .agent_teams(execution_space_id)?
        .into_iter()
        .find(|team| team.id == team_id)
        .ok_or_else(|| CliError::Usage(format!("team not found: {team_id}")))?;
    let matching = members
        .iter()
        .enumerate()
        .filter_map(|(index, member)| {
            (member.agent_member_id == team.host_agent_id).then_some(index)
        })
        .collect::<Vec<_>>();
    let [host_index] = matching.as_slice() else {
        return Err(CliError::Usage(format!(
            "AgentTeam {team_id} requires exactly one Host AgentMember runtime spec; found {}",
            matching.len()
        )));
    };
    let host = &mut members[*host_index];
    match mode {
        HostControlMode::Managed => {
            if host.execution_mode.as_deref() == Some(EXECUTION_MODE_EXTERNAL_INTERACTIVE) {
                host.execution_mode = None;
            }
        }
        HostControlMode::ExternalInteractive => {
            host.execution_mode = Some(EXECUTION_MODE_EXTERNAL_INTERACTIVE.to_string());
            host.resume_native_session_id = None;
        }
    }
    Ok(())
}

pub(super) fn configure_host_runtime_mode(
    store: &HarnessStore,
    execution_space_id: &str,
    team_id: &str,
    members: &mut [TeamMemberSpec],
    requested_mode: Option<&str>,
) -> CliResult<HostControlMode> {
    let mode = parse_host_runtime_mode(requested_mode)?;
    apply_host_runtime_mode(store, execution_space_id, team_id, members, mode)?;
    Ok(mode)
}

pub(super) fn team_member_specs_from_definition(
    store: &HarnessStore,
    execution_space_id: &str,
    team_id: &str,
) -> CliResult<Vec<TeamMemberSpec>> {
    let team = store
        .agent_teams(execution_space_id)?
        .into_iter()
        .find(|team| team.id == team_id)
        .ok_or_else(|| CliError::Usage(format!("team not found: {team_id}")))?;
    let members = store
        .trust_agent_members(execution_space_id)?
        .into_iter()
        .map(|member| (member.id.clone(), member))
        .collect::<BTreeMap<_, _>>();
    let mut memberships = store
        .fabric_team_memberships(execution_space_id)?
        .into_iter()
        .filter(|membership| {
            membership.team_id == team.id
                && membership.node_id == team.node_id
                && membership.state == harness_core::agentfirm_api::TeamMembershipStatus::Active
                && membership.role != harness_core::agentfirm_api::TeamMembershipRole::Observer
        })
        .collect::<Vec<_>>();
    memberships.sort_by_key(|membership| {
        (
            membership.role != harness_core::agentfirm_api::TeamMembershipRole::Host,
            membership.id.clone(),
        )
    });
    memberships
        .into_iter()
        .map(|membership| {
            let member = members.get(&membership.agent_member_id).ok_or_else(|| {
                CliError::Usage(format!(
                    "team {team_id} membership {} references missing AgentMember {}",
                    membership.id, membership.agent_member_id
                ))
            })?;
            Ok(TeamMemberSpec {
                agent_member_id: member.id.clone(),
                name: member.name.clone(),
                role: member.role.clone(),
                provider: member
                    .provider_profile_ref
                    .as_deref()
                    .and_then(|profile| profile.split('/').next())
                    .unwrap_or("codex")
                    .to_string(),
                execution_mode: None,
                model: member.model_preference.clone(),
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            })
        })
        .collect()
}

pub(super) fn team_member_provider_profile(provider: &str) -> ProviderIntegrationProfile {
    team_member_provider_profile_for_mode(provider, None)
}

pub(super) fn validate_team_member_execution_mode(member: &TeamMemberSpec) -> CliResult<()> {
    if member.provider == "codex" && member.execution_mode.as_deref() == Some("codex_exec") {
        return Err(CliError::Usage(
            "codex_exec is retired; Agent Team Codex members use codex_app_server".to_string(),
        ));
    }
    if member.provider == "claude" && member.execution_mode.as_deref() == Some("claude_cli") {
        return Err(CliError::Usage(
            "claude_cli is retired; Agent Team Claude members use claude_agent_sdk".to_string(),
        ));
    }
    if member.execution_mode.as_deref() == Some(EXECUTION_MODE_EXTERNAL_INTERACTIVE) {
        // An external interactive member is the user's own already-open
        // provider session; there is no provider-native session to resume and
        // no Harness adapter whose registry should constrain the provider
        // label. Known providers may use the plugin hook while any other
        // non-empty label remains usable through the same trusted-local
        // inbox/send/ack contract.
        if member.resume_native_session_id.is_some() {
            return Err(CliError::Usage(
                "external_interactive members have no provider-native session to resume"
                    .to_string(),
            ));
        }
        return Ok(());
    }
    if let Some(mode) = member.execution_mode.as_deref() {
        if crate::runtime_adapter::shared_team_runtime_kind(member.provider.as_str(), Some(mode))
            .is_none()
        {
            return Err(CliError::Usage(format!(
                "execution mode {mode} is not registered for provider {}",
                member.provider
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_team_member_identity(
    store: &HarnessStore,
    member: &TeamMemberSpec,
) -> CliResult<()> {
    let agent_member_id = member.agent_member_id.as_str();
    if agent_member_id.trim().is_empty() {
        return Err(CliError::Usage(
            "team member agent_member_id must not be empty".to_string(),
        ));
    }
    let durable = store
        .all_trust_agent_members()?
        .into_iter()
        .find(|candidate| {
            candidate.id == agent_member_id
                && candidate.organization_status
                    == harness_core::agentfirm_api::AgentMemberOrganizationStatus::Active
        });
    let Some(durable) = durable else {
        return Err(CliError::Usage(format!(
            "team member {} references missing canonical AgentMember {agent_member_id}",
            member.name
        )));
    };
    if member.execution_mode.as_deref() != Some(EXECUTION_MODE_EXTERNAL_INTERACTIVE)
        && durable.permission_ceiling != harness_core::agentfirm_api::PermissionCeiling::FullAccess
    {
        return Err(CliError::Usage(format!(
            "TRUSTED_DEVELOPMENT_FULL_ACCESS_REQUIRED: managed coding AgentMember {} is frozen to {:?}; create a new FullAccess AgentMember/Session instead of widening it in place",
            durable.id, durable.permission_ceiling
        )));
    }
    Ok(())
}

/// Resolved-composition fingerprint for a ProviderIntegrationProfile
/// (DOC-89 §11.1): pins provider + execution mode + adapter contract/bridge
/// revision. External-protocol adapters have no plugin tree to resolve yet;
/// when a native bridge exists, its resolved composition joins this input.
pub(super) fn profile_composition_fingerprint(
    provider: &str,
    execution_mode: &str,
    adapter_contract_version: Option<&str>,
) -> Option<String> {
    adapter_contract_version.map(|contract| {
        harness_store::canonical_json_fingerprint(&serde_json::json!({
            "fingerprint_kind": "agentfirm.runtime_composition.v1",
            "provider": provider,
            "execution_mode": execution_mode,
            "adapter_contract_version": contract,
        }))
    })
}

pub(super) fn profile_capability_fingerprint(
    profile: &ProviderIntegrationProfile,
) -> Option<String> {
    Some(harness_store::canonical_json_fingerprint(
        &serde_json::json!({
            "fingerprint_kind": "agentfirm.provider_capabilities.v1",
            "agent_runtime_provider": profile
                .agent_runtime_provider
                .as_ref()
                .map(|provider| provider.0.as_str())
                .unwrap_or(profile.provider.as_str()),
            "execution_mode": profile.execution_mode,
            "provider_version": profile.provider_version,
            "adapter_contract_version": profile.adapter_contract_version,
            "adapter_bridge_revision": profile.adapter_bridge_revision,
            "security_enforcement_locus": profile.security_enforcement_locus,
            "binding_admission": profile.binding_admission,
            "capability_bindings": profile.capability_bindings,
        }),
    ))
}

/// Commit to the exact executable runtime composition, not merely the brand
/// and transport name. This is recomputed after every version, permission
/// locus, model-route, or capability refresh; commands carrying the old hash
/// are rejected before crossing the provider boundary.
pub(super) fn resolved_profile_composition_fingerprint(
    profile: &ProviderIntegrationProfile,
) -> Option<String> {
    profile.adapter_contract_version.as_ref()?;
    Some(harness_store::canonical_json_fingerprint(
        &serde_json::json!({
            "fingerprint_kind": "agentfirm.runtime_composition.v2",
            "agent_runtime_provider": profile
                .agent_runtime_provider
                .as_ref()
                .map(|provider| provider.0.as_str())
                .unwrap_or(profile.provider.as_str()),
            "execution_mode": profile.execution_mode,
            "provider_version": profile.provider_version,
            "adapter_contract_version": profile.adapter_contract_version,
            "adapter_bridge_revision": profile.adapter_bridge_revision,
            "control_topology": profile.control_topology,
            "execution_driver": profile.execution_driver,
            "model_route": profile.model_route,
            "interaction_mode": profile.interaction_mode,
            "ordinary_message_boundary": profile.ordinary_message_boundary,
            "plan_mode": profile.plan_mode,
            "goal_mode": profile.goal_mode,
            "security_enforcement_locus": profile.security_enforcement_locus,
            "capability_fingerprint": profile.capability_fingerprint,
        }),
    ))
}

pub(super) fn finalize_provider_integration_profile(profile: &mut ProviderIntegrationProfile) {
    if profile.agent_runtime_provider.is_none() {
        profile.agent_runtime_provider =
            Some(harness_core::AgentRuntimeProvider(profile.provider.clone()));
    }
    let executable_bindings = match (profile.provider.as_str(), profile.execution_mode.as_str()) {
        ("pi", "pi_rpc")
        | ("kimi", "kimi_acp")
        | ("codex", "codex_app_server")
        | ("claude", "claude_agent_sdk")
        | ("deepseek_harness", "deepseek_sdk") => {
            crate::runtime_adapter::capability_bindings_for(&profile.provider)
        }
        _ => None,
    };
    if let Some(bindings) = executable_bindings {
        let adapter_revision = profile
            .adapter_bridge_revision
            .clone()
            .or_else(|| profile.adapter_contract_version.clone());
        profile.capability_bindings = bindings
                .into_iter()
                .map(|binding| {
                    let live_canary_ref = match (
                        profile.provider.as_str(),
                        binding.capability,
                        profile.provider_version.as_deref(),
                    ) {
                        ("codex", "open_or_resume", Some("0.148.0-alpha.9")) =>
                            Some("live:DEV-26:codex_app_server@0.148.0-alpha.9:thread-new+exact-thread-resume+effective-permission-receipt"
                                .to_string()),
                        ("codex", "start_cycle", Some("0.148.0-alpha.9")) =>
                            Some("live:DEV-26:codex_app_server@0.148.0-alpha.9:turn-start+matching-completed+thread-idle"
                                .to_string()),
                        ("codex", "interrupt_current_cycle", Some("0.148.0-alpha.9")) =>
                            Some("live:DEV-26:codex_app_server@0.148.0-alpha.9:command-start+turn-interrupt+matching-interrupted+thread-idle"
                                .to_string()),
                        ("codex", "close_runtime", Some("0.148.0-alpha.9")) =>
                            Some("live:DEV-26:codex_app_server@0.148.0-alpha.9:owned-process-reap+thread-retained+reopen"
                                .to_string()),
                        ("codex", "observe", Some("0.148.0-alpha.9")) =>
                            Some("live:DEV-26:codex_app_server@0.148.0-alpha.9:thread-read+transport-liveness"
                                .to_string()),
                        ("kimi", "open_or_resume", Some("0.36.1")) =>
                            Some("live:DEV-26:kimi_acp@0.36.1:session-new+same-session-resume"
                                .to_string()),
                        ("kimi", "start_cycle", Some("0.36.1")) =>
                            Some("live:DEV-26:kimi_acp@0.36.1:k3+max+prompt+end_turn"
                                .to_string()),
                        ("kimi", "interrupt_current_cycle", Some("0.36.1")) =>
                            Some("live:DEV-26:kimi_acp@0.36.1:session-cancel+cancelled"
                                .to_string()),
                        ("kimi", "close_runtime", Some("0.36.1")) =>
                            Some("live:DEV-26:kimi_acp@0.36.1:session-close+clean-reap:session_4d61b18d-4e9c-4640-8ee0-d00c5ceb6f49"
                                .to_string()),
                        ("kimi", "observe", Some("0.36.1")) =>
                            Some("live:DEV-26:kimi_acp@0.36.1:owned-process+transport-liveness"
                                .to_string()),
                        ("pi", "open_or_resume", Some("0.84.2")) =>
                            Some("live:DEV-26:pi_rpc@0.84.2:session-2026-08-16T01-23-34-207Z_01a0082a-bebf-72d6-8a0e-2d8f8afac173:new+exact-session-resume"
                                .to_string()),
                        ("pi", "start_cycle", Some("0.84.2")) =>
                            Some("live:DEV-26:pi_rpc@0.84.2:prompt-accepted+agent-settled+get-state-idle+no-persisted-thinking"
                                .to_string()),
                        ("pi", "interrupt_current_cycle", Some("0.84.2")) =>
                            Some("live:DEV-26:pi_rpc@0.84.2:session-2026-08-16T03-58-33-028Z_01a008b8-a244-79e1-a65f-fa26d56accde:busy-bash+abort-receipt+agent-settled+get-state-idle"
                                .to_string()),
                        ("pi", "close_runtime", Some("0.84.2")) =>
                            Some("live:DEV-26:pi_rpc@0.84.2:session-2026-08-16T03-58-33-028Z_01a008b8-a244-79e1-a65f-fa26d56accde:owned-process-reaped+native-session-retained"
                                .to_string()),
                        ("pi", "observe", Some("0.84.2")) =>
                            Some("live:DEV-26:pi_rpc@0.84.2:get-state-before-and-after-exact-session-resume"
                                .to_string()),
                        ("claude", "open_or_resume", Some("2.1.220")) =>
                            Some("live:2026-07-28:claude_agent_sdk@2.1.220:team-run-1785230417407-p72711-0:member-run-1785230417407-p72711-1:session-ec91628d-a514-4d40-ae9c-7f73ecf3c40f:exact-session-open+same-session-resume"
                                .to_string()),
                        ("claude", "start_cycle", Some("2.1.220")) =>
                            Some("live:2026-07-28:claude_agent_sdk@2.1.220:team-run-1785230417407-p72711-0:member-run-1785230417407-p72711-1:session-ec91628d-a514-4d40-ae9c-7f73ecf3c40f:two-host-rounds+matching-completion"
                                .to_string()),
                        ("claude", "interrupt_current_cycle", Some("2.1.220")) =>
                            Some("live:DEV-26:2026-08-16:claude_agent_sdk@2.1.220:sdk@0.3.220:session-3590068d-b58c-4a90-852c-8c38b7de0250:query-interrupt+query-close+same-session-resume"
                                .to_string()),
                        ("claude", "close_runtime", Some("2.1.220")) =>
                            Some("live:2026-07-28:claude_agent_sdk@2.1.220:team-run-1785230417407-p72711-0:member-run-1785230417407-p72711-1:session-ec91628d-a514-4d40-ae9c-7f73ecf3c40f:explicit-host-close+session-retained"
                                .to_string()),
                        ("claude", "observe", Some("2.1.220")) =>
                            Some("live:2026-07-28:claude_agent_sdk@2.1.220:team-run-1785230417407-p72711-0:member-run-1785230417407-p72711-1:session-ec91628d-a514-4d40-ae9c-7f73ecf3c40f:two-round-runtime-lifecycle+listSessions+native-jsonl"
                                .to_string()),
                        ("deepseek_harness", "open_or_resume", Some("0.1.1-rc.2")) =>
                            Some("live:DEV-63:deepseek_sdk@0.1.1-rc.2+b150a551:star-3b69a281-44a0-4068-87a6-02d355f434d9:create+exact-session-resume"
                                .to_string()),
                        ("deepseek_harness", "start_cycle", Some("0.1.1-rc.2")) =>
                            Some("live:DEV-63:deepseek_sdk@0.1.1-rc.2+b150a551:dev63-proxy-input-1+dev63-proxy-input-2:matching-inbox-splice+completed"
                                .to_string()),
                        ("deepseek_harness", "interrupt_current_cycle", Some("0.1.1-rc.2")) =>
                            Some("live:DEV-63:deepseek_sdk@0.1.1-rc.2+b150a551:star-dc77b84b-fa18-48e6-9d00-6f45e175137c:dev63-final-interrupt-input+cancel+same-session-idle"
                                .to_string()),
                        ("deepseek_harness", "close_runtime", Some("0.1.1-rc.2")) =>
                            Some("live:DEV-63:deepseek_sdk@0.1.1-rc.2+b150a551:member_closed+owned-runner-exit+native-session-retained"
                                .to_string()),
                        ("deepseek_harness", "observe", Some("0.1.1-rc.2")) =>
                            Some("live:DEV-63:deepseek_sdk@0.1.1-rc.2+b150a551:owned-runner+typed-session-events+transport-liveness"
                                .to_string()),
                        _ => None,
                    };
                    let (status, admission) = match binding.status {
                        crate::runtime_adapter::CapabilityStatus::Supported
                            if live_canary_ref.is_some() => (
                            harness_core::ProviderCapabilityStatus::Verified,
                            harness_core::ProviderBindingAdmission::Active,
                        ),
                        crate::runtime_adapter::CapabilityStatus::Supported => (
                            harness_core::ProviderCapabilityStatus::ReviewRequired,
                            harness_core::ProviderBindingAdmission::PendingDependency,
                        ),
                        crate::runtime_adapter::CapabilityStatus::Degraded => (
                            harness_core::ProviderCapabilityStatus::Degraded,
                            harness_core::ProviderBindingAdmission::Degraded,
                        ),
                        crate::runtime_adapter::CapabilityStatus::Experimental => (
                            harness_core::ProviderCapabilityStatus::ReviewRequired,
                            harness_core::ProviderBindingAdmission::PendingDependency,
                        ),
                        crate::runtime_adapter::CapabilityStatus::Unsupported => (
                            harness_core::ProviderCapabilityStatus::Unsupported,
                            harness_core::ProviderBindingAdmission::Failed,
                        ),
                    };
                    let feature_fingerprint =
                        harness_store::canonical_json_fingerprint(&serde_json::json!({
                            "provider": profile.provider,
                            "execution_mode": profile.execution_mode,
                            "provider_version": profile.provider_version,
                            "adapter_revision": adapter_revision,
                            "capability": binding.capability,
                            "status": status,
                            "admission": admission,
                            "evidence": binding.evidence,
                            "security_enforcement_locus": binding.security_enforcement_locus,
                        }));
                    let required_dependencies = match binding.capability {
                        "start_cycle" => vec!["open_or_resume", "observe"],
                        "inject_current_cycle"
                        | "queue_at_native_boundary"
                        | "interrupt_current_cycle" => vec!["observe"],
                        "close_runtime" => vec!["open_or_resume", "observe"],
                        "quiesce" => vec!["interrupt_current_cycle", "observe"],
                        "release" => vec!["quiesce"],
                        _ => Vec::new(),
                    }
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                    let mut evidence = vec![harness_core::ProviderCapabilityEvidence {
                        kind: harness_core::ProviderCapabilityEvidenceKind::SourceReview,
                        evidence_ref: binding.evidence,
                        observed_at: profile.adapter_reviewed_at.clone(),
                        note: binding.security_enforcement_locus,
                    }];
                    if binding.status == crate::runtime_adapter::CapabilityStatus::Supported {
                        evidence.push(harness_core::ProviderCapabilityEvidence {
                            kind: harness_core::ProviderCapabilityEvidenceKind::DeterministicAcceptance,
                            evidence_ref: format!(
                                "test:{}_runtime_adapter:{}",
                                profile.provider, binding.capability
                            ),
                            observed_at: profile.adapter_reviewed_at.clone(),
                            note: Some(
                                "provider-neutral preflight plus provider-specific deterministic journeys exercise this semantic binding"
                                    .to_string(),
                            ),
                        });
                        if let Some(live_canary_ref) = live_canary_ref {
                            evidence.push(harness_core::ProviderCapabilityEvidence {
                                kind: harness_core::ProviderCapabilityEvidenceKind::LiveCanary,
                                evidence_ref: live_canary_ref,
                                observed_at: profile.adapter_reviewed_at.clone(),
                                note: Some(
                                    "exact-version live transport evidence; capability semantics remain bound to their deterministic test"
                                        .to_string(),
                                ),
                            });
                        }
                    }
                    harness_core::ProviderCapabilityBinding {
                        capability: binding.capability.to_string(),
                        status,
                        admission,
                        provider_version: profile.provider_version.clone(),
                        adapter_revision: adapter_revision.clone(),
                        feature_fingerprint: Some(feature_fingerprint),
                        required_dependencies,
                        evidence,
                    }
                })
                .collect();
        profile.binding_admission = if profile
            .capability_bindings
            .iter()
            .any(|binding| binding.admission != harness_core::ProviderBindingAdmission::Active)
        {
            harness_core::ProviderBindingAdmission::Degraded
        } else {
            harness_core::ProviderBindingAdmission::Active
        };
        // Legacy aggregate booleans remain on the wire, but executable Team
        // modes may advertise them only when the exact versioned semantic
        // binding is admitted. UI/API code must not bypass a pending binding
        // merely because the provider protocol has such a method in theory.
        profile.supports_cancel =
            has_active_verified_provider_capability(profile, "interrupt_current_cycle");
        profile.supports_resume =
            has_active_verified_provider_capability(profile, "open_or_resume");
    }
    profile.capability_fingerprint = profile_capability_fingerprint(profile);
    profile.composition_fingerprint = resolved_profile_composition_fingerprint(profile);
}

pub(super) fn has_active_verified_provider_capability(
    profile: &ProviderIntegrationProfile,
    capability: &str,
) -> bool {
    profile.capability_bindings.iter().any(|binding| {
        binding.capability == capability
            && binding.status == harness_core::ProviderCapabilityStatus::Verified
            && binding.admission == harness_core::ProviderBindingAdmission::Active
    })
}

pub(super) fn finalized_provider_integration_profile(
    mut profile: ProviderIntegrationProfile,
) -> ProviderIntegrationProfile {
    finalize_provider_integration_profile(&mut profile);
    profile
}

pub(super) fn apply_permission_enforcement_to_profile(
    profile: &mut ProviderIntegrationProfile,
    ceiling: harness_core::agentfirm_api::PermissionCeiling,
) -> CliResult<()> {
    if profile.provider == "pi" && profile.execution_mode == "pi_rpc" {
        // This is both an argv compilation check and an admission gate:
        // WorkspaceWrite cannot be represented by Pi without an external
        // filesystem boundary, while ReadOnly and explicit FullAccess can.
        let _ = crate::runtime_adapter::pi_tools_allowlist_for_ceiling(ceiling)?;
        profile.security_enforcement_locus =
            crate::runtime_adapter::pi_security_enforcement_locus(ceiling);
    }
    finalize_provider_integration_profile(profile);
    if profile.provider == "pi"
        && profile.execution_mode == "pi_rpc"
        && ceiling == harness_core::agentfirm_api::PermissionCeiling::FullAccess
    {
        for binding in profile
            .capability_bindings
            .iter_mut()
            .filter(|binding| matches!(binding.capability.as_str(), "quiesce" | "release"))
        {
            binding.status = harness_core::ProviderCapabilityStatus::ReviewRequired;
            binding.admission = harness_core::ProviderBindingAdmission::PendingDependency;
            binding.evidence.retain(|evidence| {
                evidence.kind == harness_core::ProviderCapabilityEvidenceKind::SourceReview
            });
            binding.evidence.push(harness_core::ProviderCapabilityEvidence {
                kind: harness_core::ProviderCapabilityEvidenceKind::SourceReview,
                evidence_ref:
                    "gap:pi_full_access_requires_native_job_inventory_or_os_containment"
                        .to_string(),
                observed_at: profile.adapter_reviewed_at.clone(),
                note: Some(
                    "Pi RPC cannot prove that FullAccess background writers are drained; quiesce/release are denied before provider effect"
                        .to_string(),
                ),
            });
            binding.feature_fingerprint = Some(harness_store::canonical_json_fingerprint(
                &serde_json::json!({
                    "provider": profile.provider,
                    "execution_mode": profile.execution_mode,
                    "provider_version": profile.provider_version,
                    "adapter_revision": profile.adapter_bridge_revision,
                    "capability": binding.capability,
                    "status": binding.status,
                    "admission": binding.admission,
                    "evidence": binding.evidence,
                    "security_enforcement_locus": profile.security_enforcement_locus,
                }),
            ));
        }
        profile.binding_admission = harness_core::ProviderBindingAdmission::Degraded;
        profile.capability_fingerprint = profile_capability_fingerprint(profile);
        profile.composition_fingerprint = resolved_profile_composition_fingerprint(profile);
    }
    Ok(())
}

/// Resolve the exact permission ceiling for one TeamRun participant.
pub(super) fn agent_session_control_state_for_profile(
    profile: Option<&ProviderIntegrationProfile>,
    daemon_id: &str,
    daemon_generation: u64,
    runtime_generation: u64,
) -> harness_core::agentfirm_api::AgentSessionControlState {
    harness_core::agentfirm_api::AgentSessionControlState {
        runtime_residency: harness_core::agentfirm_api::RuntimeResidency::Detached,
        activity: harness_core::agentfirm_api::RuntimeActivity::Idle,
        execution_driver: profile
            .map(|profile| profile.execution_driver)
            .unwrap_or(MemberExecutionDriver::HostDriven),
        driver_generation: runtime_generation.max(1),
        driver_ref: harness_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
        },
        composition_fingerprint: profile
            .and_then(|profile| profile.composition_fingerprint.clone()),
        capability_fingerprint: profile.and_then(|profile| profile.capability_fingerprint.clone()),
        ..Default::default()
    }
}

pub(super) fn team_member_provider_profile_for_mode(
    provider: &str,
    requested_mode: Option<&str>,
) -> ProviderIntegrationProfile {
    // An external interactive member is the user's own already-open
    // interactive provider session, explicitly declared as non-driven. Harness
    // never spawns, drives, cancels, or resumes it; the session polls its
    // inbox and replies over the trusted loopback CLI/MCP. There is no adapter
    // contract and no provider-native session record, so no Harness-side
    // capability claim is made.
    if requested_mode == Some(EXECUTION_MODE_EXTERNAL_INTERACTIVE) {
        return finalized_provider_integration_profile(ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: EXECUTION_MODE_EXTERNAL_INTERACTIVE.to_string(),
            execution_driver: MemberExecutionDriver::UserDriven,
            provider_version: None,
            adapter_contract_version: None,
            reviewed_provider_versions: Vec::new(),
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: None,
            compatibility_note: Some(
                "User-driven external interactive session; Harness owns only its \
                 coordination mail and makes no provider capability claim."
                    .to_string(),
            ),
            interaction_mode: ProviderInteractionMode::Unsupported,
            ordinary_message_boundary: OrdinaryMessageBoundary::Unknown,
            plan_mode: ProviderFeatureMode::Unsupported,
            goal_mode: ProviderFeatureMode::Unsupported,
            tool_event_fidelity: ProviderEventFidelity::None,
            artifact_event_fidelity: ProviderEventFidelity::None,
            supports_cancel: false,
            supports_resume: false,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::Unknown,
            composition_fingerprint: None,
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: None,
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::NoneVerified,
                note: Some("user-driven external session; Harness enforces nothing".to_string()),
            },
        });
    }
    if provider == "deepseek_harness" && matches!(requested_mode, Some("deepseek_sdk") | None) {
        return deepseek_provider_profile();
    }
    // Agent Team Claude members are persistent Agent SDK sessions. Historical
    // `claude_cli` records remain readable but cannot start a new member.
    if provider == "claude" && matches!(requested_mode, Some("claude_agent_sdk") | None) {
        return finalized_provider_integration_profile(ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: "claude_agent_sdk".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: None,
            adapter_contract_version: Some("claude-agent-sdk-v1".to_string()),
            reviewed_provider_versions: vec!["2.1.220".to_string()],
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: Some("2026-07-28".to_string()),
            compatibility_note: Some(
                "Persistent member over the Agent SDK streaming-input mode. \
                 Deterministic lifecycle coverage and a proportional live canary \
                 verified two Host rounds on one native session, correct project \
                 selection, explicit close, and SDK-native session discovery."
                    .to_string(),
            ),
            interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
            ordinary_message_boundary: OrdinaryMessageBoundary::NextRoundBatched,
            plan_mode: ProviderFeatureMode::Emulated,
            goal_mode: ProviderFeatureMode::Emulated,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Structured,
            supports_cancel: true,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::EmbeddedSdk,
            composition_fingerprint: profile_composition_fingerprint(
                "claude",
                "claude_agent_sdk",
                Some("claude-agent-sdk-v1"),
            ),
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: Some("claude-agent-sdk-v1".to_string()),
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::ProviderNativePolicy,
                note: Some(
                    "SDK permissionMode plus runner allowedTools (bypassPermissions under \
                     trusted-development)"
                        .to_string(),
                ),
            },
        });
    }
    // Agent Team Codex members are interactive by definition. Historical
    // `codex_exec` records remain readable but cannot start a new member.
    if provider == "codex" && matches!(requested_mode, Some("codex_app_server") | None) {
        return finalized_provider_integration_profile(ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: "codex_app_server".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: None,
            adapter_contract_version: Some("codex-app-server-v1".to_string()),
            reviewed_provider_versions: vec!["0.148.0-alpha.9".to_string()],
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: Some("2026-08-16".to_string()),
            compatibility_note: Some(
                "Interactive contract reviewed against the generated app-server schemas; \
                 0.148.0-alpha.9 is the exact reviewed app-server runtime. DEV-26 \
                 deterministic coverage and a live canary proved new thread, two \
                 completed rounds, current-turn interrupt, explicit runtime Close, and \
                 exact same-thread Reopen. Steer and provider-native Goal supervision \
                 remain review-required capability slices."
                    .to_string(),
            ),
            interaction_mode: ProviderInteractionMode::PauseAndResume,
            ordinary_message_boundary: OrdinaryMessageBoundary::NextRoundBatched,
            plan_mode: ProviderFeatureMode::Native,
            goal_mode: ProviderFeatureMode::Native,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Structured,
            supports_cancel: true,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::ExternalProtocol,
            composition_fingerprint: profile_composition_fingerprint(
                "codex",
                "codex_app_server",
                Some("codex-app-server-v1"),
            ),
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: Some("codex-app-server-v1".to_string()),
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::ProviderNativePolicy,
                note: Some("app-server thread params sandbox / approvalPolicy".to_string()),
            },
        });
    }
    // Agent Team Pi members use RPC mode (`pi --mode rpc`), a persistent
    // bidirectional JSONL-over-stdio protocol. Retired print-mode records do
    // not create a second Team Member product mode.
    if provider == "pi" && matches!(requested_mode, Some("pi_rpc") | None) {
        return finalized_provider_integration_profile(ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: "pi_rpc".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: None,
            adapter_contract_version: Some("pi-rpc-v1".to_string()),
            reviewed_provider_versions: vec!["0.84.2".to_string()],
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: Some("2026-08-15".to_string()),
            compatibility_note: Some(
                "Pi RPC-mode persistent Agent Team member. Session is a JSONL file; \
                 resume via --session <path> after a fail-closed thinking scan. \
                 Persistent Team sessions force --thinking off. ReadOnly uses \
                 a read/grep/find/ls allowlist; WorkspaceWrite is unavailable \
                 because Pi does not contain paths; FullAccess is admitted only \
                 by explicit trusted policy. Prompt response proves input \
                 acceptance; agent_settled plus get_state proves the cycle boundary. \
                 Quiesce proves native-session flush with file/directory sync and \
                 proves writable-child non-creation only for the reviewed ReadOnly \
                 argv; FullAccess quiesce/release remain PendingDependency until a native job \
                 inventory or OS containment can prove child drain."
                    .to_string(),
            ),
            interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
            ordinary_message_boundary: OrdinaryMessageBoundary::NextRoundBatched,
            plan_mode: ProviderFeatureMode::Emulated,
            goal_mode: ProviderFeatureMode::Emulated,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Structured,
            supports_cancel: true,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::ExternalProtocol,
            composition_fingerprint: profile_composition_fingerprint(
                "pi",
                "pi_rpc",
                Some("pi-rpc-v1"),
            ),
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: Some("pi-rpc-v1".to_string()),
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::AdapterToolAllowlist,
                note: Some(
                    "read_only compiles to a --tools allowlist; workspace_write \
                     fails closed without filesystem containment; trusted \
                     full_access records none_verified at prepare"
                        .to_string(),
                ),
            },
        });
    }
    let mut profile = match provider {
        "kimi" => ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: "kimi_acp".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: None,
            adapter_contract_version: Some("kimi-acp-v1".to_string()),
            reviewed_provider_versions: vec![
                "0.27.0".to_string(),
                "0.31.0".to_string(),
                "0.31.1".to_string(),
                "0.32.0".to_string(),
                "0.33.0".to_string(),
                "0.36.1".to_string(),
            ],
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: Some("2026-08-15".to_string()),
            compatibility_note: Some(
                "Kimi Code 0.36.1 is reviewed for ACP initialize/session creation, \
                 K3 + max reasoning-effort selection, prompt delivery, same-session \
                 resume, prompt-scoped receipt ordering, and cooperative Interrupt. \
                 Version 0.36.1 replays attach history before session/resume returns; \
                 the adapter drains that replay before admitting the next Harness \
                 cycle. Ordinary mail remains next-round batched in the Harness queue."
                    .to_string(),
            ),
            interaction_mode: ProviderInteractionMode::PauseAndResume,
            ordinary_message_boundary: OrdinaryMessageBoundary::NextRoundBatched,
            plan_mode: ProviderFeatureMode::Native,
            goal_mode: ProviderFeatureMode::Emulated,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Summary,
            supports_cancel: true,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::ExternalProtocol,
            composition_fingerprint: profile_composition_fingerprint(
                "kimi",
                "kimi_acp",
                Some("kimi-acp-v1"),
            ),
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: Some("kimi-acp-v1".to_string()),
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::AdapterAutoApproval,
                note: Some(
                    "ACP session/request_permission auto-allow with a one-shot durable receipt"
                        .to_string(),
                ),
            },
        },
        "codex" => ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: "codex_exec".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: None,
            adapter_contract_version: Some("codex-exec-v1".to_string()),
            reviewed_provider_versions: vec!["0.145.0-alpha.18".to_string()],
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: Some("2026-07-21".to_string()),
            compatibility_note: Some("Codex rollout storage is the execution history; Harness keeps only its NativeSessionRef and coordination outcome. App-server is the interactive mode.".to_string()),
            // codex exec --json is non-interactive in this adapter. A future
            // follow-up contract must first turn an end-of-round blocker into
            // a correlated question Message; do not claim it before that exists.
            interaction_mode: ProviderInteractionMode::Unsupported,
            ordinary_message_boundary: OrdinaryMessageBoundary::Unknown,
            plan_mode: ProviderFeatureMode::Unsupported,
            goal_mode: ProviderFeatureMode::Unsupported,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Structured,
            supports_cancel: false,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::ExternalProtocol,
            composition_fingerprint: profile_composition_fingerprint(
                "codex",
                "codex_exec",
                Some("codex-exec-v1"),
            ),
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: Some("codex-exec-v1".to_string()),
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::ProviderNativePolicy,
                note: Some(
                    "historical codex_exec sandbox and approval flags"
                        .to_string(),
                ),
            },
        },
        "claude" => ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: "claude_cli".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: None,
            adapter_contract_version: Some("claude-cli-native-v1".to_string()),
            reviewed_provider_versions: vec!["2.1.181".to_string()],
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: Some("2026-07-22".to_string()),
            compatibility_note: Some(
                "Native stream-json identity, local project session storage, and --resume reviewed."
                    .to_string(),
            ),
            interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
            ordinary_message_boundary: OrdinaryMessageBoundary::Unknown,
            plan_mode: ProviderFeatureMode::Emulated,
            goal_mode: ProviderFeatureMode::Emulated,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Structured,
            supports_cancel: false,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::ExternalProtocol,
            composition_fingerprint: profile_composition_fingerprint(
                "claude",
                "claude_cli",
                Some("claude-cli-native-v1"),
            ),
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: Some("claude-cli-native-v1".to_string()),
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::ProviderNativePolicy,
                note: Some(
                    "historical claude_cli permission-mode and allowedTools flags".to_string(),
                ),
            },
        },
        _ => ProviderIntegrationProfile {
            agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(provider.to_string())),
            model_route: None,
            provider: provider.to_string(),
            execution_mode: "unsupported_team_member".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: None,
            adapter_contract_version: None,
            reviewed_provider_versions: Vec::new(),
            compatibility_status: ProviderCompatibilityStatus::Unknown,
            adapter_reviewed_at: None,
            compatibility_note: Some("No Agent Team Member adapter contract is registered.".to_string()),
            interaction_mode: ProviderInteractionMode::Unsupported,
            ordinary_message_boundary: OrdinaryMessageBoundary::Unknown,
            plan_mode: ProviderFeatureMode::Unsupported,
            goal_mode: ProviderFeatureMode::Unsupported,
            tool_event_fidelity: ProviderEventFidelity::None,
            artifact_event_fidelity: ProviderEventFidelity::None,
            supports_cancel: false,
            supports_resume: false,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::Unknown,
            composition_fingerprint: None,
            capability_fingerprint: None,
            capability_bindings: Vec::new(),
            binding_admission: harness_core::ProviderBindingAdmission::Failed,
            adapter_bridge_revision: None,
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::Unknown,
                note: None,
            },
        },
    };
    finalize_provider_integration_profile(&mut profile);
    profile
}

pub(super) fn apply_provider_version(
    profile: &mut ProviderIntegrationProfile,
    provider_version: Option<String>,
) {
    profile.provider_version = provider_version;
    // Kimi capability claims are version-specific. ACP defines
    // session/cancel as a JSON-RPC notification, not a request. The reviewed
    // 0.27.0, 0.31.0, 0.31.1, and 0.36.1 paths support that notification;
    // unknown versions fail closed rather than inheriting a stale
    // cancellation claim.
    if profile.provider == "kimi" {
        profile.supports_cancel = matches!(
            profile.provider_version.as_deref(),
            Some("0.27.0" | "0.31.0" | "0.31.1" | "0.36.1")
        );
        // Kimi 0.31 adds a real provider-native Goal lifecycle. Harness does
        // not drive it through ACP yet: execution_driver remains host_driven
        // until inspect/replace/cancel/terminal operations are reviewed.
        profile.goal_mode = if matches!(
            profile.provider_version.as_deref(),
            Some("0.31.0" | "0.31.1")
        ) {
            ProviderFeatureMode::Native
        } else {
            ProviderFeatureMode::Emulated
        };
    }
    profile.compatibility_status = match profile.provider_version.as_deref() {
        None => ProviderCompatibilityStatus::Unavailable,
        Some(version)
            if profile
                .reviewed_provider_versions
                .iter()
                .any(|known| known == version) =>
        {
            ProviderCompatibilityStatus::Current
        }
        Some(_) if profile.reviewed_provider_versions.is_empty() => {
            ProviderCompatibilityStatus::Unknown
        }
        Some(_) => ProviderCompatibilityStatus::ReviewRequired,
    };
    profile.compatibility_note = Some(match (
        profile.provider.as_str(),
        profile.provider_version.as_deref(),
        profile.compatibility_status,
    ) {
        ("kimi", Some("0.31.0" | "0.31.1"), ProviderCompatibilityStatus::Current) => {
            "Kimi Code 0.31.x is adapter-reviewed for persistent ACP prompt \
             delivery, model/reasoning-effort selection, native-session resume, \
             next-round batched mail, and cooperative Interrupt through the ACP \
             session/cancel notification."
                .to_string()
        }
        ("kimi", Some("0.36.1"), ProviderCompatibilityStatus::Current) => {
            "Kimi Code 0.36.1 is adapter-reviewed for persistent ACP prompt \
             delivery, K3/max selection, same-session resume with attach replay \
             drained, next-round batched mail, and cooperative Interrupt through \
             the ACP session/cancel notification."
                .to_string()
        }
        ("codex", Some("0.148.0-alpha.9"), ProviderCompatibilityStatus::Current) => {
            "Codex 0.148.0-alpha.9 is adapter-reviewed for persistent app-server \
             thread open/resume, effective sandbox and approval-policy receipts, \
             completed rounds, current-turn interrupt, explicit runtime Close, and \
             exact same-thread Reopen. Native Goal supervision and live steer remain \
             review-required capability slices."
                .to_string()
        }
        (_, _, ProviderCompatibilityStatus::Current) => "Installed provider version matches an adapter-reviewed version.".to_string(),
        (_, _, ProviderCompatibilityStatus::ReviewRequired) => "Installed provider version has not been reviewed against this adapter contract; regenerate protocol schemas and run provider acceptance before promotion.".to_string(),
        (_, _, ProviderCompatibilityStatus::Unavailable) => "Provider version could not be detected.".to_string(),
        (_, _, ProviderCompatibilityStatus::Incompatible) => "Provider version is known to be incompatible with this adapter contract.".to_string(),
        (_, _, ProviderCompatibilityStatus::Unknown) => "No reviewed provider version is registered for this execution mode.".to_string(),
    });
    finalize_provider_integration_profile(profile);
}

/// Probe the executable that backs a persistent Agent Team member and refresh
/// its version-specific compatibility snapshot without touching its native
/// session. The caller decides whether and how to journal the refreshed row.
pub(super) fn refreshed_team_member_provider_profile(
    member: &ProviderRuntimeProjection,
) -> CliResult<(ProviderIntegrationProfile, Option<String>)> {
    // Base the refresh on the CURRENT adapter registry, not the stored
    // profile. The stored profile's reviewed_provider_versions is frozen at
    // member-creation time, so a registry update could never unblock an
    // existing durable ProviderRuntimeProjection. The gate persists the refreshed profile
    // back, so existing members can self-heal on the next boundary.
    let stored_mode = member
        .provider_profile
        .as_ref()
        .map(|stored| stored.execution_mode.clone());
    let mut profile =
        team_member_provider_profile_for_mode(&member.provider, stored_mode.as_deref());
    let detected = team_member_provider_version_output(&member.provider);
    let probe_error = detected.as_ref().err().cloned();
    apply_provider_version(&mut profile, detected.ok());
    Ok((profile, probe_error))
}

/// Stage a refreshed compatibility snapshot without creating an in-memory
/// revision that was never persisted. Callers CAS only when this returns true.
pub(super) fn apply_refreshed_provider_profile(
    member: &mut ProviderRuntimeProjection,
    profile: ProviderIntegrationProfile,
) -> bool {
    if member.provider_profile.as_ref() == Some(&profile) {
        return false;
    }
    member.provider_profile = Some(profile);
    member.last_event_at = Some(now_string());
    true
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderCompatibilityBlockProvenance {
    pub(super) schema: String,
    pub(super) member_run_id: String,
    pub(super) provider: String,
    pub(super) execution_mode: String,
    pub(super) provider_version: String,
    pub(super) adapter_contract_version: String,
    pub(super) boundary: String,
    pub(super) compatibility_status: String,
    pub(super) source: String,
    pub(super) probe_error: Option<String>,
    pub(super) no_provider_side_effects: bool,
    pub(super) remediation: String,
}

pub(super) const PROVIDER_COMPATIBILITY_BLOCK_PREFIX: &str = "PROVIDER_COMPATIBILITY_BLOCKED: ";
pub(super) const PROVIDER_COMPATIBILITY_BLOCK_SCHEMA: &str = "provider_compatibility_block/v1";

impl ProviderCompatibilityBlockProvenance {
    pub(super) fn for_refusal(
        member: &ProviderRuntimeProjection,
        profile: &ProviderIntegrationProfile,
        resolution: &ProviderCompatibilityResolution,
        boundary: &str,
    ) -> Self {
        Self {
            schema: PROVIDER_COMPATIBILITY_BLOCK_SCHEMA.to_string(),
            member_run_id: member.id.clone(),
            provider: profile.provider.clone(),
            execution_mode: profile.execution_mode.clone(),
            provider_version: profile
                .provider_version
                .clone()
                .unwrap_or_else(|| "unavailable".to_string()),
            adapter_contract_version: profile
                .adapter_contract_version
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            boundary: boundary.to_string(),
            compatibility_status: serde_snake_label(&resolution.status),
            source: resolution.source.to_string(),
            probe_error: resolution.probe_error.clone(),
            no_provider_side_effects: true,
            remediation: "complete source review or record an exact operational admission with `harness provider admit`; admissions do not change source-review status".to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct ProviderCompatibilityResolution {
    pub(super) allowed: bool,
    pub(super) needs_review: bool,
    pub(super) status: ProviderCompatibilityStatus,
    pub(super) source: &'static str,
    pub(super) policy: Option<ProviderCompatibilityAdmissionPolicy>,
    pub(super) admission: Option<ProviderCompatibilityAdmission>,
    pub(super) probe_error: Option<String>,
    pub(super) warning: Option<String>,
}

/// The single operational resolver shared by preflight, start, resume,
/// recovery, reopen, rebind, and capability reporting. Adapter source review
/// and operational admission remain separate facts in the returned value.
pub(super) fn resolve_provider_compatibility(
    store: &HarnessStore,
    profile: &ProviderIntegrationProfile,
    probe_error: Option<&str>,
) -> CliResult<ProviderCompatibilityResolution> {
    if probe_error.is_some() {
        return Ok(ProviderCompatibilityResolution {
            allowed: false,
            needs_review: false,
            status: ProviderCompatibilityStatus::Unavailable,
            source: "version_probe",
            policy: None,
            admission: None,
            probe_error: probe_error.map(str::to_string),
            warning: None,
        });
    }
    if profile.compatibility_status == ProviderCompatibilityStatus::Current {
        return Ok(ProviderCompatibilityResolution {
            allowed: true,
            needs_review: false,
            status: profile.compatibility_status,
            source: "adapter_source_review",
            policy: None,
            admission: None,
            probe_error: None,
            warning: None,
        });
    }
    let admission = match (
        store.provider_compatibility_scope(),
        profile.provider_version.as_deref(),
        profile.adapter_contract_version.as_deref(),
    ) {
        (Some(_), Some(provider_version), Some(adapter_contract_version)) => store
            .effective_provider_compatibility_admission(
                &profile.provider,
                &profile.execution_mode,
                provider_version,
                adapter_contract_version,
            )?,
        _ => None,
    };
    // An operational admission may bridge only the explicitly review-required
    // state. Neither policy can excuse an unavailable probe, a known
    // incompatibility, or a mode with no adapter contract.
    let allowed = profile.compatibility_status == ProviderCompatibilityStatus::ReviewRequired
        && admission.is_some();
    let needs_review = profile.compatibility_status == ProviderCompatibilityStatus::ReviewRequired
        && !matches!(
            admission.as_ref().map(|value| value.policy),
            Some(ProviderCompatibilityAdmissionPolicy::Strict)
        );
    Ok(ProviderCompatibilityResolution {
        allowed,
        needs_review,
        status: profile.compatibility_status,
        source: if admission.is_some() {
            "operational_admission"
        } else {
            "adapter_compatibility"
        },
        policy: admission.as_ref().map(|value| value.policy),
        admission,
        probe_error: None,
        warning: needs_review.then(|| {
            "advisory operational admission permits execution but source review remains required"
                .to_string()
        }),
    })
}

pub(super) fn provider_compatibility_block_reason(
    member: &ProviderRuntimeProjection,
    profile: &ProviderIntegrationProfile,
    resolution: &ProviderCompatibilityResolution,
    boundary: &str,
) -> Option<String> {
    if member.is_external_interactive() || resolution.allowed {
        return None;
    }
    let provenance =
        ProviderCompatibilityBlockProvenance::for_refusal(member, profile, resolution, boundary);
    Some(format!(
        "{PROVIDER_COMPATIBILITY_BLOCK_PREFIX}{}",
        serde_json::to_string(&provenance).expect("compatibility provenance serializes")
    ))
}

pub(super) fn provider_compatibility_block_cause(
    member: &ProviderRuntimeProjection,
    profile: &ProviderIntegrationProfile,
    resolution: &ProviderCompatibilityResolution,
    boundary: ProviderCompatibilityBlockBoundary,
) -> Option<ProviderCompatibilityBlockCause> {
    if member.is_external_interactive() || resolution.allowed {
        return None;
    }
    Some(ProviderCompatibilityBlockCause {
        schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
        id: generated_id("provider-compatibility-block"),
        member_run_id: member.id.clone(),
        provider: profile.provider.clone(),
        execution_mode: profile.execution_mode.clone(),
        provider_version: profile
            .provider_version
            .clone()
            .unwrap_or_else(|| "unavailable".to_string()),
        adapter_contract_version: profile
            .adapter_contract_version
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        boundary,
        compatibility_status: resolution.status,
        source: if resolution.probe_error.is_some() {
            ProviderCompatibilityBlockSource::ProbeFailure
        } else {
            ProviderCompatibilityBlockSource::AdapterCompatibility
        },
        probe_error: resolution.probe_error.clone(),
        caused_at: now_string(),
    })
}

pub(super) fn compatibility_recovery_status(
    store: &HarnessStore,
    member: &ProviderRuntimeProjection,
) -> CliResult<MemberRunStatus> {
    if member.native_session.is_some() {
        return Ok(MemberRunStatus::Disconnected);
    }
    let has_assigned_work = store.latest_works()?.into_iter().any(|work| {
        !work.is_terminal()
            && work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str())
    });
    Ok(
        if member.provider_environment_observation.is_some() || has_assigned_work {
            MemberRunStatus::Queued
        } else {
            MemberRunStatus::Idle
        },
    )
}

#[cfg(test)]
pub(super) fn compatibility_block_matches_current_tuple(
    member: &ProviderRuntimeProjection,
    profile: &ProviderIntegrationProfile,
) -> bool {
    member.status == MemberRunStatus::Blocked
        && member
            .provider_compatibility_block_cause
            .as_ref()
            .is_some_and(|cause| {
                cause.member_run_id == member.id
                    && cause.exact_key()
                        == (
                            profile.provider.as_str(),
                            profile.execution_mode.as_str(),
                            profile.provider_version.as_deref().unwrap_or("unavailable"),
                            profile
                                .adapter_contract_version
                                .as_deref()
                                .unwrap_or("unknown"),
                        )
            })
}

/// Last pre-dispatch compatibility fence. It runs before provider capacity,
/// delivery claim, process spawn, or native-session attach, and returns a
/// durable blocked outcome instead of entering the transport recovery loop.
pub(super) fn provider_compatibility_start_gate(
    ledger: &TeamRunLedger,
    member: &mut ProviderRuntimeProjection,
    boundary: ProviderCompatibilityBlockBoundary,
) -> CliResult<Option<MemberOutcome>> {
    if member.is_external_interactive() {
        return Ok(None);
    }
    let expected = member.clone();
    let (profile, probe_error) = refreshed_team_member_provider_profile(member)?;
    let resolution =
        resolve_provider_compatibility(&ledger.store, &profile, probe_error.as_deref())?;
    let boundary_label = match boundary {
        ProviderCompatibilityBlockBoundary::StartPersistentExecution => {
            "start persistent Agent Team execution"
        }
        ProviderCompatibilityBlockBoundary::ResumePersistentExecution => {
            "resume persistent Agent Team execution"
        }
    };
    let reason = provider_compatibility_block_reason(member, &profile, &resolution, boundary_label);
    if reason.is_none() {
        let compatibility_owned_block = member.status == MemberRunStatus::Blocked
            && member.provider_compatibility_block_cause.is_some();
        if compatibility_owned_block {
            let recovery_status = compatibility_recovery_status(&ledger.store, member)?;
            let recovered = ledger
                .store
                .recover_member_run_from_provider_compatibility_block(
                    &expected,
                    &profile,
                    boundary,
                    recovery_status,
                    &now_string(),
                )?;
            *member = recovered;
        } else if apply_refreshed_provider_profile(member, profile) {
            ledger.save_member_run(&expected, member)?;
        }
        return Ok(None);
    }

    let reason = reason.expect("non-current compatibility has a refusal reason");
    if member.status == MemberRunStatus::Blocked
        && member.provider_compatibility_block_cause.is_some()
    {
        return Ok(Some(MemberOutcome::new(
            member,
            MemberRunStatus::Blocked,
            reason,
        )));
    }
    let cause = provider_compatibility_block_cause(member, &profile, &resolution, boundary)
        .expect("a blocked persistent boundary has a typed compatibility cause");
    *member = ledger.store.block_member_run_for_provider_compatibility(
        &expected,
        &profile,
        cause,
        &now_string(),
    )?;
    let action = ledger.append_action(
        &member.id,
        "provider_compatibility_blocked",
        MemberActionStatus::Failed,
        "provider compatibility gate blocked persistent execution",
        &reason,
    )?;
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "action",
        &action.id,
        "created",
        &format!("{} blocked before provider execution", member.name),
    )?;
    Ok(Some(MemberOutcome::new(
        member,
        MemberRunStatus::Blocked,
        reason,
    )))
}
