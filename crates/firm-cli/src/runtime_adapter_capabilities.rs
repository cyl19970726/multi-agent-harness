//! Capability admission, permission compilation, and provider binding registry.

use super::*;

// ---------------------------------------------------------------------------
// Capability honesty
// ---------------------------------------------------------------------------

/// Per-capability execution status (DOC-89 §8.1). Order matters only for
/// reporting; every entry must carry `evidence` naming the proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityStatus {
    Supported,
    Unsupported,
    Degraded,
    // No binding reports Experimental today; the status exists so a future
    // binding (e.g. a DeepSeek native bridge canary) can ship honestly
    // between Unsupported and Supported.
    #[allow(dead_code)]
    Experimental,
}

impl CapabilityStatus {
    #[cfg(test)]
    pub(crate) fn is_supported(self) -> bool {
        matches!(self, CapabilityStatus::Supported)
    }
}

/// One semantic intent → execution status + evidence. `evidence` names the
/// proof (RPC contract, test, or transport fact); a binding without proof
/// reports `Unsupported`/`Experimental`, never `Supported`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CapabilityBinding {
    pub capability: &'static str,
    pub status: CapabilityStatus,
    pub evidence: String,
    /// Security-relevant capabilities name the real enforcement mechanism;
    /// absence means none was verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_enforcement_locus: Option<String>,
}

pub(super) fn canonical_runtime_binding(
    session: &harness_core::agentfirm_api::AgentSession,
) -> harness_core::agentfirm_api::RuntimeCommandBinding {
    harness_core::agentfirm_api::RuntimeCommandBinding {
        target_session_id: Some(session.id.clone()),
        target_runtime_generation: Some(session.runtime_generation),
        target_driver_generation: Some(session.control_state.driver_generation),
        target_driver: session.control_state.driver_ref.clone(),
        native_session_ref: session.native_session_ref.clone(),
        composition_fingerprint: session.control_state.composition_fingerprint.clone(),
        capability_fingerprint: session.control_state.capability_fingerprint.clone(),
        capability_profile_version: session
            .control_state
            .capability_fingerprint
            .as_ref()
            .map(|_| "agentfirm-runtime-adapter-v1".to_string()),
        permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
    }
}

pub(super) fn preflight_start_cycle<A: TeamRuntimeAdapter>(
    adapter: &A,
    session: &harness_core::agentfirm_api::AgentSession,
) -> CliResult<()> {
    let binding = canonical_runtime_binding(session);
    crate::runtime_adapter_contract::preflight_effect(
        crate::runtime_adapter_contract::RuntimeAdapter::describe(adapter),
        session,
        crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &session.node_daemon_id,
            target_node_daemon_generation: session.node_daemon_generation,
        },
        crate::runtime_adapter_contract::SemanticCapability::StartCycle,
        &[],
    )
    .map(|_| ())
    .map_err(|error| CliError::Usage(format!("RUNTIME_ADAPTER_PREFLIGHT_FAILED: {error}")))
}

/// Provider-profile preflight used before a live RuntimeHandle exists. This
/// keeps process spawn/resume behind the same exact capability closure and
/// session composition fence as operations on an attached adapter.
pub(crate) fn preflight_profile_effect(
    profile: &harness_core::ProviderIntegrationProfile,
    session: &harness_core::agentfirm_api::AgentSession,
    capability: crate::runtime_adapter_contract::SemanticCapability,
) -> CliResult<()> {
    harness_core::Validate::validate(profile).map_err(|error| {
        CliError::Usage(format!(
            "RUNTIME_ADAPTER_PROFILE_INVALID: provider profile failed canonical validation: {error}"
        ))
    })?;
    let composition_fingerprint = profile.composition_fingerprint.clone().ok_or_else(|| {
        CliError::Usage(
            "RUNTIME_ADAPTER_FENCE_INCOMPLETE: profile has no composition fingerprint".to_string(),
        )
    })?;
    let capability_fingerprint = profile.capability_fingerprint.clone().ok_or_else(|| {
        CliError::Usage(
            "RUNTIME_ADAPTER_FENCE_INCOMPLETE: profile has no capability fingerprint".to_string(),
        )
    })?;
    let description = crate::runtime_adapter_contract::RuntimeDescription {
        binding_id: format!("{}:{}", profile.provider, profile.execution_mode),
        native_protocol: profile.execution_mode.clone(),
        composition_fingerprint,
        capability_fingerprint,
        capability_bindings: profile.capability_bindings.clone(),
    };
    let binding = canonical_runtime_binding(session);
    crate::runtime_adapter_contract::preflight_effect(
        &description,
        session,
        crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &session.node_daemon_id,
            target_node_daemon_generation: session.node_daemon_generation,
        },
        capability,
        &[],
    )
    .map(|_| ())
    .map_err(|error| CliError::Usage(format!("RUNTIME_ADAPTER_PREFLIGHT_FAILED: {error}")))
}

// ---------------------------------------------------------------------------
// Permission ceiling → Pi tool allowlist compilation
// ---------------------------------------------------------------------------

/// Compile an *admissible* permission ceiling into Pi launch arguments.
///
/// - `ReadOnly` → read-only tools only.
/// - `WorkspaceWrite` → refused: Pi's tool-kind allowlist does not contain
///   write/edit paths and therefore cannot enforce the workspace boundary.
/// - `FullAccess` → `None`: the Pi default toolset (including `bash`) runs
///   unrestricted. This is only honest when the profile records
///   `security_enforcement_locus = none_verified` — the adapter enforces
///   nothing and says so.
pub(crate) fn pi_tools_allowlist_for_ceiling(
    ceiling: PermissionCeiling,
) -> CliResult<Option<&'static str>> {
    match ceiling {
        PermissionCeiling::ReadOnly => Ok(Some("read,grep,find,ls")),
        PermissionCeiling::WorkspaceWrite => Err(CliError::Usage(
            "PI_PERMISSION_ADMISSION_FAILED: workspace_write requires verified filesystem containment; Pi --tools only limits tool kinds"
                .to_string(),
        )),
        PermissionCeiling::FullAccess => Ok(None),
    }
}

/// The enforcement-locus claim matching `pi_tools_allowlist_for_ceiling`.
pub(crate) fn pi_security_enforcement_locus(
    ceiling: PermissionCeiling,
) -> harness_core::SecurityEnforcementLocus {
    use harness_core::{SecurityEnforcementLocus, SecurityEnforcementLocusKind};
    match ceiling {
        PermissionCeiling::ReadOnly => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::AdapterToolAllowlist,
            note: Some(
                "compiled to Pi's read-only `--tools` allowlist at spawn".to_string(),
            ),
        },
        PermissionCeiling::WorkspaceWrite => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::NoneVerified,
            note: Some(
                "Pi --tools limits tool kinds but does not contain write/edit paths; workspace_write admission is refused without an OS sandbox or reviewed bridge"
                    .to_string(),
            ),
        },
        PermissionCeiling::FullAccess => SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::NoneVerified,
            note: Some(
                "full access runs the Pi default toolset under explicit trusted policy; no adapter-level enforcement verified"
                    .to_string(),
            ),
        },
    }
}

/// Fail-closed admission for the exact Pi spawn policy. Pi's `--tools`
/// switch is a tool-kind allowlist, not a filesystem containment boundary:
/// `write`/`edit` can target paths outside the workspace. Consequently only
/// read-only and explicitly trusted full-access launches are admissible until
/// an OS sandbox or reviewed native bridge is part of the composition.
pub(crate) fn admit_pi_permission_ceiling(
    ceiling: PermissionCeiling,
    compiled_tools: Option<&str>,
) -> CliResult<harness_core::SecurityEnforcementLocus> {
    let expected = pi_tools_allowlist_for_ceiling(ceiling)?;
    if compiled_tools != expected {
        return Err(CliError::Usage(format!(
            "PI_PERMISSION_ADMISSION_FAILED: {ceiling:?} expected tools {expected:?}, got {compiled_tools:?}"
        )));
    }
    Ok(pi_security_enforcement_locus(ceiling))
}

// ---------------------------------------------------------------------------
// Binding registry
// ---------------------------------------------------------------------------

/// The closed set of persistent Team runtime implementations. This one
/// selector is shared by admission, capability reporting, and runner dispatch
/// so a provider cannot advertise an executable binding without a runnable
/// path (or acquire a runnable path without a capability contract).
pub(crate) use harness_application::TeamRuntimeKind as SharedTeamRuntimeKind;

pub(crate) fn shared_team_runtime_kind(
    provider: &str,
    execution_mode: Option<&str>,
) -> Option<SharedTeamRuntimeKind> {
    harness_application::team_runtime_kind(provider, execution_mode)
}

/// Executable capability report for a provider's Team runtime binding, when
/// one exists. `None` means no provider-neutral Team runtime binding is
/// registered and persistent Team execution must fail closed.
pub(crate) fn capability_bindings_for(provider: &str) -> Option<Vec<CapabilityBinding>> {
    match shared_team_runtime_kind(provider, None)? {
        SharedTeamRuntimeKind::Pi => Some(crate::pi_rpc::PiTeamRuntime::capability_bindings()),
        SharedTeamRuntimeKind::Kimi => {
            Some(crate::kimi_team_runtime::KimiTeamRuntime::capability_bindings())
        }
        SharedTeamRuntimeKind::Codex => Some(crate::codex_team_runtime::capability_bindings()),
        SharedTeamRuntimeKind::Claude => {
            Some(crate::claude_team_runtime::ClaudeTeamRuntime::capability_bindings())
        }
    }
}

#[cfg(test)]
#[path = "runtime_adapter_capabilities_tests.rs"]
mod tests;
