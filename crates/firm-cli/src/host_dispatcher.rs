// host_dispatcher — daemon-driven headless host round mechanism (#387 P0-2)
//
// When the human host session goes idle, the supervisor daemon polls host_attentions
// and can spawn headless host rounds to triage pending attentions (read inbox, run
// verification, reply to members, escalate to human). Accept/merge actions require
// explicit human approval via the accept_merge_enabled flag.
//
// Evidence: session_9c6cfb21 — host session went idle at 11:26 while members kept
// producing work submissions at 11:40/11:45, creating actionable work_review_requested
// attentions with attempts:0 and no host to consume them.

use firm_core::{HostDispatchConfig, HostDispatchOutcome};
use firm_store::HarnessStore;
use std::time::Duration;

/// Check whether the host binding appears to be live (a human is actively attending).
///
/// Uses a heuristic based on recent host_binding events. Will be replaced by the
/// explicit host lease mechanism from P0-1 when available.
pub fn host_binding_appears_live(
    _store: &HarnessStore,
    _run_id: &str,
    attention_age: Duration,
) -> bool {
    // Placeholder heuristic: if the attention is less than 2x the minimum age,
    // assume the host might still be around and skip dispatch.
    // This will be replaced by the explicit lease check from P0-1 (PR #395).
    attention_age < Duration::from_secs(600)
}

/// Build the CONTRACT prompt for a headless host round.
pub fn build_headless_host_prompt(
    accept_merge_enabled: bool,
) -> String {
    let mut prompt = String::from(
        "You are a triage-only Host agent for an Agent Team operating system.\n\
         Process pending host_attentions from the team run:\n\
         - Read host inbox and unacknowledged messages\n\
         - Run verification commands if work review is requested\n\
         - Reply to members with status updates or clarification requests\n\
         - Escalate to the human user when judgment is required\n\n",
    );

    if accept_merge_enabled {
        prompt.push_str(
            "EXPLICIT PERMISSION GRANTED: You may accept, merge, or cancel works.\n\
             Exercise this power with caution and evidence.\n",
        );
    } else {
        prompt.push_str(
            "CRITICAL: Do NOT accept, merge, or cancel any works.\n\
             These actions require explicit human approval.\n\
             If a work appears ready for acceptance, escalate it instead.\n",
        );
    }

    prompt.push_str(
        "\nOutput your findings in the following format:\n\
         ===HOST_TRIAGE_SUMMARY===\n\
         HANDLED: <comma-separated attention IDs>\n\
         ESCALATED: <comma-separated attention IDs>\n\
         FAILED: <comma-separated attention IDs>\n\
         NOTES: <free-text summary of actions taken>\n\
         ===END_SUMMARY===",
    );

    prompt
}

/// Parse the structured triage summary from a headless host response.
pub fn parse_triage_summary(
    response: &str,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut handled = Vec::new();
    let mut escalated = Vec::new();
    let mut failed = Vec::new();

    for line in response.lines() {
        let trimmed = line.trim();
        if let Some(ids) = trimmed.strip_prefix("HANDLED:") {
            handled = ids.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        } else if let Some(ids) = trimmed.strip_prefix("ESCALATED:") {
            escalated = ids.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        } else if let Some(ids) = trimmed.strip_prefix("FAILED:") {
            failed = ids.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
    }

    (handled, escalated, failed)
}

/// Main dispatch loop entry point. Called periodically by the supervisor daemon
/// when the host dispatch interval has elapsed.
///
/// Returns a noop outcome if no actionable attentions are found, or if the
/// host binding appears live (human is actively attending).
pub fn poll_and_dispatch(
    store: &HarnessStore,
    _run_id: &str,
    _objective: &str,
    config: &HostDispatchConfig,
) -> Result<HostDispatchOutcome, String> {
    let outcome = HostDispatchOutcome {
        inspected: 0,
        escalated: Vec::new(),
        handled: Vec::new(),
        failed: Vec::new(),
        summary: None,
    };

    // Check if host binding is live — if so, skip this poll.
    if host_binding_appears_live(store, "", Duration::from_secs(config.attention_age_threshold_secs)) {
        return Ok(outcome); // noop
    }

    // Placeholder: full attention polling and headless host spawning
    // will be implemented after the store-level attention query methods
    // from P0-1 (PR #395) are available on master.

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_triage_only() {
        let config = HostDispatchConfig::default();
        assert!(!config.accept_merge_enabled);
        assert_eq!(config.poll_interval_secs, 60);
        assert_eq!(config.attention_age_threshold_secs, 300);
    }

    #[test]
    fn default_outcome_is_noop() {
        let outcome = HostDispatchOutcome {
        inspected: 0,
        escalated: Vec::new(),
        handled: Vec::new(),
        failed: Vec::new(),
        summary: None,
    };
        assert!(outcome.is_noop());
    }

    #[test]
    fn triage_prompt_forbids_accept_merge() {
        let prompt = build_headless_host_prompt(false);
        assert!(prompt.contains("Do NOT accept, merge, or cancel"));
        assert!(!prompt.contains("EXPLICIT PERMISSION GRANTED"));
    }

    #[test]
    fn enabled_prompt_allows_accept_merge() {
        let prompt = build_headless_host_prompt(true);
        assert!(prompt.contains("EXPLICIT PERMISSION GRANTED"));
    }
}
