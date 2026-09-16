//! ADR 0076 cycle-ending tests for the Kimi ACP adapter.
//!
//! Split out of `kimi_acp_tests.rs` when that file reached the 1,500-line
//! source ceiling: these share one subject — how a Kimi cycle ends and what it
//! records — so they are a cohesive owner boundary rather than an arbitrary cut.
//! They reuse the scripted-client harness from the parent module.

use super::*;

/// ADR 0076 exhaustiveness: every ending this adapter can produce is placed in
/// the closed table. The `expected` match below is wildcard-free, so adding a
/// variant to `KimiCycleFailure` breaks this test's compilation until the new
/// ending is decided on deliberately.
#[test]
fn every_kimi_cycle_ending_is_placed_in_the_closed_table() {
    use crate::KimiCycleFailure;
    use harness_runtime_contract::{
        CycleEnding, CycleRefusalCode, ProviderFailureCode, TerminalUnobservedCode,
    };
    fn expected(failure: KimiCycleFailure) -> CycleEnding {
        match failure {
            KimiCycleFailure::SessionNotEstablished => CycleEnding::NotStarted {
                code: CycleRefusalCode::ProviderRejectedStart,
            },
            KimiCycleFailure::PromptAlreadyActive => CycleEnding::NotStarted {
                code: CycleRefusalCode::OneDriverViolation,
            },
            KimiCycleFailure::TransportLost => CycleEnding::TransportLost {
                detail: "detail".to_string(),
            },
            KimiCycleFailure::CancelGraceExpired => CycleEnding::ControlSettleTimeout,
            KimiCycleFailure::TerminalMismatch => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::TerminalMismatch,
                detail: "detail".to_string(),
            },
            KimiCycleFailure::ProtocolViolation => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::ProtocolViolation,
                detail: "detail".to_string(),
            },
            KimiCycleFailure::HostAborted => CycleEnding::HostAborted {
                detail: "detail".to_string(),
            },
            KimiCycleFailure::ProviderError => CycleEnding::ProviderFailed {
                code: ProviderFailureCode::TurnFailed,
                detail: "detail".to_string(),
                http_status: None,
            },
        }
    }
    assert_eq!(
        KimiCycleFailure::ALL.len(),
        8,
        "ALL must list every variant the wildcard-free match above covers"
    );
    for failure in KimiCycleFailure::ALL {
        let ending = failure.ending("detail");
        assert_eq!(ending, expected(*failure), "{failure:?}");
        assert!(!ending.action_type().is_empty(), "{failure:?}");
        assert!(!ending.provider_status().is_empty(), "{failure:?}");
    }
}

/// The reviewed Kimi stop-reason vocabulary reaches the closed failure codes
/// rather than staying raw provider text.
#[test]
fn kimi_stop_reasons_classify_into_the_closed_failure_codes() {
    use harness_runtime_contract::{ProviderFailureCode, ProviderTerminalFailure};
    for (stop_reason, code) in [
        ("max_tokens", ProviderFailureCode::OutputLimit),
        ("refusal", ProviderFailureCode::Refusal),
        ("max_turn_requests", ProviderFailureCode::TurnRequestLimit),
    ] {
        assert_eq!(
            ProviderFailureCode::classify(&ProviderTerminalFailure {
                reason: stop_reason.to_string(),
                http_status: None,
            }),
            code,
            "{stop_reason}"
        );
    }
}

/// ADR 0076 misalignment 1, on the fifth provider (review r1 IP-1). A Host
/// Interrupt or Close that races a NORMALLY completed Kimi turn must still
/// reach the loop as a settled control. Gating the abort receipt on the stop
/// reason made `abort_receipt_observed` false whenever the turn finished on
/// its own first, and the shared loop then failed the whole member with
/// "kimi control lacked verified terminal acknowledgement".
#[cfg(unix)]
#[test]
fn requested_interrupt_survives_a_turn_that_completes_normally() {
    let outcome = drive_kimi_cycle(
        &kimi_control_timeouts(),
        true,
        // The provider ends the turn normally, NOT as cancelled.
        Some(terminal_frame(2, "end_turn")),
        false,
        || harness_runtime_contract::CycleControl {
            close: false,
            interrupt: true,
            fatal_error: None,
        },
    )
    .expect("a normally completed turn is still an Ok cycle");

    assert_eq!(
        outcome.interrupt,
        Some(harness_runtime_contract::InterruptCause::HostControl),
        "the requested interrupt must survive the normal completion"
    );
    let abort = outcome
        .control_receipts
        .iter()
        .find(|receipt| receipt.command == "abort")
        .expect("the delivered cancel needs its receipt");
    assert!(
        abort.success,
        "the cancel crossed the provider boundary, so its receipt succeeds on delivery: {abort:?}"
    );
    assert!(
        abort
            .response_id
            .as_deref()
            .unwrap_or_default()
            .contains("stopReason=end_turn"),
        "the eventual stop reason stays on the receipt as native evidence: {abort:?}"
    );
    // The shared loop's acceptance rule: interrupt attributed + abort receipt
    // successful + terminal observed.
    assert!(
        outcome.interrupt.is_some()
            && abort.success
            && outcome.terminal_observation.terminal_cycle_observed(),
        "the loop must be able to verify this terminal control ack"
    );
    assert_eq!(
        harness_runtime_contract::CycleEnding::from_outcome(&outcome),
        harness_runtime_contract::CycleEnding::InterruptedByHost
    );
}

/// ADR 0076 / review r1 B1: every `Err` out of `run_cycle` records a typed
/// ending. Driven to a real mid-cycle session death.
#[cfg(unix)]
#[test]
fn a_session_death_mid_cycle_records_a_typed_ending() {
    let error = drive_kimi_cycle(
        &kimi_conformance_timeouts(),
        true,
        None,
        true,
        harness_runtime_contract::CycleControl::default,
    )
    .expect_err("a dead ACP session ends the cycle");
    assert!(error.contains("kimi acp prompt"), "{error}");
}

/// ADR 0076 / review r1 P3-3: the `Ok`-half evidence on Kimi.
#[cfg(unix)]
#[test]
fn a_silent_turn_is_empty_output_on_kimi() {
    let outcome = drive_kimi_cycle(
        &kimi_conformance_timeouts(),
        true,
        Some(terminal_frame(2, "end_turn")),
        false,
        harness_runtime_contract::CycleControl::default,
    )
    .expect("a silent turn is still an Ok cycle");
    assert!(outcome.provider_terminal_failure.is_none());
    assert!(outcome.final_text.trim().is_empty());
    assert_eq!(
        harness_runtime_contract::CycleEnding::from_outcome(&outcome),
        harness_runtime_contract::CycleEnding::EmptyOutput
    );
}

/// X1b item C. Kimi is the one adapter whose acceptance is INFERRED: ACP has no
/// prompt-start acknowledgement, the request id is ours and nothing echoes it,
/// so the evidence is the first prompt-scoped `session/update`.
#[cfg(unix)]
#[test]
fn kimi_states_an_inferred_acceptance_id() {
    let outcome = drive_kimi_cycle(
        &kimi_conformance_timeouts(),
        true,
        Some(terminal_frame(2, "end_turn")),
        false,
        harness_runtime_contract::CycleControl::default,
    )
    .expect("a clean cycle");
    assert_eq!(
        outcome.native_correlation.acceptance_id_provenance,
        harness_runtime_contract::AcceptanceIdProvenance::Inferred
    );
}
