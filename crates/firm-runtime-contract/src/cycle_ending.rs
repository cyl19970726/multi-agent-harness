//! The one closed table for how an [`crate::ExecutionCycleOutcome`] — one
//! ExecutionCycle — ends (ADR 0076).
//!
//! Before this table there was no single enum naming "the ways a cycle may
//! end": the `Ok` shape had to be reconstructed by crossing three orthogonal
//! axes (terminal observed × provider terminal failure × interrupt cause),
//! and every `Err` an adapter returned collapsed into one untyped catch-all
//! action row whose only machine-readable field, `provider_status`, was
//! populated by a single provider. [`CycleEnding`] closes both halves: every
//! ending of `run_cycle`, `Ok` and `Err` alike, maps to exactly one variant.
//!
//! Scope. A [`CycleEnding`] is a SUMMARY of one finished cycle, never a
//! replacement for the outcome's own fields: `close_requested_by_harness`,
//! `interrupt` and `provider_terminal_failure` stay on
//! [`crate::ExecutionCycleOutcome`] because the settle path still needs each
//! of them independently (`verified_terminal_control_ack` reads the close and
//! interrupt flags separately from the terminal observation). It is also not
//! a semantic verdict: no variant claims Work completion or Host acceptance
//! (invariant I6).

use crate::{ExecutionCycleOutcome, InterruptCause, ProviderTerminalFailure};

/// How the durable RuntimeCommand for a cycle settles, derived from the
/// ending alone (ADR 0076 §"What each ending settles as").
///
/// This is the point of the closed table: an operator reading one ending must
/// know, without consulting the adapter, whether the input crossed the
/// provider boundary and whether the effect is proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleEndingSettlement {
    /// The cycle reached a trusted terminal boundary: `Settled / Applied /
    /// Satisfied`.
    AppliedSatisfied,
    /// Nothing crossed the provider boundary, so the effect was never
    /// applied and a replay is safe: `Rejected / NotApplied / Unsatisfied`.
    RejectedNotApplied,
    /// The input was (or may have been) accepted and the outcome cannot be
    /// proven: `RecoveryRequired / Unknown / Unknown`. Never replayed
    /// automatically.
    RecoveryRequiredUnknown,
}

/// Why the PROVIDER itself reported a failed turn.
///
/// Derived from today's producers only: Codex `codexErrorInfo` variant keys
/// and its `turn_failed` fallback, Claude/DeepSeek runner `terminalReason`
/// and `unknown_provider_error`, Kimi ACP stop reasons, Pi `error`/`length`,
/// plus the quota/auth vocabularies already closed in
/// `firm-cli/src/main_modules/provider_capacity.rs`. The provider's original
/// text is never discarded — it travels as `detail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailureCode {
    /// The account is out of capacity (HTTP 429, or a closed rate/usage/quota
    /// reason word).
    QuotaExhausted,
    /// The credential was rejected (HTTP 401/403, or a closed auth reason
    /// word).
    Unauthorized,
    /// An output or context budget ended the turn: Kimi `max_tokens`, Pi
    /// `length`.
    OutputLimit,
    /// The model declined to answer: Kimi `refusal`.
    Refusal,
    /// The provider's own agent-loop cap ended the turn: Kimi
    /// `max_turn_requests`.
    TurnRequestLimit,
    /// The provider or its runner reported an error frame for this turn:
    /// Claude/DeepSeek `runner_error`, Pi `stopReason=error`.
    RunnerError,
    /// The provider reported a failed turn with no finer classification —
    /// Codex `turn_failed`, Claude/DeepSeek `unknown_provider_error`, and any
    /// provider-authored reason outside the closed vocabularies above. The
    /// text stays in `detail`; this code means "unclassified", never "fine".
    TurnFailed,
}

impl ProviderFailureCode {
    /// Classify one provider-STRUCTURED terminal failure. Only the closed
    /// vocabularies and the exact HTTP status are read — free text is never
    /// substring-scanned, for the same reason
    /// `capacity_state_from_provider_terminal` refuses to: a member writing
    /// "fixed the 403 handler" must not classify its own account.
    pub fn classify(failure: &ProviderTerminalFailure) -> Self {
        match failure.http_status {
            Some(429) => return Self::QuotaExhausted,
            Some(401) | Some(403) => return Self::Unauthorized,
            _ => {}
        }
        match failure.reason.trim().to_ascii_lowercase().as_str() {
            "rate_limit"
            | "rate_limit_reached"
            | "usage_limit_reached"
            | "usage_limit_exceeded"
            | "usagelimitexceeded"
            | "quota_exceeded"
            | "credits_depleted" => Self::QuotaExhausted,
            "auth_error" | "authentication_error" | "forbidden" | "unauthorized" => {
                Self::Unauthorized
            }
            "max_tokens" | "length" => Self::OutputLimit,
            "refusal" => Self::Refusal,
            "max_turn_requests" => Self::TurnRequestLimit,
            "runner_error" | "error" => Self::RunnerError,
            _ => Self::TurnFailed,
        }
    }

    /// Frozen wire spelling. Stored inside `provider_status` and inside the
    /// cycle correlation's `ending`; never re-spelled per provider.
    pub fn wire(self) -> &'static str {
        match self {
            Self::QuotaExhausted => "quota_exhausted",
            Self::Unauthorized => "unauthorized",
            Self::OutputLimit => "output_limit",
            Self::Refusal => "refusal",
            Self::TurnRequestLimit => "turn_request_limit",
            Self::RunnerError => "runner_error",
            Self::TurnFailed => "turn_failed",
        }
    }
}

/// Why a cycle was refused BEFORE its input crossed the provider boundary.
///
/// Every code here is replay-safe: the provider never saw the input, so the
/// durable effect settles `Rejected / NotApplied` and the Harness may issue
/// the cycle again once the refusal is cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleRefusalCode {
    /// The adapter's runtime was already explicitly closed.
    RuntimeClosed,
    /// Starting this cycle would have created a second top-level execution
    /// driver on one native session (Codex `CODEX_ONE_DRIVER_VIOLATION`, Kimi
    /// "already has an active session/prompt").
    OneDriverViolation,
    /// A provider-native continuation was armed, so the Harness may not also
    /// drive the session.
    ContinuationArmed,
    /// The provider itself refused to open the turn — it answered the start
    /// request with an error, or omitted the id the cycle must correlate
    /// against. Still replay-safe: no turn began.
    ProviderRejectedStart,
}

impl CycleRefusalCode {
    pub fn wire(self) -> &'static str {
        match self {
            Self::RuntimeClosed => "runtime_closed",
            Self::OneDriverViolation => "one_driver_violation",
            Self::ContinuationArmed => "continuation_armed",
            Self::ProviderRejectedStart => "provider_rejected_start",
        }
    }
}

/// Why a terminal frame could not be TRUSTED.
///
/// Deliberately not [`CycleEnding::ProviderFailed`]: "the provider reported a
/// failed turn" and "we cannot trust what we saw" are different facts with
/// different settlements. A provider failure is an observed terminal with
/// `Unsatisfied`; an unobservable terminal is `Unknown`, and the cycle
/// becomes explicit recovery work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalUnobservedCode {
    /// The provider broke the cycle protocol — a terminal frame preceded its
    /// acceptance receipt, or an interrupt did
    /// (`CLAUDE_AGENT_SDK_PROTOCOL_ERROR`, `DEEPSEEK_HARNESS_PROTOCOL_ERROR`).
    ProtocolViolation,
    /// A terminal frame arrived for another input, another turn, or another
    /// session (`KIMI_CYCLE_TERMINAL_MISMATCH`,
    /// `CLAUDE_AGENT_SDK_INTERRUPT_RESUME_MISMATCH`).
    TerminalMismatch,
    /// The terminal frame was seen but its independent postcondition was not
    /// (`CODEX_RUNTIME_POSTCONDITION_UNKNOWN`,
    /// `PI_CYCLE_SETTLEMENT_UNKNOWN`).
    PostconditionUnknown,
    /// The provider runtime closed itself mid-cycle without a Harness
    /// CloseRuntime (`CLAUDE_AGENT_SDK_UNEXPECTED_CLOSE`,
    /// `DEEPSEEK_HARNESS_UNEXPECTED_CLOSE`).
    UnexpectedClose,
}

impl TerminalUnobservedCode {
    pub fn wire(self) -> &'static str {
        match self {
            Self::ProtocolViolation => "protocol_violation",
            Self::TerminalMismatch => "terminal_mismatch",
            Self::PostconditionUnknown => "postcondition_unknown",
            Self::UnexpectedClose => "unexpected_close",
        }
    }
}

/// How one ExecutionCycle ended — the closed table (ADR 0076).
///
/// Every `run_cycle` ending maps to exactly one variant. Adapters name their
/// own endings through a local wildcard-free enum and convert here, so adding
/// an ending to an adapter is a compile error until it is placed in this
/// table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleEnding {
    /// Trusted terminal, no failure, authored output or tool calls.
    Completed,
    /// Trusted terminal, no failure, no authored text and no tool call. This
    /// is the zero-output fact on ALL five providers (ADR 0076): it is not a
    /// provider failure anywhere, and it counts toward the unproductive-round
    /// circuit breaker everywhere.
    EmptyOutput,
    /// Trusted terminal reached because the HOST interrupted the turn.
    InterruptedByHost,
    /// Trusted terminal that the PROVIDER ended as interrupted with no
    /// Harness control request.
    InterruptedByProvider { reason: String },
    /// Trusted terminal reached under a Harness Close request. Close
    /// dominates Interrupt in the precedence order because the member runtime
    /// is going away either way.
    Closed,
    /// The provider reported a failed turn. `detail` is the provider's own
    /// text, preserved verbatim.
    ProviderFailed {
        code: ProviderFailureCode,
        detail: String,
        http_status: Option<i64>,
    },
    /// The transport or child process died before a terminal was observed.
    TransportLost { detail: String },
    /// The input was written but never acknowledged within
    /// `CycleTimeouts::input_acceptance`.
    AcceptanceTimeout,
    /// An issued Interrupt/Close was never settled within
    /// `CycleTimeouts::control_settle`.
    ControlSettleTimeout,
    /// The cycle was refused before its input crossed the provider boundary.
    NotStarted { code: CycleRefusalCode },
    /// The HARNESS ended the cycle itself: a `CycleControl::fatal_error`, or
    /// its own `on_input_accepted` callback failing. Never a provider ending.
    HostAborted { detail: String },
    /// A terminal was reported but could not be trusted.
    TerminalUnobserved {
        code: TerminalUnobservedCode,
        detail: String,
    },
}

impl CycleEnding {
    /// The `Ok` half of the table: project one finished outcome onto exactly
    /// one ending.
    ///
    /// Precedence (ADR 0076 §Precedence), because close, interrupt, failure
    /// and emptiness can all be true of the same outcome:
    /// `ProviderFailed > Closed > InterruptedByHost > InterruptedByProvider >
    /// EmptyOutput > Completed`. A reported provider failure outranks
    /// everything because it is the only fact that makes the postcondition
    /// `Unsatisfied`; Close outranks Interrupt because Close is why the
    /// runtime is ending, and the outcome's own flags remain available to the
    /// settle path either way.
    ///
    /// The match is wildcard-free over the interrupt axis, so a new
    /// [`InterruptCause`] cannot silently inherit another variant's meaning.
    pub fn from_outcome(outcome: &ExecutionCycleOutcome) -> Self {
        if let Some(failure) = outcome.provider_terminal_failure.as_ref() {
            return Self::ProviderFailed {
                code: ProviderFailureCode::classify(failure),
                detail: failure.reason.clone(),
                http_status: failure.http_status,
            };
        }
        if outcome.close_requested_by_harness {
            return Self::Closed;
        }
        match &outcome.interrupt {
            Some(InterruptCause::HostControl) => return Self::InterruptedByHost,
            Some(InterruptCause::AdapterPolicy { reason }) => {
                return Self::InterruptedByProvider {
                    reason: format!("adapter_policy:{reason}"),
                }
            }
            Some(InterruptCause::ProviderInitiated { reason }) => {
                return Self::InterruptedByProvider {
                    reason: reason.clone(),
                }
            }
            None => {}
        }
        if outcome.final_text.trim().is_empty() && outcome.tool_call_count == 0 {
            return Self::EmptyOutput;
        }
        Self::Completed
    }

    /// Frozen wire spelling, recorded on the cycle correlation. Stable across
    /// releases: a stored row must stay readable.
    pub fn wire(&self) -> String {
        match self {
            Self::Completed => "completed".to_string(),
            Self::EmptyOutput => "empty_output".to_string(),
            Self::InterruptedByHost => "interrupted_by_host".to_string(),
            Self::InterruptedByProvider { .. } => "interrupted_by_provider".to_string(),
            Self::Closed => "closed".to_string(),
            Self::ProviderFailed { code, .. } => format!("provider_failed:{}", code.wire()),
            Self::TransportLost { .. } => "transport_lost".to_string(),
            Self::AcceptanceTimeout => "acceptance_timeout".to_string(),
            Self::ControlSettleTimeout => "control_settle_timeout".to_string(),
            Self::NotStarted { code } => format!("not_started:{}", code.wire()),
            Self::HostAborted { .. } => "host_aborted".to_string(),
            Self::TerminalUnobserved { code, .. } => {
                format!("terminal_unobserved:{}", code.wire())
            }
        }
    }

    /// The FIXED `member_actions.action_type` for this ending. The column
    /// stays a string on the wire; these values are frozen, and the four
    /// pre-existing spellings (`turn_completed`, `empty_provider_round`,
    /// `interrupted`, `provider_error`) keep their meaning so historical rows
    /// stay comparable.
    pub fn action_type(&self) -> &'static str {
        match self {
            Self::Completed => "turn_completed",
            Self::EmptyOutput => "empty_provider_round",
            Self::InterruptedByHost | Self::InterruptedByProvider { .. } => "interrupted",
            Self::Closed => "closed",
            Self::ProviderFailed { .. } => "provider_error",
            Self::TransportLost { .. } => "transport_lost",
            Self::AcceptanceTimeout => "input_never_accepted",
            Self::ControlSettleTimeout => "control_settle_timeout",
            Self::NotStarted { .. } => "cycle_not_started",
            Self::HostAborted { .. } => "host_aborted",
            Self::TerminalUnobserved { .. } => "terminal_unobserved",
        }
    }

    /// How the durable RuntimeCommand for this cycle settles.
    ///
    /// `input_accepted` is whether the exact provider input-acceptance
    /// receipt for THIS cycle exists. It is a parameter rather than a field
    /// because four endings are genuinely two-valued: a transport that dies
    /// before the input is accepted never applied anything (replay-safe),
    /// while the same death after acceptance leaves an outcome nobody can
    /// prove (invariant I2 — "accepted, outcome unproven", never
    /// "not applied"). The three `Ok` classes cannot occur without a receipt,
    /// and the two never-crossed classes cannot occur with one.
    pub fn settlement(&self, input_accepted: bool) -> CycleEndingSettlement {
        match self {
            Self::Completed
            | Self::EmptyOutput
            | Self::InterruptedByHost
            | Self::InterruptedByProvider { .. }
            | Self::Closed
            | Self::ProviderFailed { .. } => CycleEndingSettlement::AppliedSatisfied,
            Self::NotStarted { .. } | Self::AcceptanceTimeout => {
                CycleEndingSettlement::RejectedNotApplied
            }
            Self::TransportLost { .. }
            | Self::ControlSettleTimeout
            | Self::TerminalUnobserved { .. }
            | Self::HostAborted { .. } => {
                if input_accepted {
                    CycleEndingSettlement::RecoveryRequiredUnknown
                } else {
                    CycleEndingSettlement::RejectedNotApplied
                }
            }
        }
    }

    /// Whether this ending records a FAILED member action.
    ///
    /// `Completed` is the only ending that can carry a succeeded status, and
    /// even then only when the application's own semantic check agrees —
    /// provider satisfaction never implies Host acceptance (invariant I6).
    pub fn is_failure(&self) -> bool {
        !matches!(self, Self::Completed)
    }

    /// The machine-readable `provider_status` column for this ending, so an
    /// operator can classify EVERY ending, on every provider, without reading
    /// free text.
    ///
    /// A provider failure keeps the existing `provider_terminal:{reason}:
    /// {http}` shape byte for byte, so `ProviderTerminalFailure::parse` and
    /// the capacity classifier keep working unchanged. Every other ending
    /// uses the distinct `cycle_ending:{wire}` prefix, which `parse`
    /// deliberately does not match: a transport loss is not a provider
    /// terminal and must never be classified as one.
    pub fn provider_status(&self) -> String {
        match self.provider_terminal_failure() {
            Some(failure) => failure.to_provider_status(),
            None => format!("cycle_ending:{}", self.wire()),
        }
    }

    /// The provider's own terminal failure, reconstructed verbatim. This is
    /// what feeds the shared `TeamRuntimeAdapter::take_cycle_terminal_failure`
    /// default, so no adapter has to implement that method to get a
    /// structured failure onto its action row.
    pub fn provider_terminal_failure(&self) -> Option<ProviderTerminalFailure> {
        match self {
            Self::ProviderFailed {
                detail,
                http_status,
                ..
            } => Some(ProviderTerminalFailure {
                reason: detail.clone(),
                http_status: *http_status,
            }),
            Self::Completed
            | Self::EmptyOutput
            | Self::InterruptedByHost
            | Self::InterruptedByProvider { .. }
            | Self::Closed
            | Self::TransportLost { .. }
            | Self::AcceptanceTimeout
            | Self::ControlSettleTimeout
            | Self::NotStarted { .. }
            | Self::HostAborted { .. }
            | Self::TerminalUnobserved { .. } => None,
        }
    }
}
