//! Every way a Pi cycle can end in `Err`, and its ADR 0076 mapping.
//!
//! The `Ok` half of the table needs nothing here: `CycleEnding::from_outcome`
//! derives it from the outcome the adapter already returns. This enum names
//! the other half, so an `Err` leaving `run_cycle` can never reach the shared
//! loop as an untyped catch-all.
//!
//! Like Kimi, Pi's cycle spans two types — `PiTeamRuntime` owns `run_cycle`
//! while `PiRpcClient` owns the prompt/event loop — and both record into this
//! ONE enum so neither has to reconstruct a cause from a message string.

use harness_runtime_contract::{CycleEnding, CycleRefusalCode, TerminalUnobservedCode};

/// The closed set of `Err` endings this adapter produces.
///
/// Adding a variant is a compile error until it is placed in the table below,
/// and the mapping match is deliberately wildcard-free for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PiCycleFailure {
    /// The `prompt` RPC was never answered within `input_acceptance`. The RPC
    /// deadline IS the acceptance bound here, so its expiry is replay-safe:
    /// the turn never began.
    InputAcceptanceTimeout,
    /// The RPC transport died, or the reader thread exited.
    TransportLost,
    /// Pi answered the prompt with an error, or omitted the id the cycle must
    /// correlate against (`PI_PROMPT_RECEIPT_UNKNOWN`). This is the ONLY
    /// replay-safe value in the enum, so it is never a default and is only ever
    /// recorded before the input has crossed the provider boundary (review r1
    /// B2). There is no `Default` derive for exactly that reason.
    StartRejected,
    /// An issued abort was never settled within `control_settle`
    /// (`PI_CONTROL_SETTLE_TIMEOUT`).
    ControlSettleTimeout,
    /// `agent_settled` arrived but `get_state` did not confirm
    /// `isStreaming=false` (`PI_CYCLE_SETTLEMENT_UNKNOWN`).
    PostconditionUnknown,
    /// `CycleControl::fatal_error`, or the Harness's own `on_input_accepted`
    /// callback failing. Never a provider ending.
    HostAborted,
}

impl PiCycleFailure {
    /// The ADR 0076 ending for this failure. `detail` is the adapter's own
    /// error text, preserved verbatim so the provider-native evidence is
    /// never replaced by the classification.
    pub(crate) fn ending(self, detail: &str) -> CycleEnding {
        match self {
            Self::InputAcceptanceTimeout => CycleEnding::AcceptanceTimeout,
            Self::TransportLost => CycleEnding::TransportLost {
                detail: detail.to_string(),
            },
            Self::StartRejected => CycleEnding::NotStarted {
                code: CycleRefusalCode::ProviderRejectedStart,
            },
            Self::ControlSettleTimeout => CycleEnding::ControlSettleTimeout,
            Self::PostconditionUnknown => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::PostconditionUnknown,
                detail: detail.to_string(),
            },
            Self::HostAborted => CycleEnding::HostAborted {
                detail: detail.to_string(),
            },
        }
    }

    /// Every failure this adapter can produce, for the exhaustiveness test.
    #[cfg(test)]
    pub(crate) const ALL: &'static [Self] = &[
        Self::InputAcceptanceTimeout,
        Self::TransportLost,
        Self::StartRejected,
        Self::ControlSettleTimeout,
        Self::PostconditionUnknown,
        Self::HostAborted,
    ];
}
