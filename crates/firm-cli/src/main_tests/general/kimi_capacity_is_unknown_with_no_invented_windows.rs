use super::*;

    #[test]
    fn kimi_capacity_is_unknown_with_no_invented_windows() {
        let snapshot = kimi_capacity_probe("kimi_acp");

        assert_eq!(snapshot.state, ProviderCapacityState::Unknown);
        assert_eq!(
            snapshot.evidence_source,
            ProviderCapacityEvidence::NotExposed
        );
        assert_eq!(snapshot.confidence, ProviderCapacityConfidence::Unknown);
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.reset_at, None);
        let encoded = serde_json::to_string(&snapshot).expect("serialize");
        assert!(
            !encoded.contains("used_percent\":0"),
            "an absent quota API must not become a zero percentage: {encoded}"
        );
    }

