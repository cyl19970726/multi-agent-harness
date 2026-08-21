use super::*;

    #[test]
    fn a_recorded_403_never_discards_the_live_proxy_evidence() {
        // Two paths used to answer the SAME failure differently: the canary
        // called a 403 without a proxy `unknown`, while a RECORDED 403 replaced
        // the probe wholesale and became `unauthorized` — gating a healthy
        // account behind a missing env var, the exact Wave 2 misdiagnosis.
        let recorded = capacity_from_provider_error_actions(
            &[provider_error_action(
                "member-run-1",
                "unix-ms:1000",
                "provider turn failed: api_error (HTTP 403): ",
                Some(&claude_403_status()),
            )],
            "member-run-1",
            "claude",
            "claude_agent_sdk",
            1_500,
            1_000,
        )
        .expect("a structured 403 is recorded evidence");
        assert_eq!(recorded.state, ProviderCapacityState::Unauthorized);

        // No proxy: the canary verdict governs, and the merge must agree.
        let no_proxy = claude_probe_snapshot(None);
        let (canary_state, canary_diagnosis) = claude_canary_diagnosis(
            "API Error: 403 Request not allowed",
            &no_proxy.runtime_context,
        );
        let merged = reconcile_recorded_capacity(no_proxy, recorded.clone());

        assert_eq!(canary_state, ProviderCapacityState::Unknown);
        assert_eq!(
            merged.state, canary_state,
            "the recorded and live paths must not contradict each other on one 403"
        );
        assert_eq!(merged.diagnosis.as_deref(), Some(canary_diagnosis.as_str()));
        assert!(
            !merged.runtime_context.is_empty(),
            "the probe's proxy facts must survive the merge"
        );
        assert_eq!(
            merged.account.source, "oauth_credentials_file",
            "the probe's account boundary must survive the merge"
        );
        assert!(
            merged
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("HTTP(S)_PROXY")),
            "the recorded rejection stays visible as evidence: {:?}",
            merged.detail
        );
        assert!(
            !harness_core::provider_capacity_start_decision(Some(&merged), 1_500, 1_000)
                .is_blocked(),
            "a missing proxy must never gate a start"
        );

        // With a proxy configured, the same recorded 403 DOES implicate the
        // credential, and still keeps the live runtime facts.
        let merged = reconcile_recorded_capacity(
            claude_probe_snapshot(Some("http://127.0.0.1:7897")),
            recorded,
        );
        assert_eq!(merged.state, ProviderCapacityState::Unauthorized);
        assert_eq!(merged.account.source, "oauth_credentials_file");
        assert!(!merged.runtime_context.is_empty());
        assert!(
            harness_core::provider_capacity_start_decision(Some(&merged), 1_500, 1_000)
                .is_blocked()
        );
    }

