use super::*;

    #[test]
    fn a_failed_claude_request_without_a_proxy_is_a_runtime_gap_not_an_account_verdict() {
        // Live Wave 2 evidence: auth metadata said logged-in, the request
        // returned 403, and the identical request succeeded through the proxy.
        let no_proxy = vec![ProviderRuntimeContextFact {
            key: "HTTPS_PROXY".into(),
            present: false,
            note: Some("absent".into()),
        }];
        let (state, diagnosis) =
            claude_canary_diagnosis("API Error: 403 Request not allowed", &no_proxy);
        assert_eq!(
            state,
            ProviderCapacityState::Unknown,
            "a missing proxy must not gate a possibly healthy account"
        );
        assert!(diagnosis.contains("PROXY"), "{diagnosis}");

        // With a proxy configured, the same rejection does implicate the
        // credential.
        let with_proxy = vec![ProviderRuntimeContextFact {
            key: "HTTPS_PROXY".into(),
            present: true,
            note: Some("http://127.0.0.1:7897".into()),
        }];
        let (state, _) = claude_canary_diagnosis("401 unauthorized", &with_proxy);
        assert_eq!(state, ProviderCapacityState::Unauthorized);

        let (state, _) = claude_canary_diagnosis("429 rate limit exceeded", &with_proxy);
        assert_eq!(state, ProviderCapacityState::Exhausted);

        // An unreviewed failure stays unknown instead of guessing.
        let (state, _) = claude_canary_diagnosis("something unfamiliar", &with_proxy);
        assert_eq!(state, ProviderCapacityState::Unknown);
    }

