use super::*;

    #[test]
    fn duplicate_canonical_member_run_scope_is_never_current_authority() {
        let (store, root) = temp_store("duplicate-canonical-member-scope");
        let created = create_two_member_team_run(&store);
        let member = created.member_runs[0].clone();
        let spec = TeamMemberSpec {
            agent_member_id: member.agent_member_id.clone(),
            name: member.name.clone(),
            role: member.role.clone(),
            provider: member.provider.clone(),
            execution_mode: Some("codex_app_server".into()),
            model: member.model.clone(),
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: member.owned_paths.clone(),
            resume_native_session_id: None,
            initial_work: None,
        };
        let team = store
            .latest_teams()
            .expect("read source Team")
            .remove(&created.team_run.agent_team_id)
            .expect("source Team");
        let host = TeamMemberSpec {
            agent_member_id: team.host_agent_id.clone(),
            name: "ForeignDuplicateHost".into(),
            role: "host".into(),
            provider: "codex".into(),
            execution_mode: Some("codex_app_server".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        };
        ensure_unit_test_canonical_team(
            &store,
            "foreign-duplicate-member-space",
            &team,
            &[host, spec],
        )
        .expect("seed the foreign Team and Membership authority");
        let duplicate = canonical_member_run_admission("foreign-duplicate-member-space", &member);
        store
            .legacy_import_create_trust_member_run_projection(&duplicate.context, duplicate.run)
            .expect("reconstruct duplicate cross-space canonical projection");

        let error = store
            .current_team_run_execution_space(&created.team_run)
            .expect_err("duplicate canonical scope can never be current authority")
            .to_string();
        assert!(
            error.contains("MEMBER_RUN_MATERIALIZATION_MISMATCH")
                && error.contains(&member.id)
                && error.contains("2 canonical Execution Space projections"),
            "exact duplicate must be named: {error}"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

