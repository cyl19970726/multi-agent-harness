use super::*;

    #[test]
    fn create_refuses_partial_star_harness_env() {
        let (store, root) = temp_store("partial-env-create");
        // Simulate partial env: SURFACE set but THREAD_ID missing.
        // CLI must NOT auto-bind and must fall back to explicit arg (or "cli" default).
        let host_surface = "kimi-cli".to_string();
        let host_thread_id: Option<String> = None;
        // Partial env should resolve thread_id to None.
        assert!(host_thread_id.is_none());

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
                agent_member_id: "agent-only-member-c".into(),
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
        assert!(
            created.team_run.host_thread_id.is_none(),
            "partial env must not auto-bind"
        );
        let _ = std::fs::remove_dir_all(root);
    }

