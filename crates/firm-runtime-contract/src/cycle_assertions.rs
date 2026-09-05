//! Provider-parameterised cycle conformance assertions
//! (SPEC-TYPED-CYCLE-OUTCOME-01 §5, groups A/B/C).
//!
//! S2 runs each assertion against every adapter; each adapter's conformance
//! harness implements [`CycleConformanceFixture`] to script the transport
//! ("receipt delivered then silence", "no receipt", "transport died after
//! receipt", "interrupt not acknowledged", terminal/failure outcomes) and
//! reports the cycle's shape in runtime-contract terms. The application-layer
//! outcome enums stay in `firm-application` (invariant I6): the strongest
//! value this layer expresses is "unproven".

use harness_core::agentfirm_api::{RuntimeEffectCertainty, RuntimePostconditionStatus};
use thiserror::Error;

use crate::{CycleSettlement, CycleTimeouts, EffectReceipt, ExecutionCycleOutcome, InterruptCause};

/// One failed conformance assertion, named by its §5 identifier.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("cycle conformance assertion {assertion} failed: {detail}")]
pub struct CycleConformanceError {
    pub assertion: &'static str,
    pub detail: String,
}

fn fail<T>(assertion: &'static str, detail: impl Into<String>) -> Result<T, CycleConformanceError> {
    Err(CycleConformanceError {
        assertion,
        detail: detail.into(),
    })
}

/// What a cycle-level failure means for effect replay, reported by the
/// adapter's conformance harness in runtime-contract terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleFailureDisposition {
    /// The provider never accepted the input; replay is safe (the
    /// application layer's NotApplied).
    InputNeverAccepted,
    /// The input was accepted; the cycle outcome is unproven (the
    /// application layer's Unknown — replay would duplicate provider input).
    AcceptedOutcomeUnknown,
}

/// The cycle's terminal shape as the adapter reports it.
#[derive(Debug)]
pub enum CycleConformanceResult {
    /// The cycle ended with an outcome (which may still carry a provider
    /// terminal failure or a harness close).
    Outcome(Box<ExecutionCycleOutcome>),
    /// The cycle failed before an outcome; the fixture reports the replay
    /// disposition its adapter assigns.
    Failed(CycleFailureDisposition),
}

/// Everything one scripted cycle run must report for the A/B assertions.
#[derive(Debug)]
pub struct CycleConformanceOutcome {
    pub result: CycleConformanceResult,
    /// The attributed interrupt cause when one settled during the cycle
    /// (B1/B2). The fixture reports the typed cause here even though
    /// `ExecutionCycleOutcome.interrupt` is an S2 field.
    pub interrupt: Option<InterruptCause>,
    /// A5: an issued control (Interrupt) whose settle expired must end
    /// unproven, never successful and never as a failure of the cycle itself.
    pub control_unproven: bool,
}

/// Scripted transport an adapter's conformance test implements to drive the
/// shared A/B assertions (S2: once per adapter).
pub trait CycleConformanceFixture {
    type Error: std::fmt::Debug;

    /// A1: the exact input acceptance receipt is delivered, then the
    /// transport stays silent longer than `input_acceptance` and
    /// `transport_liveness` combined, with continuous liveness proof.
    fn run_receipt_then_silence(
        &mut self,
        timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error>;

    /// A2: the transport never delivers an acceptance receipt.
    fn run_no_receipt(
        &mut self,
        timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error>;

    /// A3: the receipt is delivered, then the transport dies / the child
    /// process exits.
    fn run_transport_dies_after_receipt(
        &mut self,
        timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error>;

    /// A5: a Host Interrupt is issued and the provider never acknowledges it.
    fn run_interrupt_not_acknowledged(
        &mut self,
        timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error>;

    /// B1: a Host Interrupt is injected through `poll_control` and settles.
    fn run_host_interrupt(
        &mut self,
        timeouts: &CycleTimeouts,
    ) -> Result<CycleConformanceOutcome, Self::Error>;

    /// B2: an adapter-internal policy interrupts with `reason` and settles.
    fn run_adapter_policy_interrupt(
        &mut self,
        timeouts: &CycleTimeouts,
        reason: &str,
    ) -> Result<CycleConformanceOutcome, Self::Error>;
}

/// A1 — accepted input never fails by time alone (invariant I1).
pub fn assert_a1_accepted_input_survives_silence<F: CycleConformanceFixture>(
    fixture: &mut F,
    timeouts: &CycleTimeouts,
) -> Result<(), CycleConformanceError> {
    const A: &str = "A1";
    let outcome = fixture
        .run_receipt_then_silence(timeouts)
        .map_err(|error| CycleConformanceError {
            assertion: A,
            detail: format!("fixture error: {error:?}"),
        })?;
    match outcome.result {
        CycleConformanceResult::Outcome(ref cycle) => {
            if cycle.provider_terminal_failure.is_some() {
                return fail(
                    A,
                    "accepted input turned into a provider terminal failure by silence alone",
                );
            }
            if outcome.interrupt.is_some() {
                return fail(A, "a silent accepted cycle was interrupted by the adapter");
            }
            Ok(())
        }
        CycleConformanceResult::Failed(disposition) => fail(
            A,
            format!("accepted input became a cycle failure by time alone: {disposition:?}"),
        ),
    }
}

/// A2 — a delivery timeout before the receipt stays replay-safe.
pub fn assert_a2_delivery_timeout_fails_closed<F: CycleConformanceFixture>(
    fixture: &mut F,
    timeouts: &CycleTimeouts,
) -> Result<(), CycleConformanceError> {
    const A: &str = "A2";
    let outcome = fixture
        .run_no_receipt(timeouts)
        .map_err(|error| CycleConformanceError {
            assertion: A,
            detail: format!("fixture error: {error:?}"),
        })?;
    match outcome.result {
        CycleConformanceResult::Failed(CycleFailureDisposition::InputNeverAccepted) => Ok(()),
        CycleConformanceResult::Failed(disposition) => fail(
            A,
            format!("a never-accepted input must stay replay-safe, got {disposition:?}"),
        ),
        CycleConformanceResult::Outcome(_) => fail(
            A,
            "a cycle without any acceptance receipt produced an outcome",
        ),
    }
}

/// A3 — transport death after acceptance stays fail-closed (invariant I2).
pub fn assert_a3_transport_death_fails_closed<F: CycleConformanceFixture>(
    fixture: &mut F,
    timeouts: &CycleTimeouts,
) -> Result<(), CycleConformanceError> {
    const A: &str = "A3";
    let outcome = fixture
        .run_transport_dies_after_receipt(timeouts)
        .map_err(|error| CycleConformanceError {
            assertion: A,
            detail: format!("fixture error: {error:?}"),
        })?;
    match outcome.result {
        CycleConformanceResult::Failed(CycleFailureDisposition::AcceptedOutcomeUnknown) => Ok(()),
        CycleConformanceResult::Failed(CycleFailureDisposition::InputNeverAccepted) => fail(
            A,
            "transport death after acceptance mapped to replay-safe, which would duplicate provider input",
        ),
        CycleConformanceResult::Outcome(_) => {
            fail(A, "a dead transport produced a cycle outcome")
        }
    }
}

/// A5 — control_settle bounds control only; an unacknowledged Interrupt ends
/// unproven (D3), never successful, never replay-safe (the input WAS
/// accepted), and never as a cycle failure.
pub fn assert_a5_control_settle_only_bounds_control<F: CycleConformanceFixture>(
    fixture: &mut F,
    timeouts: &CycleTimeouts,
) -> Result<(), CycleConformanceError> {
    const A: &str = "A5";
    let outcome = fixture
        .run_interrupt_not_acknowledged(timeouts)
        .map_err(|error| CycleConformanceError {
            assertion: A,
            detail: format!("fixture error: {error:?}"),
        })?;
    if !outcome.control_unproven {
        return fail(
            A,
            "an unacknowledged Interrupt did not end unproven after control_settle",
        );
    }
    match outcome.result {
        // The honest fail-closed shapes: a control-level Unknown. The input
        // was accepted, so a replay-safe disposition is always wrong here.
        CycleConformanceResult::Failed(CycleFailureDisposition::AcceptedOutcomeUnknown)
        | CycleConformanceResult::Outcome(_) => Ok(()),
        CycleConformanceResult::Failed(CycleFailureDisposition::InputNeverAccepted) => fail(
            A,
            "control_settle expiry was attributed to an unaccepted input (replay-safe)",
        ),
    }
}

/// B1 — a Host control interrupt is attributed to the Host.
pub fn assert_b1_host_interrupt_attribution<F: CycleConformanceFixture>(
    fixture: &mut F,
    timeouts: &CycleTimeouts,
) -> Result<(), CycleConformanceError> {
    const A: &str = "B1";
    let outcome = fixture
        .run_host_interrupt(timeouts)
        .map_err(|error| CycleConformanceError {
            assertion: A,
            detail: format!("fixture error: {error:?}"),
        })?;
    match outcome.interrupt {
        Some(InterruptCause::HostControl) => Ok(()),
        other => fail(
            A,
            format!("a Host control interrupt was attributed to {other:?}"),
        ),
    }
}

/// B2 — an adapter-policy interrupt is attributed with a non-empty reason.
pub fn assert_b2_adapter_policy_interrupt_attribution<F: CycleConformanceFixture>(
    fixture: &mut F,
    timeouts: &CycleTimeouts,
    reason: &str,
) -> Result<(), CycleConformanceError> {
    const A: &str = "B2";
    let outcome = fixture
        .run_adapter_policy_interrupt(timeouts, reason)
        .map_err(|error| CycleConformanceError {
            assertion: A,
            detail: format!("fixture error: {error:?}"),
        })?;
    match outcome.interrupt {
        Some(InterruptCause::AdapterPolicy { reason }) if !reason.trim().is_empty() => Ok(()),
        other => fail(
            A,
            format!("an adapter-policy interrupt was attributed to {other:?}"),
        ),
    }
}

/// C1 — a provider terminal failure never settles as Satisfied
/// (invariant I5).
pub fn assert_c1_terminal_failure_unsatisfied(
    receipt: &EffectReceipt,
) -> Result<(), CycleConformanceError> {
    if receipt.postcondition == RuntimePostconditionStatus::Unsatisfied {
        Ok(())
    } else {
        fail(
            "C1",
            format!(
                "a cycle with provider_terminal_failure settled as {:?}",
                receipt.postcondition
            ),
        )
    }
}

/// C2 — terminal observed, no failure, no interrupt settles Satisfied.
pub fn assert_c2_clean_terminal_satisfied(
    receipt: &EffectReceipt,
) -> Result<(), CycleConformanceError> {
    if receipt.postcondition == RuntimePostconditionStatus::Satisfied {
        Ok(())
    } else {
        fail(
            "C2",
            format!(
                "a clean terminal cycle settled as {:?}",
                receipt.postcondition
            ),
        )
    }
}

/// C3 — terminal not observed settles Unknown, and the receipt carries no
/// semantic field (the type has none to carry: effect id, certainty,
/// postcondition, admission and evidence strings only).
pub fn assert_c3_unobserved_terminal_unknown(
    receipt: &EffectReceipt,
) -> Result<(), CycleConformanceError> {
    if receipt.postcondition != RuntimePostconditionStatus::Unknown {
        return fail(
            "C3",
            format!(
                "an unobserved terminal settled as {:?}",
                receipt.postcondition
            ),
        );
    }
    if receipt.certainty != RuntimeEffectCertainty::Unknown {
        return fail(
            "C3",
            format!(
                "an unobserved terminal claims certainty {:?}",
                receipt.certainty
            ),
        );
    }
    Ok(())
}

/// C4 companion: the D5 derivation reads only the D5 axes. Two settlements
/// that differ only in facts the D5 table ignores must derive the same
/// postcondition. The non-bypassability half of C4 — that `CycleSettlement`
/// cannot be struct-literal-constructed outside this crate — is pinned by
/// the `compile_fail` doc-test on `CycleSettlement`; this assertion proves
/// the derivation itself cannot be steered through non-D5 fields.
pub fn assert_c4_postcondition_derives_only_from_settlement(
    first: &CycleSettlement,
    second: &CycleSettlement,
) -> Result<(), CycleConformanceError> {
    let derive = |settlement: &CycleSettlement| {
        EffectReceipt::for_cycle(
            "conformance-c4",
            harness_core::ProviderBindingAdmission::Active,
            settlement.clone(),
        )
        .postcondition
    };
    if derive(first) == derive(second) {
        Ok(())
    } else {
        fail(
            "C4",
            format!(
                "settlements equal on every D5 axis derived different postconditions: {:?} vs {:?}",
                derive(first),
                derive(second)
            ),
        )
    }
}
