use super::*;

    #[test]
    fn recorded_provider_errors_expire_with_the_capacity_ttl() {
        let actions = vec![provider_error_action(
            "member-run-1",
            "unix-ms:1000",
            "provider turn failed: api_error (HTTP 403)",
            Some(&claude_403_status()),
        )];

        let fresh = capacity_from_provider_error_actions(
            &actions,
            "member-run-1",
            "claude",
            "claude_agent_sdk",
            1_500,
            1_000,
        )
        .expect("a fresh 403 is observable capacity");
        assert_eq!(fresh.state, ProviderCapacityState::Unauthorized);
        assert_eq!(
            fresh.evidence_source,
            ProviderCapacityEvidence::ProviderError
        );
        assert_eq!(fresh.observed_unix_ms, 1_000);

        // Past the TTL the same row is no longer evidence of "now".
        assert!(capacity_from_provider_error_actions(
            &actions,
            "member-run-1",
            "claude",
            "claude_agent_sdk",
            100_000,
            1_000,
        )
        .is_none());

        // Another member's failure is never borrowed.
        assert!(capacity_from_provider_error_actions(
            &actions,
            "member-run-2",
            "claude",
            "claude_agent_sdk",
            1_500,
            1_000,
        )
        .is_none());
    }

