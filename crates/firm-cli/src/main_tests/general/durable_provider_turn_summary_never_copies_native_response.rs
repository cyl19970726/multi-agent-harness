use super::*;

#[test]
fn durable_provider_turn_summary_never_copies_native_response() {
    let native_response = "## RESULT\ndone\n## SUMMARY\nprivate provider transcript marker";
    let summary = provider_turn_coordination_summary("Kimi", 7, !native_response.trim().is_empty());

    assert_eq!(
        summary,
        "Kimi provider round 7 completed with authored output; transcript remains provider-native"
    );
    assert!(!summary.contains("private provider transcript marker"));
    assert_eq!(
        provider_turn_failure_summary("Claude", 3),
        "Claude provider round 3 failed; inspect the provider-native session for details"
    );
}
