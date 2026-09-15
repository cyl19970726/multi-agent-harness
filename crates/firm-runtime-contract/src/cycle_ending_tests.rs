//! ADR 0076: the closed table's own properties.
//!
//! The per-adapter halves live next to each adapter (one wildcard-free
//! `expected` match per provider). These assert the shared half: the `Ok`
//! projection and its precedence, the frozen wire and action-type spellings,
//! and what each ending settles as.

use crate::{
    ControlTransportReceipt, CycleEnding, CycleEndingSettlement, CycleRefusalCode,
    CycleRuntimeObservation, ExecutionCycleOutcome, InterruptCause, NativeCycleCorrelation,
    ProviderFailureCode, ProviderTerminalFailure, TerminalUnobservedCode,
};

fn observation(settled: bool) -> CycleRuntimeObservation {
    CycleRuntimeObservation {
        transport_alive: true,
        process_alive: true,
        is_streaming: Some(false),
        pending_message_count: Some(0),
        steering_mode: None,
        follow_up_mode: None,
        settled_boundary_observed: settled,
    }
}

fn outcome() -> ExecutionCycleOutcome {
    ExecutionCycleOutcome {
        final_text: "authored".to_string(),
        provider_terminal_failure: None,
        interrupt: None,
        close_requested_by_harness: false,
        tool_call_count: 0,
        native_correlation: NativeCycleCorrelation {
            provider_input_id: "provider-input:1".to_string(),
            input_acceptance_receipt: ControlTransportReceipt {
                command: "deliver".to_string(),
                response_id: Some("provider-receipt:1".to_string()),
                success: true,
            },
            terminal_provider_input_id: Some("provider-input:1".to_string()),
            exact_terminal_ref: Some("provider.terminal:1".to_string()),
        },
        control_receipts: Vec::new(),
        terminal_observation: observation(true),
    }
}

/// Every ending in the table, so the shared assertions below cannot silently
/// skip one. This list is checked against the wire vocabulary, which is what
/// a durable row actually carries.
fn all_endings() -> Vec<CycleEnding> {
    vec![
        CycleEnding::Completed,
        CycleEnding::EmptyOutput,
        CycleEnding::InterruptedByHost,
        CycleEnding::InterruptedByProvider {
            reason: "member cancelled in the provider UI".to_string(),
        },
        CycleEnding::Closed,
        CycleEnding::ProviderFailed {
            code: ProviderFailureCode::TurnFailed,
            detail: "turn_failed".to_string(),
            http_status: None,
        },
        CycleEnding::TransportLost {
            detail: "runner stdout disconnected".to_string(),
        },
        CycleEnding::AcceptanceTimeout,
        CycleEnding::ControlSettleTimeout,
        CycleEnding::NotStarted {
            code: CycleRefusalCode::OneDriverViolation,
        },
        CycleEnding::HostAborted {
            detail: "store settle failed".to_string(),
        },
        CycleEnding::TerminalUnobserved {
            code: TerminalUnobservedCode::ProtocolViolation,
            detail: "turn_complete preceded consumed".to_string(),
        },
    ]
}

#[test]
fn a_clean_terminal_with_authored_output_is_completed() {
    assert_eq!(
        CycleEnding::from_outcome(&outcome()),
        CycleEnding::Completed
    );
}

#[test]
fn an_empty_terminal_is_empty_output_and_tool_calls_are_output() {
    let mut empty = outcome();
    empty.final_text = "   ".to_string();
    assert_eq!(CycleEnding::from_outcome(&empty), CycleEnding::EmptyOutput);

    // A turn that called tools but authored no text is NOT empty: it did work.
    empty.tool_call_count = 1;
    assert_eq!(CycleEnding::from_outcome(&empty), CycleEnding::Completed);
}

/// The precedence order is load-bearing because close, interrupt, failure and
/// emptiness can all be true of one outcome:
/// ProviderFailed > Closed > InterruptedByHost > InterruptedByProvider >
/// EmptyOutput > Completed.
#[test]
fn the_ok_precedence_order_holds_when_several_facts_are_true_at_once() {
    let mut everything = outcome();
    everything.final_text = String::new();
    everything.close_requested_by_harness = true;
    everything.interrupt = Some(InterruptCause::HostControl);
    everything.provider_terminal_failure = Some(ProviderTerminalFailure {
        reason: "usage_limit_exceeded".to_string(),
        http_status: None,
    });
    assert_eq!(
        CycleEnding::from_outcome(&everything),
        CycleEnding::ProviderFailed {
            code: ProviderFailureCode::QuotaExhausted,
            detail: "usage_limit_exceeded".to_string(),
            http_status: None,
        },
        "a reported provider failure outranks every other fact"
    );

    everything.provider_terminal_failure = None;
    assert_eq!(
        CycleEnding::from_outcome(&everything),
        CycleEnding::Closed,
        "Close outranks Interrupt: the runtime is going away either way"
    );

    everything.close_requested_by_harness = false;
    assert_eq!(
        CycleEnding::from_outcome(&everything),
        CycleEnding::InterruptedByHost
    );

    everything.interrupt = InterruptCause::provider_initiated("provider ended the turn");
    assert_eq!(
        CycleEnding::from_outcome(&everything),
        CycleEnding::InterruptedByProvider {
            reason: "provider ended the turn".to_string(),
        }
    );

    everything.interrupt = None;
    assert_eq!(
        CycleEnding::from_outcome(&everything),
        CycleEnding::EmptyOutput
    );
}

/// The wire and action-type spellings are frozen: a stored row must stay
/// readable, and the four pre-ADR-0076 action types keep their meaning so
/// historical rows stay comparable.
#[test]
fn wire_and_action_type_spellings_are_frozen_and_distinct() {
    let expected: Vec<(&str, &str)> = vec![
        ("completed", "turn_completed"),
        ("empty_output", "empty_provider_round"),
        ("interrupted_by_host", "interrupted"),
        ("interrupted_by_provider", "interrupted"),
        ("closed", "closed"),
        ("provider_failed:turn_failed", "provider_error"),
        ("transport_lost", "transport_lost"),
        ("acceptance_timeout", "input_never_accepted"),
        ("control_settle_timeout", "control_settle_timeout"),
        ("not_started:one_driver_violation", "cycle_not_started"),
        ("host_aborted", "host_aborted"),
        (
            "terminal_unobserved:protocol_violation",
            "terminal_unobserved",
        ),
    ];
    let observed: Vec<(String, &str)> = all_endings()
        .iter()
        .map(|ending| (ending.wire(), ending.action_type()))
        .collect();
    assert_eq!(observed.len(), expected.len());
    for (index, (wire, action_type)) in expected.into_iter().enumerate() {
        assert_eq!(observed[index].0, wire);
        assert_eq!(observed[index].1, action_type);
    }

    let mut wires: Vec<String> = observed.iter().map(|(wire, _)| wire.clone()).collect();
    wires.sort();
    let unique = wires.len();
    wires.dedup();
    assert_eq!(wires.len(), unique, "every ending needs its own wire value");
}

/// The point of the table: an operator reading one ending knows whether the
/// input crossed the provider boundary and whether the effect is proven.
#[test]
fn each_ending_settles_as_the_table_says() {
    use CycleEndingSettlement::*;
    let accepted: Vec<CycleEndingSettlement> = all_endings()
        .iter()
        .map(|ending| ending.settlement(true))
        .collect();
    assert_eq!(
        accepted,
        vec![
            AppliedSatisfied,        // Completed
            AppliedSatisfied,        // EmptyOutput
            AppliedSatisfied,        // InterruptedByHost
            AppliedSatisfied,        // InterruptedByProvider
            AppliedSatisfied,        // Closed
            AppliedSatisfied, // ProviderFailed (observed terminal, Unsatisfied postcondition)
            RecoveryRequiredUnknown, // TransportLost
            RejectedNotApplied, // AcceptanceTimeout
            RecoveryRequiredUnknown, // ControlSettleTimeout
            RejectedNotApplied, // NotStarted
            RecoveryRequiredUnknown, // HostAborted
            RecoveryRequiredUnknown, // TerminalUnobserved
        ]
    );

    // Invariant I2 in the other direction: the same transport death BEFORE
    // acceptance never applied anything, so it is replay-safe.
    for ending in all_endings() {
        if matches!(ending.settlement(true), RecoveryRequiredUnknown) {
            assert_eq!(
                ending.settlement(false),
                RejectedNotApplied,
                "{ending:?} before acceptance never crossed the boundary"
            );
        }
    }
}

/// `provider_status` must be machine-readable for EVERY ending on every
/// provider — that is what was missing on four of five. A provider failure
/// keeps the historical `provider_terminal:` shape byte for byte so the
/// capacity classifier keeps working; nothing else may masquerade as one.
#[test]
fn every_ending_carries_a_machine_readable_provider_status() {
    for ending in all_endings() {
        let status = ending.provider_status();
        assert!(!status.is_empty(), "{ending:?}");
        match ending.provider_terminal_failure() {
            Some(failure) => {
                assert_eq!(status, failure.to_provider_status());
                assert_eq!(
                    ProviderTerminalFailure::parse(&status).map(|parsed| parsed.reason),
                    Some(failure.reason.clone()),
                    "a provider failure must stay parseable"
                );
            }
            None => {
                assert!(status.starts_with("cycle_ending:"), "{ending:?}: {status}");
                assert!(
                    ProviderTerminalFailure::parse(&status).is_none(),
                    "{ending:?} is not a provider terminal and must not parse as one"
                );
            }
        }
    }
}

/// Only a clean completion may carry a succeeded action status; everything
/// else is a failed row, including an empty terminal.
#[test]
fn only_a_clean_completion_is_a_non_failure() {
    for ending in all_endings() {
        assert_eq!(
            ending.is_failure(),
            ending != CycleEnding::Completed,
            "{ending:?}"
        );
    }
}

/// The closed vocabularies classify what today's producers actually emit;
/// unknown provider text stays unclassified rather than being guessed at, and
/// the text itself is never discarded.
#[test]
fn provider_failure_codes_classify_only_the_closed_vocabularies() {
    let cases: Vec<(&str, Option<i64>, ProviderFailureCode)> = vec![
        (
            "usageLimitExceeded",
            None,
            ProviderFailureCode::QuotaExhausted,
        ),
        ("anything", Some(429), ProviderFailureCode::QuotaExhausted),
        ("anything", Some(403), ProviderFailureCode::Unauthorized),
        ("auth_error", None, ProviderFailureCode::Unauthorized),
        ("max_tokens", None, ProviderFailureCode::OutputLimit),
        ("length", None, ProviderFailureCode::OutputLimit),
        ("refusal", None, ProviderFailureCode::Refusal),
        (
            "max_turn_requests",
            None,
            ProviderFailureCode::TurnRequestLimit,
        ),
        ("error", None, ProviderFailureCode::RunnerError),
        ("turn_failed", None, ProviderFailureCode::TurnFailed),
        (
            "unknown_provider_error",
            None,
            ProviderFailureCode::TurnFailed,
        ),
        (
            "I fixed the 403 handler",
            None,
            ProviderFailureCode::TurnFailed,
        ),
    ];
    for (reason, http_status, code) in cases {
        let failure = ProviderTerminalFailure {
            reason: reason.to_string(),
            http_status,
        };
        assert_eq!(ProviderFailureCode::classify(&failure), code, "{reason}");
        let ending = CycleEnding::ProviderFailed {
            code,
            detail: failure.reason.clone(),
            http_status,
        };
        assert_eq!(
            ending.provider_terminal_failure(),
            Some(failure),
            "the provider's own text must survive classification"
        );
    }
}
