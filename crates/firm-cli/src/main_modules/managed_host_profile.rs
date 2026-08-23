use super::*;

/// A managed Host is a coordination runtime, not a second writable driver in
/// the Team execution root. Without an isolated Host workspace, providers that
/// can prove `ReadOnly` are narrowed to it. An explicit, uniquely reserved
/// workspace distinct from the Team execution root may instead retain the
/// durable AgentMember ceiling. The Store-owned MemberRun reservation keeps
/// one writable driver per workspace without inventing a provider sandbox.
pub(super) fn effective_member_permission_ceiling(
    store: &HarnessStore,
    durable_ceiling: harness_core::agentfirm_api::PermissionCeiling,
    run: &AgentTeamRun,
    member: &ProviderRuntimeProjection,
) -> CliResult<harness_core::agentfirm_api::PermissionCeiling> {
    let is_host = run
        .host_actor
        .as_ref()
        .is_some_and(|host| host.kind == TeamActorKind::Host && host.id == member.agent_member_id);
    if !is_host {
        return Ok(durable_ceiling);
    }

    let read_only = harness_core::agentfirm_api::PermissionCeiling::ReadOnly;
    let supports_read_only =
        crate::provider_adapter::map_permission(&member.provider, read_only).is_ok();
    if durable_ceiling == read_only {
        if supports_read_only {
            return Ok(read_only);
        }
        return Err(CliError::Usage(format!(
            "MANAGED_HOST_PERMISSION_UNPROVABLE: provider {} cannot prove ReadOnly for Host AgentMember {}",
            member.provider, member.agent_member_id
        )));
    }
    if supports_read_only
        && member
            .provider_cwd_hint
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Ok(read_only);
    }
    if !supports_read_only
        && durable_ceiling != harness_core::agentfirm_api::PermissionCeiling::FullAccess
    {
        return Err(CliError::Usage(format!(
            "MANAGED_HOST_PERMISSION_UNPROVABLE: provider {} cannot prove ReadOnly and Host AgentMember {} is not frozen to FullAccess",
            member.provider, member.agent_member_id
        )));
    }
    let host_workspace = member
        .provider_cwd_hint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "MANAGED_HOST_WORKSPACE_ISOLATION_REQUIRED: provider {} requires an explicit Host provider_cwd_hint distinct from the Team execution root",
                member.provider
            ))
        })?;
    let host_workspace = project::canonicalize_best_effort(std::path::Path::new(host_workspace));
    let team_workspace = run
        .execution_root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(std::path::Path::new)
        .map(project::canonicalize_best_effort)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "MANAGED_HOST_WORKSPACE_ISOLATION_REQUIRED: provider {} cannot prove an independent Host workspace without an exact Team execution root",
                member.provider
            ))
        })?;
    if team_workspace == host_workspace {
        return Err(CliError::Usage(format!(
            "MANAGED_HOST_WORKSPACE_ISOLATION_REQUIRED: provider {} Host workspace must differ from the Team execution root",
            member.provider
        )));
    }
    store.require_unique_managed_host_workspace(run, member)?;
    Ok(durable_ceiling)
}

pub(super) fn host_runtime_projection(mode: HostControlMode) -> serde_json::Value {
    let managed = mode == HostControlMode::Managed;
    serde_json::json!({
        "mode": if managed { "managed" } else { "external_interactive" },
        "delivery_guarantee": if managed { "daemon_managed" } else { "pull_only" },
        "runtime_residency": if managed { "managed_member_run" } else { "detached_user_driven" },
        "workspace_policy": if managed { "provider_read_only_or_distinct_host_workspace" } else { "user_managed" },
        "warning": (!managed).then_some("External Host delivery is weaker: Harness cannot drive or prove provider receipt; the Host must explicitly read and acknowledge its own inbox."),
    })
}
