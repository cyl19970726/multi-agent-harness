use super::*;

pub(super) fn deepseek_provider_profile() -> ProviderIntegrationProfile {
    finalized_provider_integration_profile(ProviderIntegrationProfile {
        agent_runtime_provider: Some(harness_core::AgentRuntimeProvider(
            "deepseek_harness".to_string(),
        )),
        model_route: None,
        provider: "deepseek_harness".to_string(),
        execution_mode: "deepseek_sdk".to_string(),
        execution_driver: MemberExecutionDriver::HostDriven,
        provider_version: None,
        adapter_contract_version: Some("deepseek-harness-native-v1".to_string()),
        reviewed_provider_versions: vec!["0.1.1-rc.2".to_string()],
        compatibility_status: ProviderCompatibilityStatus::Unknown,
        adapter_reviewed_at: Some("2026-08-23".to_string()),
        compatibility_note: Some(
            "Host-driven native Cordis/AgentHandle bridge over DeepSeek Harness \
             0.1.1-rc.2 (upstream b150a551). It uses ctx.agents.create/resume, \
             agent/inbox/spliced input receipts, Agent.cancel, whenIdle, and \
             provider-owned JSONL persistence. Goal plugins are not loaded."
                .to_string(),
        ),
        interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
        ordinary_message_boundary: OrdinaryMessageBoundary::NextRoundBatched,
        plan_mode: ProviderFeatureMode::Emulated,
        goal_mode: ProviderFeatureMode::Unsupported,
        tool_event_fidelity: ProviderEventFidelity::Structured,
        artifact_event_fidelity: ProviderEventFidelity::Summary,
        supports_cancel: true,
        supports_resume: true,
        observes_native_subagents: false,
        observes_background_tasks: false,
        thinking_transient_only: true,
        control_topology: ControlTopology::EmbeddedSdk,
        composition_fingerprint: profile_composition_fingerprint(
            "deepseek_harness",
            "deepseek_sdk",
            Some("deepseek-harness-native-v1"),
        ),
        capability_fingerprint: None,
        capability_bindings: Vec::new(),
        binding_admission: harness_core::ProviderBindingAdmission::Failed,
        adapter_bridge_revision: Some("dsh-0.1.1-rc.2+b150a551-native-agent-handle-v1".to_string()),
        security_enforcement_locus: SecurityEnforcementLocus {
            kind: SecurityEnforcementLocusKind::ProviderNativePolicy,
            note: Some(
                "DSH sandbox-policy is shared by dsh-bash-sandbox and \
                 dsh-fs-sandbox; the durable Session cwd is the workspace-write root"
                    .to_string(),
            ),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_reviewed_dsh_version_admits_native_lifecycle_bindings() {
        let mut profile = deepseek_provider_profile();
        apply_provider_version(&mut profile, Some("0.1.1-rc.2".to_string()));
        assert_eq!(
            profile.compatibility_status,
            ProviderCompatibilityStatus::Current
        );
        for capability in [
            "open_or_resume",
            "start_cycle",
            "interrupt_current_cycle",
            "observe",
            "close_runtime",
        ] {
            assert!(has_active_verified_provider_capability(
                &profile, capability
            ));
        }
        assert!(profile.supports_resume);
        assert!(profile.supports_cancel);
        assert_eq!(profile.execution_driver, MemberExecutionDriver::HostDriven);
        assert_eq!(profile.goal_mode, ProviderFeatureMode::Unsupported);
    }
}
