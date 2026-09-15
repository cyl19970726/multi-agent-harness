//! Every way a Kimi cycle can end in `Err`, and its ADR 0076 mapping.
//!
//! The `Ok` half of the table needs nothing here: `CycleEnding::from_outcome`
//! derives it from the outcome the adapter already returns. This enum names
//! the other half, so an `Err` leaving `run_cycle` can never reach the shared
//! loop as an untyped catch-all.
//!
//! Kimi is the one adapter whose cycle spans two types: `KimiTeamRuntime`
//! owns `run_cycle` while `KimiAcpClient` owns `prompt`/`drive_prompt`. Both
//! record into this ONE enum, so the ACP client never has to invent a second
//! vocabulary and the adapter never has to reconstruct a cause from a message
//! string.

use harness_runtime_contract::{
    CycleEnding, CycleRefusalCode, ProviderFailureCode, ProviderTerminalFailure,
    TerminalUnobservedCode,
};

/// The closed set of `Err` endings this adapter produces.
///
/// Adding a variant is a compile error until it is placed in the table below,
/// and the mapping match is deliberately wildcard-free for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum KimiCycleFailure {
    /// No ACP session exists yet, so no prompt can be opened.
    SessionNotEstablished,
    /// A `session/prompt` is already in flight on this session.
    PromptAlreadyActive,
    /// The ACP transport died, or its multiplexed reader disconnected.
    TransportLost,
    /// `session/cancel` was ignored for the whole grace window and the owned
    /// process group was killed. Kimi is the only adapter that escalates a
    /// control expiry to a kill; the expiry itself is still Unknown.
    CancelGraceExpired,
    /// A terminal frame belonged to another prompt, or the acceptance receipt
    /// did not correlate with the terminal's prompt id.
    TerminalMismatch,
    /// The prompt ended with no correlated acceptance receipt, or a reverse
    /// provider request could not be served.
    ProtocolViolation,
    /// `CycleControl::fatal_error`, or the Harness's own `on_input_accepted`
    /// callback failing. Never a provider ending.
    HostAborted,
    /// The provider reported an error for this turn. The conservative
    /// default: an unclassified failure is the provider's, and its own text
    /// travels with it.
    #[default]
    ProviderError,
}

impl KimiCycleFailure {
    /// The ADR 0076 ending for this failure. `detail` is the adapter's own
    /// error text, preserved verbatim so the provider-native evidence is
    /// never replaced by the classification.
    pub(crate) fn ending(self, detail: &str) -> CycleEnding {
        match self {
            Self::SessionNotEstablished => CycleEnding::NotStarted {
                code: CycleRefusalCode::ProviderRejectedStart,
            },
            Self::PromptAlreadyActive => CycleEnding::NotStarted {
                code: CycleRefusalCode::OneDriverViolation,
            },
            Self::TransportLost => CycleEnding::TransportLost {
                detail: detail.to_string(),
            },
            Self::CancelGraceExpired => CycleEnding::ControlSettleTimeout,
            Self::TerminalMismatch => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::TerminalMismatch,
                detail: detail.to_string(),
            },
            Self::ProtocolViolation => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::ProtocolViolation,
                detail: detail.to_string(),
            },
            Self::HostAborted => CycleEnding::HostAborted {
                detail: detail.to_string(),
            },
            Self::ProviderError => {
                let failure = ProviderTerminalFailure {
                    reason: detail.to_string(),
                    http_status: None,
                };
                CycleEnding::ProviderFailed {
                    code: ProviderFailureCode::classify(&failure),
                    detail: failure.reason,
                    http_status: None,
                }
            }
        }
    }

    /// Every failure this adapter can produce, for the exhaustiveness test.
    #[cfg(test)]
    pub(crate) const ALL: &'static [Self] = &[
        Self::SessionNotEstablished,
        Self::PromptAlreadyActive,
        Self::TransportLost,
        Self::CancelGraceExpired,
        Self::TerminalMismatch,
        Self::ProtocolViolation,
        Self::HostAborted,
        Self::ProviderError,
    ];
}
