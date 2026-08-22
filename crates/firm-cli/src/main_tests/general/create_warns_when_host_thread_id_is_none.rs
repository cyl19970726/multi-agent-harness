use super::*;

#[test]
fn create_warns_when_host_thread_id_is_none() {
    let (store, root) = temp_store("warn-unbound-create");
    // create with a surface but no thread-id — must still succeed (not
    // hard-error), and the run must record its missing binding.
    let created = create_team_run(
        &store,
        None,
        None,
        None,
        "Deliver an artifact",
        None,
        "test-surface",
        None, // no host_thread_id → L0 warning
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
    assert!(
        created.team_run.host_thread_id.is_none(),
        "run must record absent host_thread_id"
    );
    assert_eq!(created.team_run.host_surface, "test-surface");
    let _ = std::fs::remove_dir_all(root);
}
