use super::*;

    #[test]
    fn advisory_admission_never_overrides_probe_failure_and_migrates() {
        let root = std::env::temp_dir().join(format!(
            "harness-provider-admission-cli-test-{}",
            generated_id("advisory")
        ));
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-a", "space-a");
        let mut profile = team_member_provider_profile_for_mode("kimi", Some("kimi_acp"));
        apply_provider_version(&mut profile, Some("99.0.0".into()));
        let admission = ProviderCompatibilityAdmission {
            id: "admission-advisory".into(),
            project_id: "project-a".into(),
            store_id: "space-a".into(),
            provider: "kimi".into(),
            execution_mode: "kimi_acp".into(),
            provider_version: "99.0.0".into(),
            adapter_contract_version: "kimi-acp-v1".into(),
            policy: ProviderCompatibilityAdmissionPolicy::Advisory,
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
        let advisory = resolve_provider_compatibility(&store, &profile, None).unwrap();
        assert!(advisory.allowed);
        assert!(advisory.needs_review);
        assert!(
            !resolve_provider_compatibility(&store, &profile, Some("probe failed"))
                .unwrap()
                .allowed
        );
        profile.compatibility_status = ProviderCompatibilityStatus::Incompatible;
        assert!(
            !resolve_provider_compatibility(&store, &profile, None)
                .unwrap()
                .allowed
        );
        assert!(EXECUTION_LEDGER_NAMES.contains(&"provider_compatibility_admissions.jsonl"));
        let _ = std::fs::remove_dir_all(root);
    }

