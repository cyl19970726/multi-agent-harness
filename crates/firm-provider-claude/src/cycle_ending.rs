//! Every way a Claude cycle can end in `Err`, and its ADR 0076 mapping.
//!
//! The `Ok` half of the table needs nothing here: `CycleEnding::from_outcome`
//! derives it from the outcome the adapter already returns. This enum names
//! the other half, so an `Err` leaving `run_cycle` can never reach the shared
//! loop as an untyped catch-all.

use super::*;

/// The closed set of `Err` endings this adapter produces.
///
/// Adding a variant is a compile error until it is placed in the table below,
/// and the mapping match is deliberately wildcard-free for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeCycleFailure {
    /// `CycleControl::fatal_error`: the Harness itself ended the cycle.
    HostFatalControl,
    /// The Harness's own `on_input_accepted` callback failed (a durable
    /// settlement write), not the provider.
    AcceptanceCallbackFailed,
    /// Runner stdout disconnected, the child exited, or the transport was
    /// already closed.
    TransportClosed,
    /// The input was written but never consumed within `input_acceptance`.
    InputAcceptanceTimeout,
    /// An issued interrupt was never acknowledged within `control_settle`.
    ControlSettleTimeout,
    /// The runner reported an error frame for this turn.
    RunnerError,
    /// `member_closed` arrived without a Harness CloseRuntime.
    UnexpectedClose,
    /// A terminal or interrupt frame preceded its acceptance receipt, or a
    /// runner frame violated the shared protocol.
    ProtocolViolation,
    /// A resumed or bound session id did not match the retained one.
    TerminalMismatch,
}

impl ClaudeCycleFailure {
    /// The ADR 0076 ending for this failure. `detail` is the adapter's own
    /// error text, preserved verbatim so the provider-native evidence is
    /// never replaced by the classification.
    pub(crate) fn ending(self, detail: &str) -> CycleEnding {
        match self {
            Self::HostFatalControl | Self::AcceptanceCallbackFailed => CycleEnding::HostAborted {
                detail: detail.to_string(),
            },
            Self::TransportClosed => CycleEnding::TransportLost {
                detail: detail.to_string(),
            },
            Self::InputAcceptanceTimeout => CycleEnding::AcceptanceTimeout,
            Self::ControlSettleTimeout => CycleEnding::ControlSettleTimeout,
            Self::RunnerError => CycleEnding::ProviderFailed {
                code: ProviderFailureCode::RunnerError,
                detail: detail.to_string(),
                http_status: None,
            },
            Self::UnexpectedClose => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::UnexpectedClose,
                detail: detail.to_string(),
            },
            Self::ProtocolViolation => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::ProtocolViolation,
                detail: detail.to_string(),
            },
            Self::TerminalMismatch => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::TerminalMismatch,
                detail: detail.to_string(),
            },
        }
    }

    /// Every failure this adapter can produce, for the exhaustiveness test.
    #[cfg(test)]
    pub(crate) const ALL: &'static [Self] = &[
        Self::HostFatalControl,
        Self::AcceptanceCallbackFailed,
        Self::TransportClosed,
        Self::InputAcceptanceTimeout,
        Self::ControlSettleTimeout,
        Self::RunnerError,
        Self::UnexpectedClose,
        Self::ProtocolViolation,
        Self::TerminalMismatch,
    ];
}
