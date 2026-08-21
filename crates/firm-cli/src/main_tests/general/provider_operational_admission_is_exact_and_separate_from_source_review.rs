use super::*;

    #[test]
    fn provider_operational_admission_is_exact_and_separate_from_source_review() {
        let root = std::env::temp_dir().join(format!(
            "harness-provider-admission-cli-test-{}",
            generated_id("exact")
        ));
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-a", "space-a");
        let mut profile = team_member_provider_profile_for_mode("codex", Some("codex_app_server"));
        apply_provider_version(&mut profile, Some("9.9.9".into()));
        assert_eq!(
            profile.compatibility_status,
            ProviderCompatibilityStatus::ReviewRequired
        );
        let admission = ProviderCompatibilityAdmission {
            id: "admission-exact".into(),
            project_id: "project-a".into(),
            store_id: "space-a".into(),
            provider: "codex".into(),
            execution_mode: "codex_app_server".into(),
            provider_version: "9.9.9".into(),
            adapter_contract_version: "codex-app-server-v1".into(),
            policy: ProviderCompatibilityAdmissionPolicy::Strict,
            actor: "operator".into(),
            evidence_refs: vec!["evidence:test".into()],
            admitted_at: "unix-ms:1".into(),
            lifecycle: ProviderCompatibilityAdmissionLifecycle::Active,
            predecessor_admission_id: None,
            reason: None,
        };
        store
            .admit_provider_compatibility_admission(&admission)
            .unwrap();
        let allowed = resolve_provider_compatibility(&store, &profile, None).unwrap();
        assert!(allowed.allowed);
        assert!(!allowed.needs_review);
        assert_eq!(allowed.source, "operational_admission");
        assert_eq!(
            profile.compatibility_status,
            ProviderCompatibilityStatus::ReviewRequired
        );

        profile.execution_mode = "codex_exec".into();
        assert!(
            !resolve_provider_compatibility(&store, &profile, None)
                .unwrap()
                .allowed
        );
        profile.execution_mode = "codex_app_server".into();
        let isolated = HarnessStore::new(root.join("other-store"));
        assert!(
            !resolve_provider_compatibility(&isolated, &profile, None)
                .unwrap()
                .allowed
        );
        let same_store_other_project =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-b", "space-a");
        assert!(
            !resolve_provider_compatibility(&same_store_other_project, &profile, None)
                .unwrap()
                .allowed
        );
        let migrated_scope =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-a", "space-b");
        assert!(
            !resolve_provider_compatibility(&migrated_scope, &profile, None)
                .unwrap()
                .allowed
        );
        let _ = std::fs::remove_dir_all(root);
    }

