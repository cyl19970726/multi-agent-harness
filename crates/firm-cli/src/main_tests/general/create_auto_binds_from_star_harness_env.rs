use super::*;

#[test]
fn create_auto_binds_from_star_harness_env() {
    let (store, root) = temp_store("auto-bind-create");
    // Simulate the operator's shell having exported both STAR_HARNESS_HOST_*
    // vars (the retired plugin hook used to set them; ADR 0063)
    // by directly seeding the values the CLI resolution would produce.
    let host_surface = "kimi-cli".to_string();
    let host_thread_id = Some("thread-xyz".to_string());
    let created = create_team_run(
        &store,
        None,
        None,
        None,
        "Deliver an artifact",
        None,
        &host_surface,
        host_thread_id,
        HostControlMode::ExternalInteractive,
        None,
        None,
        None,
        None,
        &[TeamMemberSpec {
            agent_member_id: "host".into(),
            name: "OnlyMember".into(),
            role: "sole".into(),
            provider: "codex".into(),
            execution_mode: Some(EXECUTION_MODE_EXTERNAL_INTERACTIVE.into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: vec![],
            resume_native_session_id: None,
            initial_work: None,
        }],
    )
    .expect("create succeeds");
    assert_eq!(
        created.team_run.host_thread_id.as_deref(),
        Some("thread-xyz"),
        "auto-bind must record thread-id from env"
    );
    assert_eq!(created.team_run.host_surface, "kimi-cli");
    let _ = std::fs::remove_dir_all(root);
}
