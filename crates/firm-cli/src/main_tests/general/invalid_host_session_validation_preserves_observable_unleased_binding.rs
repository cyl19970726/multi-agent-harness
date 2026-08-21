use super::*;

    #[test]
    fn invalid_host_session_validation_preserves_observable_unleased_binding() {
        let (store, root) = temp_store("invalid-bind-lease");
        let created = create_two_member_team_run(&store);
        let validator = FakeHostSessionValidator {
            receipt: Err("native session not discoverable".into()),
        };
        let result = bind_host_with_validator(
            &store,
            &created.team_run.id,
            "claude-code",
            "manual-thread",
            30_000,
            &validator,
            100,
        )
        .expect("binding remains observable");
        assert_eq!(result.run.host_surface, "claude");
        assert_eq!(result.run.host_thread_id.as_deref(), Some("manual-thread"));
        assert!(result.lease.is_none());
        assert!(result
            .validation_warning
            .as_deref()
            .is_some_and(|warning| warning.contains("remains unleased")));
        assert!(store
            .latest_host_binding_lease(&created.team_run.id)
            .expect("lease read")
            .is_none());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

