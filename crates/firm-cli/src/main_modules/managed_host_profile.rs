use super::*;

/// Trusted-development managed coding Agents use one provider-neutral policy:
/// Host and Member both retain the durable FullAccess ceiling and freeze an
/// exact canonical cwd in their AgentSession. Multiple explicitly bound
/// Sessions may share that cwd; worktree isolation is an operator choice.
pub(super) fn effective_member_permission_ceiling(
    _store: &HarnessStore,
    durable_ceiling: harness_core::agentfirm_api::PermissionCeiling,
    _run: &AgentTeamRun,
    member: &ProviderRuntimeProjection,
) -> CliResult<harness_core::agentfirm_api::PermissionCeiling> {
    let full_access = harness_core::agentfirm_api::PermissionCeiling::FullAccess;
    if durable_ceiling != full_access {
        return Err(CliError::Usage(format!(
            "TRUSTED_DEVELOPMENT_FULL_ACCESS_REQUIRED: managed coding AgentMember {} is frozen to {:?}; create a new FullAccess AgentMember/Session instead of widening it in place",
            member.agent_member_id, durable_ceiling
        )));
    }
    let workspace = member
        .provider_cwd_hint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "MEMBER_WORKSPACE_REQUIRED: MemberRun {} requires an exact canonical cwd",
                member.id
            ))
        })?;
    let canonical = std::fs::canonicalize(workspace).map_err(|error| {
        CliError::Usage(format!(
            "MEMBER_WORKSPACE_NOT_CANONICAL: MemberRun {} cwd {} cannot be resolved: {error}",
            member.id, workspace
        ))
    })?;
    if canonical.as_path() != std::path::Path::new(workspace) {
        return Err(CliError::Usage(format!(
            "MEMBER_WORKSPACE_NOT_CANONICAL: MemberRun {} must freeze exact cwd {}",
            member.id,
            canonical.display()
        )));
    }
    crate::provider_adapter::map_permission(&member.provider, full_access)
        .map_err(CliError::Usage)?;
    Ok(full_access)
}

pub(super) fn host_runtime_projection(mode: HostControlMode) -> serde_json::Value {
    let managed = mode == HostControlMode::Managed;
    serde_json::json!({
        "mode": if managed { "managed" } else { "external_interactive" },
        "delivery_guarantee": if managed { "daemon_managed" } else { "pull_only" },
        "runtime_residency": if managed { "managed_member_run" } else { "detached_user_driven" },
        "workspace_policy": if managed { "trusted_full_access_exact_cwd_shared_allowed" } else { "user_managed" },
        "workspace_requirement": managed.then_some("exact_canonical_cwd_shared_or_isolated"),
        "warning": (!managed).then_some("External Host delivery is weaker: Harness cannot drive or prove provider receipt; the Host must explicitly read and acknowledge its own inbox."),
    })
}
