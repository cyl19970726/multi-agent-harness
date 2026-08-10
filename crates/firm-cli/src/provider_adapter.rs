//! Provider-neutral runtime adapter contract for Wave 4C.
//!
//! Provider-specific protocol code may execute these frozen primitives, but it
//! may not decide Team, Work, Message, identity, or session authority.

use harness_core::agentfirm_api::{AgentSessionStatus, PermissionCeiling, RuntimeDispatchMode};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderCapabilities {
    pub create_attach_resume: bool,
    pub queue_next_turn: bool,
    pub safe_current_turn_injection: bool,
    pub interrupt: bool,
    pub close: bool,
    pub inspect_state: bool,
    pub reconcile_effect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderPermissionMapping {
    pub provider: String,
    pub requested: PermissionCeiling,
    pub effective: PermissionCeiling,
    pub native_sandbox: String,
    pub native_approval: String,
}

pub(crate) fn capabilities(provider: &str) -> Option<ProviderCapabilities> {
    let safe_current_turn_injection = match provider {
        // Codex app-server exposes steer, but the adapter still has to prove a
        // safe injection point at runtime; the static matrix alone never does.
        "codex" => true,
        "claude" | "kimi" | "pi" => false,
        _ => return None,
    };
    Some(ProviderCapabilities {
        create_attach_resume: true,
        queue_next_turn: true,
        safe_current_turn_injection,
        interrupt: true,
        close: true,
        inspect_state: true,
        reconcile_effect: true,
    })
}

pub(crate) fn map_permission(
    provider: &str,
    requested: PermissionCeiling,
) -> Result<ProviderPermissionMapping, String> {
    if capabilities(provider).is_none() {
        return Err(format!("PROVIDER_CAPABILITY_UNPROVABLE: {provider}"));
    }
    let (native_sandbox, native_approval) = match (provider, requested) {
        ("codex", PermissionCeiling::ReadOnly) => ("read-only", "on-request"),
        ("codex", PermissionCeiling::WorkspaceWrite) => ("workspace-write", "on-request"),
        ("codex", PermissionCeiling::FullAccess) => ("danger-full-access", "on-request"),
        ("claude", PermissionCeiling::ReadOnly) => ("plan", "default"),
        ("claude", PermissionCeiling::WorkspaceWrite) => ("acceptEdits", "default"),
        ("kimi" | "pi", PermissionCeiling::ReadOnly) => ("read-only", "default"),
        ("kimi" | "pi", PermissionCeiling::WorkspaceWrite) => ("workspace-write", "default"),
        // These adapters cannot prove a native ceiling equivalent to explicit
        // full access. Failing closed is safer than silently widening.
        ("claude" | "kimi" | "pi", PermissionCeiling::FullAccess) => {
            return Err(format!(
                "PROVIDER_PERMISSION_MISMATCH: {provider} cannot prove full_access"
            ))
        }
        _ => return Err(format!("PROVIDER_CAPABILITY_UNPROVABLE: {provider}")),
    };
    Ok(ProviderPermissionMapping {
        provider: provider.to_string(),
        requested,
        effective: requested,
        native_sandbox: native_sandbox.to_string(),
        native_approval: native_approval.to_string(),
    })
}

pub(crate) fn effective_delivery_mode(
    provider: &str,
    requested: RuntimeDispatchMode,
    lifecycle: AgentSessionStatus,
    safe_injection_point_observed: bool,
) -> Result<RuntimeDispatchMode, String> {
    let capabilities = capabilities(provider)
        .ok_or_else(|| format!("PROVIDER_CAPABILITY_UNPROVABLE: {provider}"))?;
    Ok(match requested {
        RuntimeDispatchMode::QueueOnly => RuntimeDispatchMode::QueueOnly,
        RuntimeDispatchMode::StartIfIdle => match lifecycle {
            AgentSessionStatus::Cold | AgentSessionStatus::Idle => RuntimeDispatchMode::StartIfIdle,
            _ => RuntimeDispatchMode::QueueOnly,
        },
        RuntimeDispatchMode::InjectIfSafe
            if lifecycle == AgentSessionStatus::Active
                && capabilities.safe_current_turn_injection
                && safe_injection_point_observed =>
        {
            RuntimeDispatchMode::InjectIfSafe
        }
        RuntimeDispatchMode::InjectIfSafe => RuntimeDispatchMode::QueueOnly,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_provider_conformance_matrix_is_closed_and_fail_closed() {
        for provider in ["codex", "claude", "kimi", "pi"] {
            let capabilities = capabilities(provider).expect("known provider");
            assert!(capabilities.create_attach_resume);
            assert!(capabilities.queue_next_turn);
            assert!(capabilities.interrupt && capabilities.close);
            assert!(capabilities.inspect_state && capabilities.reconcile_effect);
            assert!(map_permission(provider, PermissionCeiling::ReadOnly).is_ok());
            assert!(map_permission(provider, PermissionCeiling::WorkspaceWrite).is_ok());
        }
        assert!(map_permission("unknown", PermissionCeiling::ReadOnly).is_err());
        for provider in ["claude", "kimi", "pi"] {
            assert!(map_permission(provider, PermissionCeiling::FullAccess).is_err());
        }
        let codex = map_permission("codex", PermissionCeiling::FullAccess).unwrap();
        assert_eq!(codex.native_approval, "on-request");
        assert_ne!(codex.native_approval, "never");
    }

    #[test]
    fn injection_requires_both_adapter_capability_and_observed_safe_point() {
        assert_eq!(
            effective_delivery_mode(
                "codex",
                RuntimeDispatchMode::InjectIfSafe,
                AgentSessionStatus::Active,
                false,
            )
            .unwrap(),
            RuntimeDispatchMode::QueueOnly
        );
        assert_eq!(
            effective_delivery_mode(
                "codex",
                RuntimeDispatchMode::InjectIfSafe,
                AgentSessionStatus::Active,
                true,
            )
            .unwrap(),
            RuntimeDispatchMode::InjectIfSafe
        );
        assert_eq!(
            effective_delivery_mode(
                "claude",
                RuntimeDispatchMode::InjectIfSafe,
                AgentSessionStatus::Active,
                true,
            )
            .unwrap(),
            RuntimeDispatchMode::QueueOnly
        );
    }
}
