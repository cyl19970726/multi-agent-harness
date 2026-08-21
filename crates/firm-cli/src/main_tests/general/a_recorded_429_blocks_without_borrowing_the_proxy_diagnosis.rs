use super::*;

#[test]
fn a_recorded_429_blocks_without_borrowing_the_proxy_diagnosis() {
    // A spent quota is not caused by a missing proxy. The probe sets its
    // missing-proxy diagnosis whenever no proxy is configured, so an
    // unguarded `recorded.diagnosis.or(probe.diagnosis)` blocked correctly
    // as `exhausted` while telling the operator to go fix their proxy.
    let recorded = capacity_from_provider_error_actions(
        &[provider_error_action(
            "member-run-1",
            "unix-ms:1000",
            "provider turn failed: api_error (HTTP 429): ",
            Some(
                &ProviderTerminalFailure {
                    reason: "api_error".into(),
                    http_status: Some(429),
                }
                .to_provider_status(),
            ),
        )],
        "member-run-1",
        "claude",
        "claude_agent_sdk",
        1_500,
        1_000,
    )
    .expect("a structured 429 is recorded evidence");
    assert_eq!(recorded.state, ProviderCapacityState::Exhausted);

    // No proxy configured: the probe carries a missing-proxy diagnosis.
    let probe = claude_probe_snapshot(None);
    let probe_diagnosis = claude_missing_proxy_diagnosis();
    let probe = ProviderCapacitySnapshot {
        diagnosis: Some(probe_diagnosis.clone()),
        ..probe
    };
    assert!(!claude_has_proxy_configured(&probe.runtime_context));

    let merged = reconcile_recorded_capacity(probe, recorded);

    // Still blocks, and still as exhausted: a missing proxy does not
    // excuse a spent quota the way it excuses a credential rejection.
    assert_eq!(merged.state, ProviderCapacityState::Exhausted);
    let decision = harness_core::provider_capacity_start_decision(Some(&merged), 1_500, 1_000);
    assert!(decision.is_blocked());
    assert!(
        decision.reason().contains("exhausted"),
        "{}",
        decision.reason()
    );

    // But it must not claim proxy causation.
    assert_eq!(
        merged.diagnosis, None,
        "a spent quota must not borrow the probe's missing-proxy diagnosis"
    );
    assert!(
        !format!("{:?}", merged.diagnosis).contains("PROXY"),
        "diagnosis leaked proxy causation: {:?}",
        merged.diagnosis
    );
    // The runtime facts still travel as evidence, just not as cause.
    assert!(
        merged
            .runtime_context
            .iter()
            .any(|fact| fact.key == "HTTPS_PROXY" && !fact.present),
        "the probe's runtime facts must still be visible"
    );
}
