use super::*;

    #[test]
    fn an_unclassifiable_error_never_hides_a_classifiable_one() {
        // The empty-report rule manufactures unclassifiable `provider_error`
        // rows. Selecting the NEWEST row and classifying afterwards would let
        // one of them bury a real 403 recorded seconds earlier, and the member
        // would start and burn its Assignment on a rejected credential.
        // Three unclassifiable rows stacked on top of the real failure: the
        // search must walk PAST all of them, not stop at the newest.
        let mut actions = vec![provider_error_action(
            "member-run-1",
            "unix-ms:1000",
            "provider turn failed: api_error (HTTP 403): ",
            Some(&claude_403_status()),
        )];
        for (offset, summary) in [
            "provider turn failed: empty_final_report (the provider ended the round without an \
             agent message): ",
            "provider turn failed: unknown_provider_error: transport disconnected",
            "provider turn failed: empty_final_report: ",
        ]
        .iter()
        .enumerate()
        {
            let structured = (offset == 1).then(|| {
                ProviderTerminalFailure {
                    reason: "unknown_provider_error".into(),
                    http_status: None,
                }
                .to_provider_status()
            });
            actions.push(provider_error_action(
                "member-run-1",
                &format!("unix-ms:{}", 1_100 + offset),
                summary,
                structured.as_deref(),
            ));
        }

        let snapshot = capacity_from_provider_error_actions(
            &actions,
            "member-run-1",
            "claude",
            "claude_agent_sdk",
            1_500,
            1_000,
        )
        .expect("the classifiable 403 must still be found");

        assert_eq!(snapshot.state, ProviderCapacityState::Unauthorized);
        assert_eq!(snapshot.observed_unix_ms, 1_000);
    }

