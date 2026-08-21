use super::*;

    #[test]
    fn kimi_cancel_capability_is_version_specific() {
        let mut reviewed = team_member_provider_profile("kimi");
        apply_provider_version(&mut reviewed, Some("0.27.0".to_string()));
        assert!(!reviewed.supports_cancel);
        assert_eq!(
            reviewed.compatibility_status,
            ProviderCompatibilityStatus::Current
        );

        let mut method_missing = team_member_provider_profile("kimi");
        apply_provider_version(&mut method_missing, Some("0.29.1".to_string()));
        assert!(!method_missing.supports_cancel);
        assert_eq!(
            method_missing.compatibility_status,
            ProviderCompatibilityStatus::ReviewRequired
        );

        let mut current = team_member_provider_profile("kimi");
        apply_provider_version(&mut current, Some("0.31.0".to_string()));
        assert!(!current.supports_cancel);
        assert_eq!(current.goal_mode, ProviderFeatureMode::Native);
        assert_eq!(
            current.compatibility_status,
            ProviderCompatibilityStatus::Current
        );

        let mut current_patch = team_member_provider_profile("kimi");
        apply_provider_version(&mut current_patch, Some("0.31.1".to_string()));
        assert!(!current_patch.supports_cancel);
        assert_eq!(current_patch.goal_mode, ProviderFeatureMode::Native);
        assert_eq!(
            current_patch.compatibility_status,
            ProviderCompatibilityStatus::Current
        );

        let mut current_0361 = team_member_provider_profile("kimi");
        apply_provider_version(&mut current_0361, Some("0.36.1".to_string()));
        assert!(current_0361.supports_cancel);
        assert_eq!(current_0361.goal_mode, ProviderFeatureMode::Emulated);
        assert_eq!(
            current_0361.compatibility_status,
            ProviderCompatibilityStatus::Current
        );
        assert!(current_0361.capability_bindings.iter().any(|binding| {
            binding.capability == "interrupt_current_cycle"
                && binding.status == harness_core::ProviderCapabilityStatus::Verified
                && binding.admission == harness_core::ProviderBindingAdmission::Active
                && binding.provider_version.as_deref() == Some("0.36.1")
        }));
        harness_core::Validate::validate(&current_0361)
            .expect("reviewed Kimi 0.36.1 profile validates");

        let mut future = team_member_provider_profile("kimi");
        apply_provider_version(&mut future, Some("0.32.0".to_string()));
        // 0.32.0 is adapter-reviewed for prompt delivery/resume/mail, but
        // cancel and native goal mode stay unclaimed (fail-closed per
        // capability, not inherited from 0.31.x).
        assert!(!future.supports_cancel);
        assert_eq!(future.goal_mode, ProviderFeatureMode::Emulated);
        assert_eq!(
            future.compatibility_status,
            ProviderCompatibilityStatus::Current
        );

        let mut unreviewed = team_member_provider_profile("kimi");
        // 0.34.0 is ahead of the reviewed adapter list (0.27.0..0.33.0) and
        // must fail closed to ReviewRequired rather than inherit claims.
        apply_provider_version(&mut unreviewed, Some("0.34.0".to_string()));
        assert_eq!(
            unreviewed.compatibility_status,
            ProviderCompatibilityStatus::ReviewRequired
        );
    }

