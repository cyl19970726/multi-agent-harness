use super::*;

    #[test]
    fn only_provider_structured_terminal_metadata_classifies_capacity() {
        let classify = |reason: &str, http_status: Option<i64>| {
            capacity_state_from_provider_terminal(&ProviderTerminalFailure {
                reason: reason.into(),
                http_status,
            })
            .map(|(state, _)| state)
        };

        // Exact transport status integers and a closed reason vocabulary.
        assert_eq!(
            classify("api_error", Some(403)),
            Some(ProviderCapacityState::Unauthorized)
        );
        assert_eq!(
            classify("api_error", Some(429)),
            Some(ProviderCapacityState::Exhausted)
        );
        assert_eq!(
            classify("usage_limit_reached", None),
            Some(ProviderCapacityState::Exhausted)
        );
        assert_eq!(
            classify("auth_error", None),
            Some(ProviderCapacityState::Unauthorized)
        );

        // A neighbouring status is a different status, not a near-match: the
        // old substring rule read "1403" and "4030" as 403.
        for status in [1403, 4030, 500, 404, 200] {
            assert_eq!(
                classify("api_error", Some(status)),
                None,
                "HTTP {status} is not a capacity verdict"
            );
        }
        assert_eq!(classify("transport_disconnected", None), None);
        assert_eq!(classify("unknown_provider_error", None), None);
    }

