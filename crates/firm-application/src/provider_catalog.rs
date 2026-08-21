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

/// One current execution-mode binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeModeDescriptor {
    pub execution_mode: &'static str,
    pub team_runtime: Option<TeamRuntimeKind>,
}

/// Compile-time catalog row for one production coding-agent provider.
///
/// Historical aliases are decode-only. A current Host or compatibility route
/// may use the same provider CLI spelling, but it does so through a distinct
/// typed field rather than turning historical metadata into executable truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProviderDescriptor {
    pub provider: &'static str,
    pub team: RuntimeModeDescriptor,
    pub headless_host: Option<RuntimeModeDescriptor>,
    pub direct_delivery_compatibility: Option<RuntimeModeDescriptor>,
    pub event_decoder: bool,
    pub version_probe: bool,
    pub capacity_probe: bool,
    pub native_session_locator: bool,
    pub standalone_node_session: bool,
    pub historical_aliases: &'static [&'static str],
}

pub const PROVIDERS: [ProviderDescriptor; 4] = [
    ProviderDescriptor {
        provider: "codex",
        team: RuntimeModeDescriptor {
            execution_mode: "codex_app_server",
            team_runtime: Some(TeamRuntimeKind::Codex),
        },
        headless_host: None,
        direct_delivery_compatibility: Some(RuntimeModeDescriptor {
            execution_mode: "codex_exec",
            team_runtime: None,
        }),
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: true,
        historical_aliases: &["codex_exec"],
    },
    ProviderDescriptor {
        provider: "claude",
        team: RuntimeModeDescriptor {
            execution_mode: "claude_agent_sdk",
            team_runtime: Some(TeamRuntimeKind::Claude),
        },
        headless_host: Some(RuntimeModeDescriptor {
            execution_mode: "claude_cli",
            team_runtime: None,
        }),
        direct_delivery_compatibility: Some(RuntimeModeDescriptor {
            execution_mode: "claude_cli",
            team_runtime: None,
        }),
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: false,
        historical_aliases: &["claude_cli"],
    },
    ProviderDescriptor {
        provider: "kimi",
        team: RuntimeModeDescriptor {
            execution_mode: "kimi_acp",
            team_runtime: Some(TeamRuntimeKind::Kimi),
        },
        headless_host: Some(RuntimeModeDescriptor {
            execution_mode: "kimi_acp",
            team_runtime: None,
        }),
        direct_delivery_compatibility: Some(RuntimeModeDescriptor {
            execution_mode: "kimi_exec",
            team_runtime: None,
        }),
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: false,
        historical_aliases: &["kimi_exec"],
    },
    ProviderDescriptor {
        provider: "pi",
        team: RuntimeModeDescriptor {
            execution_mode: "pi_rpc",
            team_runtime: Some(TeamRuntimeKind::Pi),
        },
        headless_host: None,
        direct_delivery_compatibility: None,
        event_decoder: true,
        version_probe: true,
        capacity_probe: true,
        native_session_locator: true,
        standalone_node_session: false,
        historical_aliases: &[],
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
    descriptor.team.team_runtime
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
            assert!(descriptor.team.team_runtime.is_some());
        }
        assert!(provider_descriptor("deepseek").is_none());
    }

    #[test]
    fn only_exact_team_modes_resolve_to_executable_team_bindings() {
        for descriptor in PROVIDERS {
            assert_eq!(
                team_runtime_kind(descriptor.provider, None),
                descriptor.team.team_runtime
            );
            assert_eq!(
                team_runtime_kind(descriptor.provider, Some(descriptor.team.execution_mode)),
                descriptor.team.team_runtime
            );
            for historical in descriptor.historical_aliases {
                assert_eq!(
                    team_runtime_kind(descriptor.provider, Some(historical)),
                    None
                );
            }
        }
        assert_eq!(team_runtime_kind("unknown", None), None);
    }

    #[test]
    fn host_compatibility_and_historical_surfaces_remain_distinct() {
        let codex = provider_descriptor("codex").unwrap();
        assert!(codex.headless_host.is_none());
        assert_eq!(
            codex.direct_delivery_compatibility.unwrap().execution_mode,
            "codex_exec"
        );
        assert!(codex.historical_aliases.contains(&"codex_exec"));

        let claude = provider_descriptor("claude").unwrap();
        assert_eq!(claude.headless_host.unwrap().execution_mode, "claude_cli");
        assert_eq!(
            claude.direct_delivery_compatibility.unwrap().execution_mode,
            "claude_cli"
        );
        assert!(claude.historical_aliases.contains(&"claude_cli"));

        let kimi = provider_descriptor("kimi").unwrap();
        assert_eq!(kimi.headless_host.unwrap().execution_mode, "kimi_acp");
        assert_eq!(
            kimi.direct_delivery_compatibility.unwrap().execution_mode,
            "kimi_exec"
        );

        let pi = provider_descriptor("pi").unwrap();
        assert!(pi.headless_host.is_none());
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
