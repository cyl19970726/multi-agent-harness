use super::*;

    #[test]
    fn create_auto_binds_from_star_harness_env() {
        let (store, root) = temp_store("auto-bind-create");
        // Simulate star-harness SessionStart having set both env vars
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
            None,
            None,
            None,
            None,
            &[TeamMemberSpec {
                agent_member_id: "agent-only-member-b".into(),
                name: "OnlyMember".into(),
                role: "sole".into(),
                provider: "codex".into(),
                execution_mode: Some("codex_app_server".into()),
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

