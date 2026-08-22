use super::*;

pub(super) fn provider_compatibility_admission(
    id: &str,
    execution_mode: &str,
    adapter_contract_version: &str,
) -> ProviderCompatibilityAdmission {
    ProviderCompatibilityAdmission {
        id: id.to_string(),
        project_id: "project-1".to_string(),
        store_id: "store-1".to_string(),
        provider: "claude".to_string(),
        execution_mode: execution_mode.to_string(),
        provider_version: "2.1.220".to_string(),
        adapter_contract_version: adapter_contract_version.to_string(),
        policy: firm_core::ProviderCompatibilityAdmissionPolicy::Strict,
        actor: "operator-1".to_string(),
        evidence_refs: vec!["evidence-1".to_string()],
        admitted_at: "unix-ms:1".to_string(),
        lifecycle: ProviderCompatibilityAdmissionLifecycle::Active,
        predecessor_admission_id: None,
        reason: None,
    }
}

pub(super) fn provider_compatibility_test_profile() -> ProviderIntegrationProfile {
    ProviderIntegrationProfile {
        agent_runtime_provider: Some(firm_core::AgentRuntimeProvider("kimi".into())),
        model_route: None,
        provider: "kimi".into(),
        execution_mode: "kimi_acp".into(),
        execution_driver: MemberExecutionDriver::HostDriven,
        provider_version: Some("2.1.220".into()),
        adapter_contract_version: Some("kimi-acp-v1".into()),
        reviewed_provider_versions: Vec::new(),
        compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
        adapter_reviewed_at: None,
        compatibility_note: None,
        interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
        ordinary_message_boundary: OrdinaryMessageBoundary::InTurn,
        plan_mode: ProviderFeatureMode::Emulated,
        goal_mode: ProviderFeatureMode::Emulated,
        tool_event_fidelity: ProviderEventFidelity::Structured,
        artifact_event_fidelity: ProviderEventFidelity::Structured,
        supports_cancel: true,
        supports_resume: true,
        observes_native_subagents: false,
        observes_background_tasks: false,
        thinking_transient_only: true,
        control_topology: firm_core::ControlTopology::default(),
        composition_fingerprint: None,
        capability_fingerprint: None,
        capability_bindings: Vec::new(),
        binding_admission: firm_core::ProviderBindingAdmission::Failed,
        adapter_bridge_revision: None,
        security_enforcement_locus: firm_core::SecurityEnforcementLocus::default(),
    }
}

pub(super) fn external_interactive_test_profile(provider: &str) -> ProviderIntegrationProfile {
    let mut profile = provider_compatibility_test_profile();
    profile.agent_runtime_provider = Some(firm_core::AgentRuntimeProvider(provider.into()));
    profile.provider = provider.into();
    profile.execution_mode = firm_core::EXECUTION_MODE_EXTERNAL_INTERACTIVE.into();
    profile.execution_driver = MemberExecutionDriver::UserDriven;
    profile.provider_version = None;
    profile.adapter_contract_version = None;
    profile.compatibility_status = ProviderCompatibilityStatus::Unknown;
    profile.interaction_mode = ProviderInteractionMode::Unsupported;
    profile.ordinary_message_boundary = OrdinaryMessageBoundary::Unknown;
    profile.plan_mode = ProviderFeatureMode::Unsupported;
    profile.goal_mode = ProviderFeatureMode::Unsupported;
    profile.tool_event_fidelity = ProviderEventFidelity::None;
    profile.artifact_event_fidelity = ProviderEventFidelity::None;
    profile.supports_cancel = false;
    profile.supports_resume = false;
    profile
}

pub(super) fn provider_admission_test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "firm-store-provider-admission-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

pub(super) fn provider_admission_test_store(label: &str) -> HarnessStore {
    HarnessStore::new(provider_admission_test_root(label))
        .with_provider_compatibility_scope("project-1", "store-1")
}
