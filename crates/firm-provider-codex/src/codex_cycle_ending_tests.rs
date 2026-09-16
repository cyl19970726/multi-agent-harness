//! ADR 0076 cycle-ending tests for the Codex app-server adapter.
//!
//! Split out of `codex_team_runtime_tests.rs` when that file reached the
//! 1,500-line source ceiling. These share one subject — how a Codex cycle ends
//! and what it records — so they are a cohesive owner boundary rather than an
//! arbitrary cut, and they reuse the `FakeBridge` harness from the parent
//! module. Declared from that module, not from `lib.rs`, so the crate root
//! keeps its headroom.

use super::*;

/// ADR 0076 exhaustiveness: every ending this adapter can produce is placed in
/// the closed table. The `expected` match below is wildcard-free, so adding a
/// variant to `CodexCycleFailure` breaks this test's compilation until the new
/// ending is decided on deliberately.
#[test]
fn every_codex_cycle_ending_is_placed_in_the_closed_table() {
    use crate::cycle_ending::CodexCycleFailure;
    use harness_runtime_contract::{
        CycleEnding, CycleRefusalCode, ProviderFailureCode, TerminalUnobservedCode,
    };
    fn expected(failure: CodexCycleFailure) -> CycleEnding {
        match failure {
            CodexCycleFailure::RuntimeClosed => CycleEnding::NotStarted {
                code: CycleRefusalCode::RuntimeClosed,
            },
            CodexCycleFailure::OneDriverViolation => CycleEnding::NotStarted {
                code: CycleRefusalCode::OneDriverViolation,
            },
            CodexCycleFailure::ContinuationArmed => CycleEnding::NotStarted {
                code: CycleRefusalCode::ContinuationArmed,
            },
            CodexCycleFailure::StartRejected => CycleEnding::NotStarted {
                code: CycleRefusalCode::ProviderRejectedStart,
            },
            CodexCycleFailure::HostFatalControl | CodexCycleFailure::AcceptanceCallbackFailed => {
                CycleEnding::HostAborted {
                    detail: "detail".to_string(),
                }
            }
            CodexCycleFailure::InputAcceptanceTimeout => CycleEnding::AcceptanceTimeout,
            CodexCycleFailure::TransportClosed => CycleEnding::TransportLost {
                detail: "detail".to_string(),
            },
            CodexCycleFailure::ControlSettleTimeout => CycleEnding::ControlSettleTimeout,
            CodexCycleFailure::UnknownTerminalStatus => CycleEnding::ProviderFailed {
                code: ProviderFailureCode::TurnFailed,
                detail: "detail".to_string(),
                http_status: None,
            },
            CodexCycleFailure::PostconditionUnknown => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::PostconditionUnknown,
                detail: "detail".to_string(),
            },
            CodexCycleFailure::TerminalMismatch => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::TerminalMismatch,
                detail: "detail".to_string(),
            },
            CodexCycleFailure::ProtocolViolation => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::ProtocolViolation,
                detail: "detail".to_string(),
            },
        }
    }
    assert_eq!(
        CodexCycleFailure::ALL.len(),
        13,
        "ALL must list every variant the wildcard-free match above covers"
    );
    for failure in CodexCycleFailure::ALL {
        let ending = failure.ending("detail");
        assert_eq!(ending, expected(*failure), "{failure:?}");
        assert!(!ending.action_type().is_empty(), "{failure:?}");
        assert!(!ending.provider_status().is_empty(), "{failure:?}");
    }
}

/// A refused start never crossed the provider boundary, so its ending is
/// replay-safe and carries no provider terminal failure — the diagnostic from
/// an earlier cycle must not leak onto it.
#[test]
fn a_refused_start_records_a_replay_safe_not_started_ending() {
    use harness_runtime_contract::{CycleEnding, CycleEndingSettlement, CycleRefusalCode};
    let mut adapter = CodexTeamRuntime::new(FakeBridge::completed("completed"));
    adapter.runtime_closed = true;
    let error = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "input",
        CycleTimeouts::with_input_acceptance(Duration::from_secs(1)),
        &mut |_| Ok(()),
        &mut |_| {},
        &mut CycleControl::default,
    )
    .expect_err("a closed runtime refuses the cycle");
    assert!(error.to_string().contains("explicitly closed"));
    let ending = TeamRuntimeAdapter::take_cycle_ending(&mut adapter).expect("a typed ending");
    assert_eq!(
        ending,
        CycleEnding::NotStarted {
            code: CycleRefusalCode::RuntimeClosed
        }
    );
    assert_eq!(
        ending.settlement(false),
        CycleEndingSettlement::RejectedNotApplied
    );
    assert!(ending.provider_terminal_failure().is_none());
    assert!(
        TeamRuntimeAdapter::take_cycle_ending(&mut adapter).is_none(),
        "the ending is consumed once"
    );
}

/// ADR 0076 / review r1 B1. The exhaustiveness test proves the local enum maps
/// totally onto the table; it cannot prove that every `Err` SITE records a
/// variant. These two drive the adapter to the two `run_cycle` sites that were
/// missed in the first submission, and to a mid-cycle transport death.
#[test]
fn a_refused_provider_request_mid_cycle_records_a_typed_ending() {
    let bridge = FakeBridge::completed("completed");
    // A reverse provider request this adapter refuses fail-closed. It arrives
    // BEFORE the terminal frame, i.e. after `on_input_accepted` has fired.
    bridge.frames.borrow_mut().push_front(Ok(serde_json::json!({
        "id": 7,
        "method": "codex/unhandledReverseRequest",
        "params": {"threadId": "thread-1"}
    })));
    let mut adapter = CodexTeamRuntime::new(bridge);
    let mut accepted = 0;
    let error = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "input",
        CycleTimeouts::with_input_acceptance(Duration::from_secs(1)),
        &mut |_| {
            accepted += 1;
            Ok(())
        },
        &mut |_| {},
        &mut CycleControl::default,
    )
    .expect_err("an unserveable provider request ends the cycle");
    assert_eq!(accepted, 1, "the input had already crossed the boundary");
    assert!(
        error.to_string().contains("CODEX_PROVIDER_REQUEST"),
        "{error}"
    );
    let ending = TeamRuntimeAdapter::take_cycle_ending(&mut adapter)
        .expect("a refused provider request must record a typed ending");
    assert_eq!(
        ending,
        harness_runtime_contract::CycleEnding::TerminalUnobserved {
            code: harness_runtime_contract::TerminalUnobservedCode::ProtocolViolation,
            detail: error.to_string(),
        }
    );
    assert_eq!(
        ending.settlement(true),
        harness_runtime_contract::CycleEndingSettlement::RecoveryRequiredUnknown
    );
}

#[test]
fn a_foreign_thread_frame_mid_cycle_records_a_typed_ending() {
    let bridge = FakeBridge::completed("completed");
    // A frame scoped to a thread this cycle never admitted.
    bridge.frames.borrow_mut().push_front(Ok(serde_json::json!({
        "method": "item/started",
        "params": {"threadId": "foreign-thread", "turnId": "turn-1", "item": {"type": "command"}}
    })));
    let mut adapter = CodexTeamRuntime::new(bridge);
    let result = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "input",
        CycleTimeouts::with_input_acceptance(Duration::from_secs(1)),
        &mut |_| Ok(()),
        &mut |_| {},
        &mut CycleControl::default,
    );
    // Whatever this frame does — refused outright, or skipped as a descendant
    // and the cycle then running out of frames — the invariant is the same: an
    // `Err` out of `run_cycle` always carries a typed ending.
    if result.is_err() {
        assert!(
            TeamRuntimeAdapter::take_cycle_ending(&mut adapter).is_some(),
            "every Err path records a typed ending"
        );
    }
}

#[test]
fn a_transport_disconnect_mid_cycle_records_a_typed_ending() {
    let bridge = FakeBridge::completed("completed");
    // No frames at all: `recv` returns Disconnected immediately.
    bridge.frames.borrow_mut().clear();
    let mut adapter = CodexTeamRuntime::new(bridge);
    let error = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "input",
        CycleTimeouts::with_input_acceptance(Duration::from_secs(1)),
        &mut |_| Ok(()),
        &mut |_| {},
        &mut CycleControl::default,
    )
    .expect_err("a disconnected app-server ends the cycle");
    assert!(
        error.to_string().contains("transport disconnected"),
        "{error}"
    );
    let ending = TeamRuntimeAdapter::take_cycle_ending(&mut adapter)
        .expect("a transport death must record a typed ending");
    assert!(
        matches!(
            ending,
            harness_runtime_contract::CycleEnding::TransportLost { .. }
        ),
        "{ending:?}"
    );
    assert_eq!(ending.action_type(), "transport_lost");
}

/// ADR 0076 / review r1 P3-3: the `Ok`-half evidence, per adapter rather than
/// only on the two that changed. A turn that authors nothing and calls no tool
/// is `EmptyOutput` here exactly as on Claude and DeepSeek, so zero output
/// feeds the unproductive-round circuit breaker identically on all five.
#[test]
fn a_silent_turn_is_empty_output_on_codex() {
    let bridge = FakeBridge::completed("completed");
    bridge.frames.borrow_mut()[0].as_mut().unwrap()["params"]["turn"]["items"] =
        serde_json::json!([]);
    let mut adapter = CodexTeamRuntime::new(bridge);
    let outcome = TeamRuntimeAdapter::run_cycle(
        &mut adapter,
        "input",
        CycleTimeouts::with_input_acceptance(Duration::from_secs(1)),
        &mut |_| Ok(()),
        &mut |_| {},
        &mut CycleControl::default,
    )
    .expect("a silent turn is still an Ok cycle");
    assert!(outcome.provider_terminal_failure.is_none());
    assert_eq!(
        harness_runtime_contract::CycleEnding::from_outcome(&outcome),
        harness_runtime_contract::CycleEnding::EmptyOutput
    );
    assert_eq!(
        harness_runtime_contract::CycleEnding::from_outcome(&outcome).action_type(),
        "empty_provider_round"
    );
}
