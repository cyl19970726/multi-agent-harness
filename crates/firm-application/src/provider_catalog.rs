use serde::Serialize;

/// Closed implementation selector for persistent Agent Team runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRuntimeKind {
    Codex,
    Claude,
    Kimi,
    Pi,
}

/// One current persistent Agent Team binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TeamRuntimeBinding {
    pub execution_mode: &'static str,
    pub binding: TeamRuntimeKind,
}

/// Closed selector for an explicitly external Host notification transport.
/// It is never the standard managed Team Host runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalHostTransportKind {
    ClaudeCli,
    KimiAcp,
}

/// An external Host transport is not a Team fallback and owns no coordination
/// lifecycle, AgentSession, provider receipt, or acknowledgement authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExternalHostTransportBinding {
    pub execution_mode: &'static str,
    pub binding: ExternalHostTransportKind,
}

/// Closed selector for the still-current `/v1/agents/*` compatibility route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityDeliveryKind {
    CodexExec,
    ClaudeCli,
    KimiExec,
}

/// Compatibility delivery is neither a Team nor Host execution contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CompatibilityDeliveryBinding {
    pub execution_mode: &'static str,
    pub binding: CompatibilityDeliveryKind,
}

/// Decode-only historical spelling. It intentionally has no effect selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HistoricalProviderMode {
    pub execution_mode: &'static str,
}

/// Compile-time catalog row for one production coding-agent provider.
///
/// Historical aliases are decode-only. A current Host or compatibility route
/// may use the same provider CLI spelling, but it does so through a distinct
/// typed field rather than turning historical metadata into executable truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProviderDescriptor {
    pub provider: &'static str,
    pub team: TeamRuntimeBinding,
    pub external_host_transport: Option<ExternalHostTransportBinding>,
    pub direct_delivery_compatibility: Option<CompatibilityDeliveryBinding>,
    pub event_decoder: bool,
    pub version_probe: bool,
    pub capacity_probe: bool,
    pub native_session_locator: bool,
    pub standalone_node_session: bool,
    pub historical_modes: &'static [HistoricalProviderMode],
}

pub const PROVIDERS: [ProviderDescriptor; 4] = [
    ProviderDescriptor {
        provider: "codex",
        team: TeamRuntimeBinding {
            execution_mode: "codex_app_server",
            binding: TeamRuntimeKind::Codex,
        },
        external_host_transport: None,
        direct_delivery_compatibility: Some(CompatibilityDeliveryBinding {
            execution_mode: "codex_exec",
            binding: CompatibilityDeliveryKind::CodexExec,
        }),
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: true,
        historical_modes: &[HistoricalProviderMode {
            execution_mode: "codex_exec",
        }],
    },
    ProviderDescriptor {
        provider: "claude",
        team: TeamRuntimeBinding {
            execution_mode: "claude_agent_sdk",
            binding: TeamRuntimeKind::Claude,
        },
        external_host_transport: Some(ExternalHostTransportBinding {
            execution_mode: "claude_cli",
            binding: ExternalHostTransportKind::ClaudeCli,
        }),
        direct_delivery_compatibility: Some(CompatibilityDeliveryBinding {
            execution_mode: "claude_cli",
            binding: CompatibilityDeliveryKind::ClaudeCli,
        }),
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: false,
        historical_modes: &[HistoricalProviderMode {
            execution_mode: "claude_cli",
        }],
    },
    ProviderDescriptor {
        provider: "kimi",
        team: TeamRuntimeBinding {
            execution_mode: "kimi_acp",
            binding: TeamRuntimeKind::Kimi,
        },
        external_host_transport: Some(ExternalHostTransportBinding {
            execution_mode: "kimi_acp",
            binding: ExternalHostTransportKind::KimiAcp,
        }),
        direct_delivery_compatibility: Some(CompatibilityDeliveryBinding {
            execution_mode: "kimi_exec",
            binding: CompatibilityDeliveryKind::KimiExec,
        }),
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: false,
        historical_modes: &[HistoricalProviderMode {
            execution_mode: "kimi_exec",
        }],
    },
    ProviderDescriptor {
        provider: "pi",
        team: TeamRuntimeBinding {
            execution_mode: "pi_rpc",
            binding: TeamRuntimeKind::Pi,
        },
        external_host_transport: None,
        direct_delivery_compatibility: None,
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: false,
        historical_modes: &[],
    },
];

pub fn provider_descriptor(provider: &str) -> Option<&'static ProviderDescriptor> {
    PROVIDERS.iter().find(|entry| entry.provider == provider)
}

pub fn team_runtime_kind(provider: &str, execution_mode: Option<&str>) -> Option<TeamRuntimeKind> {
    let descriptor = provider_descriptor(provider)?;
    let requested = execution_mode.unwrap_or(descriptor.team.execution_mode);
    if requested != descriptor.team.execution_mode {
        return None;
    }
    Some(descriptor.team.binding)
}

/// Resolve the only current direct-delivery compatibility effect for a
/// provider. Historical aliases never participate in this selection.
pub fn compatibility_delivery_kind(provider: &str) -> Option<CompatibilityDeliveryKind> {
    provider_descriptor(provider)?
        .direct_delivery_compatibility
        .map(|binding| binding.binding)
}

pub fn compatibility_provider_names() -> impl Iterator<Item = &'static str> {
    PROVIDERS.iter().filter_map(|descriptor| {
        descriptor
            .direct_delivery_compatibility
            .map(|_| descriptor.provider)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_closed_unique_and_complete_for_four_production_providers() {
        assert_eq!(PROVIDERS.len(), 4);
        assert_eq!(
            PROVIDERS
                .iter()
                .map(|entry| entry.provider)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["claude", "codex", "kimi", "pi"])
        );
        for descriptor in PROVIDERS {
            assert!(descriptor.event_decoder);
            assert!(descriptor.version_probe);
            assert!(descriptor.capacity_probe);
            assert!(descriptor.native_session_locator);
        }
        assert!(provider_descriptor("deepseek").is_none());
    }

    #[test]
    fn only_exact_team_modes_resolve_to_executable_team_bindings() {
        for descriptor in PROVIDERS {
            assert_eq!(
                team_runtime_kind(descriptor.provider, None),
                Some(descriptor.team.binding)
            );
            assert_eq!(
                team_runtime_kind(descriptor.provider, Some(descriptor.team.execution_mode)),
                Some(descriptor.team.binding)
            );
            for historical in descriptor.historical_modes {
                assert_eq!(
                    team_runtime_kind(descriptor.provider, Some(historical.execution_mode)),
                    None
                );
            }
        }
        assert_eq!(team_runtime_kind("unknown", None), None);
    }

    #[test]
    fn host_compatibility_and_historical_surfaces_remain_distinct() {
        let codex = provider_descriptor("codex").unwrap();
        assert!(codex.external_host_transport.is_none());
        assert_eq!(
            codex.direct_delivery_compatibility.unwrap().execution_mode,
            "codex_exec"
        );
        assert_eq!(codex.historical_modes[0].execution_mode, "codex_exec");

        let claude = provider_descriptor("claude").unwrap();
        assert_eq!(
            claude.external_host_transport.unwrap().execution_mode,
            "claude_cli"
        );
        assert_eq!(
            claude.direct_delivery_compatibility.unwrap().execution_mode,
            "claude_cli"
        );
        assert_eq!(claude.historical_modes[0].execution_mode, "claude_cli");

        let kimi = provider_descriptor("kimi").unwrap();
        assert_eq!(
            kimi.external_host_transport.unwrap().execution_mode,
            "kimi_acp"
        );
        assert_eq!(
            kimi.direct_delivery_compatibility.unwrap().execution_mode,
            "kimi_exec"
        );

        assert_eq!(
            compatibility_provider_names().collect::<Vec<_>>(),
            vec!["codex", "claude", "kimi"]
        );
        assert_eq!(
            compatibility_delivery_kind("codex"),
            Some(CompatibilityDeliveryKind::CodexExec)
        );
        assert_eq!(compatibility_delivery_kind("pi"), None);

        let pi = provider_descriptor("pi").unwrap();
        assert!(pi.external_host_transport.is_none());
        assert!(pi.direct_delivery_compatibility.is_none());
    }

    #[test]
    fn standalone_node_session_support_is_declared_without_team_inference() {
        for descriptor in PROVIDERS {
            assert_eq!(
                descriptor.standalone_node_session,
                descriptor.provider == "codex"
            );
        }
    }
}
