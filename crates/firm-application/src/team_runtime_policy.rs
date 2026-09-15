//! Provider-neutral Agent Team application decisions.
//!
//! Native transports report facts through `firm-runtime-contract`. This module
//! decides how those facts affect the current Team round. Durable stores and
//! process handles remain ports supplied by the executable composition root.

use firm_runtime_contract::CycleEnding;

pub const UNPRODUCTIVE_ROUND_LIMIT: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundActionStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRoundDecision {
    pub action_type: &'static str,
    pub action_status: RoundActionStatus,
    pub action_title: String,
    pub summary: String,
    pub provider_status: Option<String>,
    pub zero_output_streak: u32,
    pub circuit_breaker_open: bool,
}

/// Decide what one finished cycle does to the current Team round.
///
/// Keyed by the cycle's ADR 0076 [`CycleEnding`]: the action row's type and
/// its machine-readable `provider_status` both come from the closed table, so
/// the same observable fact can no longer be recorded two different ways on
/// two different providers. Only the human-readable title and summary are
/// composed here.
pub fn decide_team_round(
    display_name: &str,
    round: u32,
    ending: &CycleEnding,
    final_text: &str,
    semantic_done: bool,
    previous_zero_output_streak: u32,
) -> TeamRoundDecision {
    // ADR 0076: an empty terminal is EmptyOutput on all five providers, so
    // the unproductive-round streak is fed identically everywhere. Before the
    // table, Claude and DeepSeek reported the same fact as a provider failure
    // and RESET the streak instead.
    let zero_output = matches!(ending, CycleEnding::EmptyOutput);
    let zero_output_streak = if zero_output {
        previous_zero_output_streak.saturating_add(1)
    } else {
        0
    };
    let terminal_failure = ending.provider_terminal_failure();

    let (action_title, summary) = match terminal_failure.as_ref() {
        Some(failure) => {
            let status = failure
                .http_status
                .map(|code| format!(" (HTTP {code})"))
                .unwrap_or_default();
            (
                format!("{display_name} provider round {round} failed"),
                format!(
                    "{display_name} provider round {round} failed: {}{status}; transcript remains provider-native",
                    failure.reason
                ),
            )
        }
        None if zero_output => (
            format!("{display_name} provider round {round} completed without output"),
            provider_turn_coordination_summary(display_name, round, false),
        ),
        None => (
            format!("{display_name} provider round {round} completed"),
            provider_turn_coordination_summary(display_name, round, !final_text.trim().is_empty()),
        ),
    };

    TeamRoundDecision {
        action_type: ending.action_type(),
        action_status: if !ending.is_failure() && semantic_done {
            RoundActionStatus::Succeeded
        } else {
            RoundActionStatus::Failed
        },
        action_title,
        summary,
        provider_status: Some(ending.provider_status()),
        zero_output_streak,
        circuit_breaker_open: zero_output_streak >= UNPRODUCTIVE_ROUND_LIMIT,
    }
}

pub fn verified_terminal_control_ack(
    interrupted: bool,
    abort_receipt_observed: bool,
    terminal_cycle_observed: bool,
    close_requested: bool,
    close_receipt_observed: bool,
) -> bool {
    interrupted
        && abort_receipt_observed
        && terminal_cycle_observed
        && (!close_requested || close_receipt_observed)
}

pub fn circuit_breaker_reason(display_name: &str) -> String {
    format!(
        "{display_name} provider circuit breaker opened after {UNPRODUCTIVE_ROUND_LIMIT} consecutive unproductive rounds (last outcome: empty terminal success). No durable agent output was produced. Provider capacity remains unknown because the runtime adapter has no reviewed quota receipt for this outcome. Inspect the provider-native session, account access, and model-specific controls before explicitly reopening the member."
    )
}

pub fn provider_turn_coordination_summary(
    display_name: &str,
    round: u32,
    has_authored_output: bool,
) -> String {
    let output = if has_authored_output {
        "with authored output"
    } else {
        "without authored output"
    };
    format!(
        "{display_name} provider round {round} completed {output}; transcript remains provider-native"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use firm_runtime_contract::ProviderTerminalFailure;

    #[test]
    fn zero_output_opens_the_application_circuit_breaker_on_the_third_round() {
        let decision = decide_team_round("Codex", 3, &CycleEnding::EmptyOutput, "", true, 2);
        assert_eq!(decision.action_type, "empty_provider_round");
        assert_eq!(decision.action_status, RoundActionStatus::Failed);
        assert_eq!(decision.zero_output_streak, 3);
        assert!(decision.circuit_breaker_open);
    }

    #[test]
    fn provider_failure_is_not_reclassified_as_empty_success() {
        let failure = ProviderTerminalFailure {
            reason: "capacity".to_string(),
            http_status: Some(429),
        };
        let ending = CycleEnding::ProviderFailed {
            code: firm_runtime_contract::ProviderFailureCode::classify(&failure),
            detail: failure.reason.clone(),
            http_status: failure.http_status,
        };
        let decision = decide_team_round("Claude", 2, &ending, "", false, 2);
        assert_eq!(decision.action_type, "provider_error");
        assert_eq!(decision.zero_output_streak, 0);
        assert!(!decision.circuit_breaker_open);
        assert_eq!(
            decision.provider_status.as_deref(),
            Some("provider_terminal:capacity:429")
        );
    }

    #[test]
    fn close_ack_requires_both_cycle_abort_and_close_receipt() {
        assert!(!verified_terminal_control_ack(
            true, true, true, true, false
        ));
        assert!(verified_terminal_control_ack(true, true, true, true, true));
        assert!(verified_terminal_control_ack(
            true, true, true, false, false
        ));
    }
}
