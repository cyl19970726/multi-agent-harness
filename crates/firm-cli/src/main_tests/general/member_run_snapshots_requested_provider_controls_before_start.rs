use super::*;

#[test]
fn member_run_snapshots_requested_provider_controls_before_start() {
    let member = TeamMemberSpec {
        agent_member_id: "agent-controlled-builder".into(),
        name: "ControlledBuilder".into(),
        role: "builder".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: Some("gpt-5.6-sol".into()),
        effort: Some("max".into()),
        service_tier: Some("priority".into()),
        provider_cwd_hint: None,
        owned_paths: Vec::new(),
        resume_native_session_id: None,
        initial_work: None,
    };

    let run = build_member_run_for_team(None, "team-run-controls", &member)
        .expect("build ProviderRuntimeProjection");

    assert_eq!(run.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(
        run.provider_controls.model.requested.as_deref(),
        Some("gpt-5.6-sol")
    );
    assert_eq!(
        run.provider_controls.reasoning_effort.requested.as_deref(),
        Some("max")
    );
    assert_eq!(
        run.provider_controls.service_tier.requested.as_deref(),
        Some("priority")
    );
    assert_eq!(
        run.provider_controls.model.status,
        harness_core::ProviderControlStatus::Requested
    );
    assert_eq!(
        run.provider_controls.reasoning_effort.status,
        harness_core::ProviderControlStatus::Requested
    );
    assert_eq!(
        run.provider_controls.service_tier.status,
        harness_core::ProviderControlStatus::Requested
    );
}
