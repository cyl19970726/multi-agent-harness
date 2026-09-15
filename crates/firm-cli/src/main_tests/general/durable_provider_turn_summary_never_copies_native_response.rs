#[test]
fn durable_provider_turn_summary_never_copies_native_response() {
    let native_response = "## RESULT\ndone\n## SUMMARY\nprivate provider transcript marker";
    let summary = harness_application::provider_turn_coordination_summary(
        "Kimi",
        7,
        !native_response.trim().is_empty(),
    );

    assert_eq!(
        summary,
        "Kimi provider round 7 completed with authored output; transcript remains provider-native"
    );
    assert!(!summary.contains("private provider transcript marker"));

    // ADR 0076 / review r1 B3. The one catch-all failure summary
    // ("{provider} provider round {round} failed; inspect the provider-native
    // session for details") is deleted: every ending now carries its own title
    // and summary, so a Harness abort or a cycle that never started can no
    // longer claim the provider failed. None of them copies provider output.
    for ending in [
        harness_runtime_contract::CycleEnding::HostAborted {
            detail: native_response.to_string(),
        },
        harness_runtime_contract::CycleEnding::NotStarted {
            code: harness_runtime_contract::CycleRefusalCode::RuntimeClosed,
        },
        harness_runtime_contract::CycleEnding::TransportLost {
            detail: native_response.to_string(),
        },
        harness_runtime_contract::CycleEnding::TerminalUnobserved {
            code: harness_runtime_contract::TerminalUnobservedCode::ProtocolViolation,
            detail: native_response.to_string(),
        },
    ] {
        let summary = ending.action_summary("Kimi", 7);
        assert!(
            !summary.contains("private provider transcript marker"),
            "{ending:?} leaked the native transcript: {summary}"
        );
        assert!(
            !summary.contains("provider round 7 failed"),
            "{ending:?} must not claim the provider failed: {summary}"
        );
    }
    assert!(
        harness_runtime_contract::CycleEnding::NotStarted {
            code: harness_runtime_contract::CycleRefusalCode::RuntimeClosed,
        }
        .action_summary("Kimi", 7)
        .contains("no provider session was touched"),
        "a refused start must not send an operator to a session that never began"
    );
}
