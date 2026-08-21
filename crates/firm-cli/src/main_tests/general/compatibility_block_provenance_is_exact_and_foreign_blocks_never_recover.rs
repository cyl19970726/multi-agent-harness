use super::*;

    #[test]
    fn compatibility_block_provenance_is_exact_and_foreign_blocks_never_recover() {
        let (store, root) = temp_store("compatibility-block-provenance");
        let created = create_two_member_team_run(&store);
        let mut member = created.member_runs[0].clone();
        member.status = MemberRunStatus::Blocked;
        let mut profile = team_member_provider_profile_for_mode("codex", Some("codex_app_server"));
        apply_provider_version(&mut profile, Some("9.9.9".into()));
        let exact = compatibility_block_action(&member, &profile, 1);
        store
            .append_member_action(&exact)
            .expect("append hostile exact-looking audit action");
        assert!(
            !compatibility_block_matches_current_tuple(&member, &profile),
            "a forged exact-looking MemberAction cannot authorize recovery"
        );
        member.provider_profile = Some(profile.clone());
        member.provider_compatibility_block_cause =
            Some(compatibility_test_cause(&member, &profile));
        assert!(compatibility_block_matches_current_tuple(&member, &profile));
        assert_eq!(
            compatibility_recovery_status(&store, &member).expect("unbound recovery status"),
            MemberRunStatus::Idle
        );
        let mut native = member.clone();
        native.native_session = Some(capacity_test_session());
        assert_eq!(
            compatibility_recovery_status(&store, &native).expect("native recovery status"),
            MemberRunStatus::Disconnected
        );

        let mut wrong_tuple = profile.clone();
        wrong_tuple.provider_version = Some("9.9.10".into());
        assert!(!compatibility_block_matches_current_tuple(
            &member,
            &wrong_tuple
        ));

        for (index, (action_type, summary)) in [
            ("operator_blocked", exact.summary.clone()),
            ("provider_capacity_blocked", exact.summary.clone()),
            ("provider_compatibility_blocked", "not parseable".into()),
        ]
        .into_iter()
        .enumerate()
        {
            let mut foreign = exact.clone();
            foreign.id = format!("hostile-action-{index}");
            foreign.seq = index as u64 + 2;
            foreign.action_type = action_type.into();
            foreign.summary = summary;
            store
                .append_member_action(&foreign)
                .expect("append hostile audit action");
            assert!(
                compatibility_block_matches_current_tuple(&member, &profile),
                "MemberAction prose is audit-only and cannot alter typed authority"
            );
        }
        std::fs::remove_dir_all(root).expect("cleanup");
    }

