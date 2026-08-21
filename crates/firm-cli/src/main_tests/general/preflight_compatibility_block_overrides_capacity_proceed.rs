use super::*;

    #[test]
    fn preflight_compatibility_block_overrides_capacity_proceed() {
        let capacity = harness_core::ProviderCapacityStartDecision::Proceed {
            reason: "capacity available".to_string(),
        };
        let compatibility = ProviderCompatibilityResolution {
            allowed: false,
            needs_review: true,
            status: ProviderCompatibilityStatus::ReviewRequired,
            source: "adapter_compatibility",
            policy: None,
            admission: None,
            probe_error: None,
            warning: None,
        };
        let decision = provider_preflight_start_decision(&capacity, &compatibility);
        assert_eq!(decision["decision"], "block");
        assert_eq!(decision["gate"], "provider_compatibility");
    }

