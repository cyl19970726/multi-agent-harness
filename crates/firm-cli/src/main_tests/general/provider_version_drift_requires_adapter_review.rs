use super::*;

    #[test]
    fn provider_version_drift_requires_adapter_review() {
        let mut current = team_member_provider_profile("codex");
        apply_provider_version(&mut current, Some("0.148.0-alpha.9".to_string()));
        assert_eq!(
            current.compatibility_status,
            ProviderCompatibilityStatus::Current
        );

        let mut drifted = team_member_provider_profile("codex");
        apply_provider_version(&mut drifted, Some("0.149.0".to_string()));
        assert_eq!(
            drifted.compatibility_status,
            ProviderCompatibilityStatus::ReviewRequired
        );
        assert!(drifted
            .compatibility_note
            .as_deref()
            .is_some_and(|note| note.contains("regenerate protocol schemas")));
    }

