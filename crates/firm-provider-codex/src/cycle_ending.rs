//! Every way a Codex cycle can end in `Err`, and its ADR 0076 mapping.
//!
//! The `Ok` half of the table needs nothing here: `CycleEnding::from_outcome`
//! derives it from the outcome the adapter already returns. This enum names
//! the other half, so an `Err` leaving `run_cycle` can never reach the shared
//! loop as an untyped catch-all.

use harness_runtime_contract::{
    CycleEnding, CycleRefusalCode, ProviderFailureCode, TerminalUnobservedCode,
};

/// The closed set of `Err` endings this adapter produces.
///
/// Adding a variant is a compile error until it is placed in the table below,
/// and the mapping match is deliberately wildcard-free for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexCycleFailure {
    /// The adapter's runtime was already explicitly closed.
    RuntimeClosed,
    /// Starting this cycle would have created a second top-level execution
    /// driver on the thread (`CODEX_ONE_DRIVER_VIOLATION`).
    OneDriverViolation,
    /// A native Goal continuation was armed, so Harness may not also drive.
    ContinuationArmed,
    /// The app-server answered `turn/start` with an error, or omitted the
    /// turn id the cycle must correlate against.
    StartRejected,
    /// `CycleControl::fatal_error`: the Harness itself ended the cycle.
    HostFatalControl,
    /// The Harness's own `on_input_accepted` callback failed (a durable
    /// settlement write), not the provider.
    AcceptanceCallbackFailed,
    /// `turn/start` was never acknowledged within `input_acceptance`. The
    /// app-server RPC deadline IS the acceptance bound here, so its expiry is
    /// replay-safe: the turn never began.
    InputAcceptanceTimeout,
    /// The app-server transport disconnected before `turn/completed`, or a
    /// bridge RPC failed on a dead transport.
    TransportClosed,
    /// `turn/interrupt` was acknowledged but `turn/completed` was never
    /// observed (`CODEX_RUNTIME_CONTROL_UNKNOWN`).
    ControlSettleTimeout,
    /// The turn ended with a status outside {completed, interrupted, failed}.
    UnknownTerminalStatus,
    /// The terminal frame or its independent idle postcondition could not be
    /// proven (`CODEX_RUNTIME_POSTCONDITION_UNKNOWN`).
    PostconditionUnknown,
    /// A frame belonged to another thread, turn or descendant than the one
    /// this cycle admitted.
    TerminalMismatch,
    /// The provider asked for something outside the reviewed protocol and the
    /// adapter refused it fail-closed mid-turn
    /// (`CODEX_PROVIDER_REQUEST_UNSAFE` / `_UNSUPPORTED` / `_UNHANDLED`).
    ProtocolViolation,
}

impl CodexCycleFailure {
    /// The ADR 0076 ending for this failure. `detail` is the adapter's own
    /// error text, preserved verbatim so the provider-native evidence is
    /// never replaced by the classification.
    pub(crate) fn ending(self, detail: &str) -> CycleEnding {
        match self {
            Self::RuntimeClosed => CycleEnding::NotStarted {
                code: CycleRefusalCode::RuntimeClosed,
            },
            Self::OneDriverViolation => CycleEnding::NotStarted {
                code: CycleRefusalCode::OneDriverViolation,
            },
            Self::ContinuationArmed => CycleEnding::NotStarted {
                code: CycleRefusalCode::ContinuationArmed,
            },
            Self::StartRejected => CycleEnding::NotStarted {
                code: CycleRefusalCode::ProviderRejectedStart,
            },
            Self::HostFatalControl | Self::AcceptanceCallbackFailed => CycleEnding::HostAborted {
                detail: detail.to_string(),
            },
            Self::InputAcceptanceTimeout => CycleEnding::AcceptanceTimeout,
            Self::TransportClosed => CycleEnding::TransportLost {
                detail: detail.to_string(),
            },
            Self::ControlSettleTimeout => CycleEnding::ControlSettleTimeout,
            // The provider DID report a terminal for this turn; it is its
            // status word we cannot place. That is a provider-reported
            // failure, not an unobservable terminal.
            Self::UnknownTerminalStatus => CycleEnding::ProviderFailed {
                code: ProviderFailureCode::TurnFailed,
                detail: detail.to_string(),
                http_status: None,
            },
            Self::PostconditionUnknown => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::PostconditionUnknown,
                detail: detail.to_string(),
            },
            Self::TerminalMismatch => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::TerminalMismatch,
                detail: detail.to_string(),
            },
            Self::ProtocolViolation => CycleEnding::TerminalUnobserved {
                code: TerminalUnobservedCode::ProtocolViolation,
                detail: detail.to_string(),
            },
        }
    }

    /// Every failure this adapter can produce, for the exhaustiveness test.
    #[cfg(test)]
    pub(crate) const ALL: &'static [Self] = &[
        Self::RuntimeClosed,
        Self::OneDriverViolation,
        Self::ContinuationArmed,
        Self::StartRejected,
        Self::HostFatalControl,
        Self::AcceptanceCallbackFailed,
        Self::InputAcceptanceTimeout,
        Self::TransportClosed,
        Self::ControlSettleTimeout,
        Self::UnknownTerminalStatus,
        Self::PostconditionUnknown,
        Self::TerminalMismatch,
        Self::ProtocolViolation,
    ];
}
