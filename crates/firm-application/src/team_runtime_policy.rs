//! Provider-neutral Agent Team application decisions.
//!
//! Native transports report facts through `firm-runtime-contract`. This module
//! decides how those facts affect the current Team round. Durable stores and
//! process handles remain ports supplied by the executable composition root.

use firm_runtime_contract::ProviderTerminalFailure;

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

pub fn decide_team_round(
    display_name: &str,
    round: u32,
    final_text: &str,
    tool_call_count: u32,
    terminal_failure: Option<&ProviderTerminalFailure>,
    semantic_done: bool,
    previous_zero_output_streak: u32,
) -> TeamRoundDecision {
    let zero_output =
        terminal_failure.is_none() && final_text.trim().is_empty() && tool_call_count == 0;
    let zero_output_streak = if zero_output {
        previous_zero_output_streak.saturating_add(1)
    } else {
        0
    };

    let (action_type, action_title, summary, provider_status) = if let Some(failure) =
        terminal_failure
    {
        let status = failure
            .http_status
            .map(|code| format!(" (HTTP {code})"))
            .unwrap_or_default();
        (
                "provider_error",
                format!("{display_name} provider round {round} failed"),
                format!(
                    "{display_name} provider round {round} failed: {}{status}; transcript remains provider-native",
                    failure.reason
                ),
                Some(failure.to_provider_status()),
            )
    } else if zero_output {
        (
            "empty_provider_round",
            format!("{display_name} provider round {round} completed without output"),
            provider_turn_coordination_summary(display_name, round, false),
            None,
        )
    } else {
        (
            "turn_completed",
            format!("{display_name} provider round {round} completed"),
            provider_turn_coordination_summary(display_name, round, !final_text.trim().is_empty()),
            None,
        )
    };

    TeamRoundDecision {
        action_type,
        action_status: if !zero_output && terminal_failure.is_none() && semantic_done {
            RoundActionStatus::Succeeded
        } else {
            RoundActionStatus::Failed
        },
        action_title,
        summary,
        provider_status,
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

    #[test]
    fn zero_output_opens_the_application_circuit_breaker_on_the_third_round() {
        let decision = decide_team_round("Codex", 3, "", 0, None, true, 2);
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
        let decision = decide_team_round("Claude", 2, "", 0, Some(&failure), false, 2);
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
