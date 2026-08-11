//! Provider-neutral runtime adapter contract for Wave 4C.
//!
//! Provider-specific protocol code may execute these frozen primitives, but it
//! may not decide Team, Work, Message, identity, or session authority.

use harness_core::agentfirm_api::{
    AgentSession, AgentSessionStatus, NativeSessionAvailability, NativeSessionRef,
    PermissionCeiling, RuntimeDispatchMode,
};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::codex_app_server::{CodexAppServerClient, CodexAppServerSpawnOptions};
use crate::{
    CliError, CliResult, ProviderEffectAdmission, ProviderRuntimeProjection, TeamRunLedger,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderControlAction {
    CancelProviderTurn,
    CloseSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeControlPrimitive {
    CodexTurnInterrupt,
    ClaudeAgentSdkInterrupt,
    KimiAcpCancel,
    PiRpcInterrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderControlPlan {
    pub provider: String,
    pub action: ProviderControlAction,
    pub primitive: NativeControlPrimitive,
    pub requires_terminal_ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderAvailability {
    pub provider: String,
    pub binary: String,
    pub available: bool,
    pub version_probe: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct NodeSessionCapabilities {
    pub start: bool,
    pub resume: bool,
    pub cancel_turn: bool,
    pub stop: bool,
}

/// Live provider handle owned by the machine NodeDaemon, never by a Team.
/// New provider variants enter only after they implement the same start / stop
/// / resume / cancellation contract; an installed binary alone is not proof.
pub(crate) enum NodeSessionRuntime {
    Codex(CodexAppServerClient),
}

impl NodeSessionRuntime {
    pub(crate) fn provider(&self) -> &'static str {
        match self {
            Self::Codex(_) => "codex",
        }
    }

    pub(crate) fn native_session_id(&self) -> &str {
        match self {
            Self::Codex(client) => client.thread_id(),
        }
    }
}

pub(crate) struct OpenedNodeSession {
    pub runtime: NodeSessionRuntime,
    pub native_session_ref: NativeSessionRef,
    pub permission_mapping: ProviderPermissionMapping,
}

pub(crate) fn node_session_capabilities(provider: &str) -> Option<NodeSessionCapabilities> {
    Some(match provider {
        "codex" => NodeSessionCapabilities {
            start: true,
            resume: true,
            // Standalone DispatchProvider is not yet wired to this handle, so
            // a synthetic AgentSession turn id cannot be advertised as a
            // native Codex turn cancellation capability.
            cancel_turn: false,
            stop: true,
        },
        // These Team provider loops are real, but their standalone
        // NodeDaemon-owned AgentSession handles have not yet been proven.
        // Report the narrower truth and fail closed instead of inheriting the
        // generic Team adapter's capabilities.
        "claude" | "kimi" | "pi" => NodeSessionCapabilities {
            start: false,
            resume: false,
            cancel_turn: false,
            stop: false,
        },
        _ => return None,
    })
}

/// Open or resume the provider-native session inside the NodeDaemon process.
/// The concrete adapter consumes the frozen permission mapping; callers cannot
/// substitute a browser-selected sandbox or approval policy.
pub(crate) fn open_node_session(
    session: &AgentSession,
    cwd: &Path,
    display_name: &str,
) -> Result<OpenedNodeSession, String> {
    let capabilities = node_session_capabilities(&session.provider_kind)
        .ok_or_else(|| format!("PROVIDER_CAPABILITY_UNPROVABLE: {}", session.provider_kind))?;
    let resuming = session.native_session_ref.is_some();
    if (resuming && !capabilities.resume) || (!resuming && !capabilities.start) {
        return Err(format!(
            "PROVIDER_RUNTIME_UNSUPPORTED: {} has no proven NodeDaemon-owned {} adapter",
            session.provider_kind,
            if resuming { "resume" } else { "start" }
        ));
    }
    let availability = provider_availability(&session.provider_kind)?;
    if !availability.available {
        return Err(format!(
            "PROVIDER_UNAVAILABLE: {} binary {} is unavailable",
            availability.provider, availability.binary
        ));
    }
    let permission_mapping =
        map_permission(&session.provider_kind, session.effective_permission_ceiling)?;
    match session.provider_kind.as_str() {
        "codex" => {
            let client = CodexAppServerClient::spawn(
                cwd,
                CodexAppServerSpawnOptions {
                    model: None,
                    reasoning_effort: None,
                    service_tier: None,
                    resume_thread_id: session
                        .native_session_ref
                        .as_ref()
                        .map(|native| native.native_session_id.as_str()),
                    member_name: display_name,
                    collaboration_env: &[],
                    plan_mode: false,
                    sandbox: Some(permission_mapping.native_sandbox.as_str()),
                    approval_policy: Some(permission_mapping.native_approval.as_str()),
                },
            )
            .map_err(|error| format!("PROVIDER_SESSION_START_FAILED: {error}"))?;
            let native_session_ref = NativeSessionRef {
                provider: "codex".into(),
                execution_mode: "node_daemon_app_server".into(),
                native_session_id: client.thread_id().to_string(),
                native_locator_kind: "codex_thread".into(),
                provider_version: availability.version_probe,
                adapter_contract_version: "agentfirm-node-session-v1".into(),
                availability: NativeSessionAvailability::Available,
                supports_resume: true,
                last_verified_at: Some(session.opened_at.clone()),
                parent_native_session_id: None,
            };
            Ok(OpenedNodeSession {
                runtime: NodeSessionRuntime::Codex(client),
                native_session_ref,
                permission_mapping,
            })
        }
        provider => Err(format!(
            "PROVIDER_RUNTIME_UNSUPPORTED: {provider} has no proven NodeDaemon-owned session adapter"
        )),
    }
}

/// Capabilities of the existing Team collaboration provider loops. These are
/// deliberately distinct from [`node_session_capabilities`]: proving a Team
/// loop can interrupt its own native transport does not grant the standalone
/// AgentSession fabric that capability.
pub(crate) fn team_loop_capabilities(provider: &str) -> Option<ProviderCapabilities> {
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

/// Freeze the provider-native control primitive before a RuntimeCommand is
/// admitted. Provider loops must execute this plan and settle the durable
/// command from the observed acknowledgement; static capability flags alone
/// never count as conformance evidence.
pub(crate) fn control_plan(
    provider: &str,
    action: ProviderControlAction,
) -> Result<ProviderControlPlan, String> {
    let primitive = match provider {
        "codex" => NativeControlPrimitive::CodexTurnInterrupt,
        "claude" => NativeControlPrimitive::ClaudeAgentSdkInterrupt,
        "kimi" => NativeControlPrimitive::KimiAcpCancel,
        "pi" => NativeControlPrimitive::PiRpcInterrupt,
        _ => return Err(format!("PROVIDER_CONTROL_UNSUPPORTED: {provider}")),
    };
    Ok(ProviderControlPlan {
        provider: provider.to_string(),
        action,
        primitive,
        requires_terminal_ack: true,
    })
}

/// Admit a Team collaboration control request through the canonical runtime
/// command ledger, but keep provider-specific capability selection here at the
/// adapter boundary. Team code supplies intent; it never chooses a native
/// primitive or bypasses the AgentSession/NodeDaemon fence.
pub(crate) fn prepare_team_control_effect(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    source_record_id: &str,
    reason: &str,
    close: bool,
) -> CliResult<ProviderEffectAdmission> {
    let control_plan = control_plan(
        &member.provider,
        if close {
            ProviderControlAction::CloseSession
        } else {
            ProviderControlAction::CancelProviderTurn
        },
    )
    .map_err(CliError::Usage)?;
    let mut admission = crate::prepare_provider_effect_kind(
        ledger,
        member,
        source_record_id,
        reason,
        // Team close is a collaboration lifecycle operation. It may cancel
        // that Team provider turn, but it must never become authority to stop
        // the machine-global AgentSession (which may serve another Team).
        harness_core::agentfirm_api::RuntimeCommandKind::CancelProviderTurn,
        "provider.cancel",
    )?;
    admission.control_plan = Some(control_plan);
    Ok(admission)
}

/// Execute Codex control through the concrete app-server adapter and settle
/// the same durable RuntimeCommand. This is deliberately not an injected
/// closure: the canonical adapter owns the provider-native interrupt mapping,
/// and an exact replay never re-enters the provider transport.
pub(crate) fn execute_codex_team_control(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    source_record_id: &str,
    reason: &str,
    close: bool,
    client: &mut CodexAppServerClient,
    turn_id: &str,
) -> CliResult<()> {
    let admission =
        match prepare_team_control_effect(ledger, member, source_record_id, reason, close) {
            Ok(admission) => admission,
            Err(CliError::Usage(detail))
                if detail.starts_with("RUNTIME_COMMAND_REPLAY_APPLIED:") =>
            {
                return Ok(())
            }
            Err(error) => return Err(error),
        };
    let plan = admission.control_plan.as_ref().ok_or_else(|| {
        CliError::Usage("PROVIDER_CONTROL_UNPROVEN: missing frozen control plan".into())
    })?;
    if plan.provider != "codex" || plan.primitive != NativeControlPrimitive::CodexTurnInterrupt {
        return Err(CliError::Usage(format!(
            "PROVIDER_CONTROL_UNPROVEN: Codex adapter received {:?}",
            plan.primitive
        )));
    }
    match client.interrupt(turn_id) {
        Ok(()) => crate::settle_provider_effect(
            ledger,
            &admission,
            true,
            Some(serde_json::json!({
                "control": if close { "close" } else { "interrupt" },
                "provider_ack": "accepted",
            })),
            None,
        ),
        Err(error) => {
            let error = format!(
                "PROVIDER_CONTROL_FAILED:{}:{:?}:{error}",
                plan.provider, plan.action
            );
            crate::settle_provider_effect(ledger, &admission, false, None, Some(error.clone()))?;
            Err(CliError::Usage(error))
        }
    }
}

pub(crate) fn provider_availability(provider: &str) -> Result<ProviderAvailability, String> {
    let binary = match provider {
        "codex" => "codex",
        "claude" => "claude",
        "kimi" => "kimi",
        "pi" => "pi",
        _ => return Err(format!("PROVIDER_CAPABILITY_UNPROVABLE: {provider}")),
    };
    let output = Command::new(binary).arg("--version").output();
    let (available, version_probe) = match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let version = if stdout.is_empty() { stderr } else { stdout };
            (true, (!version.is_empty()).then_some(version))
        }
        _ => (false, None),
    };
    Ok(ProviderAvailability {
        provider: provider.to_string(),
        binary: binary.to_string(),
        available,
        version_probe,
    })
}

pub(crate) fn map_permission(
    provider: &str,
    requested: PermissionCeiling,
) -> Result<ProviderPermissionMapping, String> {
    if !matches!(provider, "codex" | "claude" | "kimi" | "pi") {
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
    let capabilities = team_loop_capabilities(provider)
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
            let capabilities = team_loop_capabilities(provider).expect("known provider");
            assert!(capabilities.create_attach_resume);
            assert!(capabilities.queue_next_turn);
            assert!(capabilities.interrupt && capabilities.close);
            assert!(capabilities.inspect_state && capabilities.reconcile_effect);
            assert!(map_permission(provider, PermissionCeiling::ReadOnly).is_ok());
            assert!(map_permission(provider, PermissionCeiling::WorkspaceWrite).is_ok());
            for action in [
                ProviderControlAction::CancelProviderTurn,
                ProviderControlAction::CloseSession,
            ] {
                let plan = control_plan(provider, action).expect("known provider control plan");
                assert!(plan.requires_terminal_ack);
                assert_eq!(plan.provider, provider);
            }
        }
        assert!(map_permission("unknown", PermissionCeiling::ReadOnly).is_err());
        for provider in ["claude", "kimi", "pi"] {
            assert!(map_permission(provider, PermissionCeiling::FullAccess).is_err());
        }
        let codex = map_permission("codex", PermissionCeiling::FullAccess).unwrap();
        assert_eq!(codex.native_approval, "on-request");
        assert_ne!(codex.native_approval, "never");
        assert!(control_plan("unknown", ProviderControlAction::CancelProviderTurn).is_err());
    }

    #[test]
    fn provider_binary_probe_is_explicit_and_never_fabricates_availability() {
        for provider in ["codex", "claude", "kimi", "pi"] {
            let availability = provider_availability(provider).expect("known provider");
            assert_eq!(availability.provider, provider);
            assert!(!availability.binary.is_empty());
            if availability.available {
                assert!(availability.version_probe.is_some());
            } else {
                assert!(availability.version_probe.is_none());
            }
        }
        assert!(provider_availability("unknown").is_err());
    }

    #[test]
    fn standalone_node_session_capabilities_are_narrower_than_team_loops() {
        let codex = node_session_capabilities("codex").expect("Codex node adapter");
        assert!(codex.start && codex.resume && codex.stop);
        assert!(
            !codex.cancel_turn,
            "unwired native turn cancel is not advertised"
        );
        for provider in ["claude", "kimi", "pi"] {
            let caps = node_session_capabilities(provider).expect("closed provider tuple");
            assert_eq!(
                caps,
                NodeSessionCapabilities {
                    start: false,
                    resume: false,
                    cancel_turn: false,
                    stop: false,
                },
                "an installed binary alone cannot claim NodeDaemon session conformance for {provider}"
            );
        }
        assert!(node_session_capabilities("unknown").is_none());
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
