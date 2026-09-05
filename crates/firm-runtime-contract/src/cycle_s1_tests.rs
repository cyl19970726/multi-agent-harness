//! Contract-level tests for SPEC-TYPED-CYCLE-OUTCOME-01 S1: the typed
//! timeouts, interrupt attribution, settlement-derived receipts (C1-C4) and
//! the provider-parameterised A/B assertion family driven by one in-crate
//! scripted fixture.

use harness_core::agentfirm_api::{RuntimeEffectCertainty, RuntimePostconditionStatus};
use harness_core::ProviderBindingAdmission;

use super::*;

fn test_correlation() -> NativeCycleCorrelation {
    NativeCycleCorrelation {
        provider_input_id: "input-1".to_string(),
        input_acceptance_receipt: ControlTransportReceipt {
            command: "cycle.input".to_string(),
            response_id: Some("receipt-1".to_string()),
            success: true,
        },
        terminal_provider_input_id: Some("input-1".to_string()),
        exact_terminal_ref: Some("terminal-1".to_string()),
    }
}

fn settlement(
    terminal: CycleTerminalStatus,
    failure: Option<ProviderTerminalFailure>,
    interrupt: CycleInterruptSettlement,
) -> CycleSettlement {
    CycleSettlement::new(test_correlation(), terminal, failure, interrupt)
}

fn cycle_receipt(settlement: CycleSettlement) -> EffectReceipt {
    EffectReceipt::for_cycle(
        "effect-start-cycle-1",
        ProviderBindingAdmission::Active,
        settlement,
    )
}

#[test]
fn c1_provider_terminal_failure_always_settles_unsatisfied() {
    for terminal in [
        CycleTerminalStatus::Observed,
        CycleTerminalStatus::NotObserved,
    ] {
        for interrupt in [
            CycleInterruptSettlement::None,
            CycleInterruptSettlement::Settled(InterruptCause::HostControl),
            CycleInterruptSettlement::Unsettled,
        ] {
            let receipt = cycle_receipt(settlement(
                terminal,
                Some(ProviderTerminalFailure {
                    reason: "provider_error".to_string(),
                    http_status: Some(500),
                }),
                interrupt.clone(),
            ));
            assert_c1_terminal_failure_unsatisfied(&receipt)
                .unwrap_or_else(|error| panic!("{terminal:?}/{interrupt:?}: {error}"));
        }
    }
}

#[test]
fn c2_clean_terminal_settles_satisfied() {
    let receipt = cycle_receipt(settlement(
        CycleTerminalStatus::Observed,
        None,
        CycleInterruptSettlement::None,
    ));
    assert_c2_clean_terminal_satisfied(&receipt).expect("clean terminal");
    assert_eq!(receipt.certainty, RuntimeEffectCertainty::Applied);
}

#[test]
fn c3_unobserved_terminal_settles_unknown_without_semantics() {
    let receipt = cycle_receipt(settlement(
        CycleTerminalStatus::NotObserved,
        None,
        CycleInterruptSettlement::None,
    ));
    assert_c3_unobserved_terminal_unknown(&receipt).expect("unobserved terminal");
    // The receipt type carries only effect id, certainty, postcondition,
    // admission and evidence strings: no semantic field exists to read
    // (invariant I6).
    assert_eq!(receipt.postcondition, RuntimePostconditionStatus::Unknown);
}

#[test]
fn settled_interrupt_on_clean_terminal_settles_satisfied() {
    // D5's fourth cell (Brain errata): a settled interrupt — Host or
    // AdapterPolicy — with the terminal observed and no failure derives
    // Satisfied; the cause travels on the receipt's evidence.
    for cause in [
        InterruptCause::HostControl,
        InterruptCause::AdapterPolicy {
            reason: "reviewed policy".to_string(),
        },
    ] {
        let receipt = cycle_receipt(settlement(
            CycleTerminalStatus::Observed,
            None,
            CycleInterruptSettlement::Settled(cause.clone()),
        ));
        assert_eq!(
            receipt.postcondition,
            RuntimePostconditionStatus::Satisfied,
            "settled {cause:?} on a clean terminal must derive Satisfied"
        );
        assert_eq!(receipt.certainty, RuntimeEffectCertainty::Applied);
    }
}

#[test]
fn unsettled_interrupt_settles_unknown() {
    let receipt = cycle_receipt(settlement(
        CycleTerminalStatus::Observed,
        None,
        CycleInterruptSettlement::Unsettled,
    ));
    assert_eq!(receipt.postcondition, RuntimePostconditionStatus::Unknown);
}

#[test]
fn c4_no_settlement_input_can_smuggle_a_satisfied_postcondition() {
    // Equal on every D5 axis but different correlation: same derivation.
    let first = settlement(
        CycleTerminalStatus::Observed,
        None,
        CycleInterruptSettlement::None,
    );
    let mut second_correlation = test_correlation();
    second_correlation.exact_terminal_ref = Some("other-terminal".to_string());
    let second = CycleSettlement::new(
        second_correlation,
        CycleTerminalStatus::Observed,
        None,
        CycleInterruptSettlement::None,
    );
    assert_c4_postcondition_derives_only_from_settlement(&first, &second)
        .expect("D5-equivalent settlements derive equally");
    // The exhaustively dishonest target — failure present with everything
    // else "clean" — is unreachable by construction: CycleSettlement fields
    // are private and for_cycle derives postcondition only from the D5 axes.
    let dishonest = cycle_receipt(settlement(
        CycleTerminalStatus::Observed,
        Some(ProviderTerminalFailure {
            reason: "hidden".to_string(),
            http_status: None,
        }),
        CycleInterruptSettlement::None,
    ));
    assert_ne!(
        dishonest.postcondition,
        RuntimePostconditionStatus::Satisfied,
        "a StartCycle-shaped receipt must never be Satisfied with a terminal failure"
    );
}

#[test]
fn adapter_policy_reason_must_be_non_empty() {
    assert!(InterruptCause::adapter_policy("").is_none());
    assert!(InterruptCause::adapter_policy("   ").is_none());
    assert_eq!(
        InterruptCause::adapter_policy("rate limit"),
        Some(InterruptCause::AdapterPolicy {
            reason: "rate limit".to_string()
        })
    );
}

#[test]
fn timeouts_single_flag_shape_uses_contract_defaults() {
    let timeouts = CycleTimeouts::with_input_acceptance(std::time::Duration::from_secs(42));
    assert_eq!(
        timeouts.input_acceptance,
        std::time::Duration::from_secs(42)
    );
    assert_eq!(
        timeouts.transport_liveness,
        CycleTimeouts::DEFAULT_TRANSPORT_LIVENESS
    );
    assert_eq!(
        timeouts.control_settle,
        CycleTimeouts::DEFAULT_CONTROL_SETTLE
    );
    let control = CycleTimeouts::control_path(std::time::Duration::from_secs(7));
    assert_eq!(control.control_settle, std::time::Duration::from_secs(7));
}

fn test_cycle_outcome() -> ExecutionCycleOutcome {
    ExecutionCycleOutcome {
        final_text: "done".to_string(),
        provider_terminal_failure: None,
        interrupt: None,
        close_requested_by_harness: false,
        tool_call_count: 1,
        native_correlation: test_correlation(),
        control_receipts: Vec::new(),
        terminal_observation: CycleRuntimeObservation {
            transport_alive: true,
            process_alive: true,
            is_streaming: Some(false),
            pending_message_count: Some(0),
            steering_mode: None,
            follow_up_mode: None,
            settled_boundary_observed: true,
        },
    }
}

/// The in-crate scripted fixture: reports a conformant shape per script,
/// with switches to make individual scripts non-conformant.
struct ScriptedFixture {
    a1_terminal_failure: bool,
    a1_interrupted: bool,
    a2_disposition: CycleFailureDisposition,
    a3_disposition: CycleFailureDisposition,
    a5_unproven: bool,
    a5_replay_safe: bool,
    b1_cause: InterruptCause,
    b2_cause: InterruptCause,
}

impl Default for ScriptedFixture {
    fn default() -> Self {
        Self {
            a1_terminal_failure: false,
            a1_interrupted: false,
            a2_disposition: CycleFailureDisposition::InputNeverAccepted,
            a3_disposition: CycleFailureDisposition::AcceptedOutcomeUnknown,
            a5_unproven: true,
            a5_replay_safe: false,
            b1_cause: InterruptCause::HostControl,
            b2_cause: InterruptCause::AdapterPolicy {
                reason: "scripted policy".to_string(),
            },
        }
    }
}

impl CycleConformanceFixture for ScriptedFixture {
    type Error = String;

    fn run_receipt_then_silence(
        &mut self,
        _timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error> {
        let mut outcome = test_cycle_outcome();
        if self.a1_terminal_failure {
            outcome.provider_terminal_failure = Some(ProviderTerminalFailure {
                reason: "silence".to_string(),
                http_status: None,
            });
        }
        Ok(CycleConformanceOutcome {
            result: CycleConformanceResult::Outcome(Box::new(outcome)),
            interrupt: self.a1_interrupted.then_some(InterruptCause::HostControl),
            control_unproven: false,
        })
    }

    fn run_no_receipt(
        &mut self,
        _timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error> {
        Ok(CycleConformanceOutcome {
            result: CycleConformanceResult::Failed(self.a2_disposition),
            interrupt: None,
            control_unproven: false,
        })
    }

    fn run_transport_dies_after_receipt(
        &mut self,
        _timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error> {
        Ok(CycleConformanceOutcome {
            result: CycleConformanceResult::Failed(self.a3_disposition),
            interrupt: None,
            control_unproven: false,
        })
    }

    fn run_interrupt_not_acknowledged(
        &mut self,
        _timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error> {
        Ok(CycleConformanceOutcome {
            result: if self.a5_replay_safe {
                CycleConformanceResult::Failed(CycleFailureDisposition::InputNeverAccepted)
            } else {
                CycleConformanceResult::Failed(CycleFailureDisposition::AcceptedOutcomeUnknown)
            },
            interrupt: None,
            control_unproven: self.a5_unproven,
        })
    }

    fn run_host_interrupt(
        &mut self,
        _timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error> {
        Ok(CycleConformanceOutcome {
            result: CycleConformanceResult::Outcome(Box::new(test_cycle_outcome())),
            interrupt: Some(self.b1_cause.clone()),
            control_unproven: false,
        })
    }

    fn run_adapter_policy_interrupt(
        &mut self,
        _timeouts: &CycleTimeouts,
        _reason: &str,
    ) -> Result<CycleConformanceOutcome, Self::Error> {
        Ok(CycleConformanceOutcome {
            result: CycleConformanceResult::Outcome(Box::new(test_cycle_outcome())),
            interrupt: Some(self.b2_cause.clone()),
            control_unproven: false,
        })
    }
}

#[test]
fn conforming_fixture_passes_the_a_and_b_assertion_family() {
    let timeouts = CycleTimeouts::default();
    let mut fixture = ScriptedFixture::default();
    assert_a1_accepted_input_survives_silence(&mut fixture, &timeouts).expect("A1");
    assert_a2_delivery_timeout_fails_closed(&mut fixture, &timeouts).expect("A2");
    assert_a3_transport_death_fails_closed(&mut fixture, &timeouts).expect("A3");
    assert_a5_control_settle_only_bounds_control(&mut fixture, &timeouts).expect("A5");
    assert_b1_host_interrupt_attribution(&mut fixture, &timeouts).expect("B1");
    assert_b2_adapter_policy_interrupt_attribution(&mut fixture, &timeouts, "scripted policy")
        .expect("B2");
}

#[test]
fn nonconforming_fixtures_fail_their_assertions() {
    let timeouts = CycleTimeouts::default();

    let mut a1_failure = ScriptedFixture {
        a1_terminal_failure: true,
        ..ScriptedFixture::default()
    };
    assert!(
        assert_a1_accepted_input_survives_silence(&mut a1_failure, &timeouts).is_err(),
        "silence turned into a terminal failure must fail A1"
    );

    let mut a1_interrupted = ScriptedFixture {
        a1_interrupted: true,
        ..ScriptedFixture::default()
    };
    assert!(
        assert_a1_accepted_input_survives_silence(&mut a1_interrupted, &timeouts).is_err(),
        "an adapter-interrupted silent cycle must fail A1"
    );

    let mut a2_wrong = ScriptedFixture {
        a2_disposition: CycleFailureDisposition::AcceptedOutcomeUnknown,
        ..ScriptedFixture::default()
    };
    assert!(
        assert_a2_delivery_timeout_fails_closed(&mut a2_wrong, &timeouts).is_err(),
        "a never-accepted input mapped to Unknown must fail A2"
    );

    let mut a3_wrong = ScriptedFixture {
        a3_disposition: CycleFailureDisposition::InputNeverAccepted,
        ..ScriptedFixture::default()
    };
    assert!(
        assert_a3_transport_death_fails_closed(&mut a3_wrong, &timeouts).is_err(),
        "transport death mapped to replay-safe must fail A3"
    );

    let mut a5_wrong = ScriptedFixture {
        a5_unproven: false,
        ..ScriptedFixture::default()
    };
    assert!(
        assert_a5_control_settle_only_bounds_control(&mut a5_wrong, &timeouts).is_err(),
        "an unacknowledged Interrupt ending proven must fail A5"
    );

    let mut a5_replay_safe = ScriptedFixture {
        a5_replay_safe: true,
        ..ScriptedFixture::default()
    };
    assert!(
        assert_a5_control_settle_only_bounds_control(&mut a5_replay_safe, &timeouts).is_err(),
        "a replay-safe settle expiry must fail A5"
    );

    let mut b1_wrong = ScriptedFixture {
        b1_cause: InterruptCause::AdapterPolicy {
            reason: "self".to_string(),
        },
        ..ScriptedFixture::default()
    };
    assert!(
        assert_b1_host_interrupt_attribution(&mut b1_wrong, &timeouts).is_err(),
        "a Host interrupt attributed to the adapter must fail B1"
    );

    let mut b2_wrong = ScriptedFixture {
        b2_cause: InterruptCause::HostControl,
        ..ScriptedFixture::default()
    };
    assert!(
        assert_b2_adapter_policy_interrupt_attribution(&mut b2_wrong, &timeouts, "x").is_err(),
        "an adapter-policy interrupt attributed to the Host must fail B2"
    );
}
