//! Host dispatcher — daemon-driven headless host rounds (#387 P0-2).
//!
//! When the supervisor daemon detects actionable [`HostAttention`] rows that
//! have been pending longer than a configurable threshold and no live human
//! Host session holds the binding, it spawns a headless host round through ACP.
//! The headless host is triage-only by default: it may inspect, verify, and
//! reply to members, but must not accept, merge, or cancel Work unless
//! explicitly enabled.
//!
//! After the round completes, handled attentions are escalated to
//! [`HostAttentionStatus::EscalationRequired`] when they need human decision,
//! or left actionable for a future round when the headless host cannot
//! determine the correct action.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use firm_core::{HostAttention, HostDispatchConfig, HostDispatchOutcome, TeamRunEventSourceKind};
use firm_store::HarnessStore;

use crate::kimi_acp::{KimiAcpClient, PromptControl};
use crate::{latest_team_run, now_string, CliResult, TeamRunLedger};

/// How long the headless host prompt may run before the idle timer fires.
const HEADLESS_HOST_PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

/// Build the triage-only CONTRACT prompt for the headless host.
fn build_headless_host_prompt(
    config: &HostDispatchConfig,
    attentions: &[HostAttention],
    team_run_objective: &str,
) -> String {
    let mut prompt = String::new();

    prompt.push_str(
        "You are a triage-only Host. Process pending host_attentions: \
         read inbox, run verification, reply to members if actionable, \
         notify human user if escalation is needed. Do NOT accept/merge/cancel \
         works — those require explicit human approval.\n\n",
    );

    if config.accept_merge_enabled {
        prompt.push_str(
            "ACCEPT-MERGE MODE ENABLED: you are authorized to accept and merge \
             work that passes verification. This is a dangerous override — \
             only use it when the attention clearly describes a completed, \
             verified piece of work with no ambiguities.\n\n",
        );
    }

    prompt.push_str(&format!(
        "Team run objective: {team_run_objective}\n\n"
    ));

    prompt.push_str(&format!(
        "You have {} pending host attention(s) to process:\n\n",
        attentions.len()
    ));

    for (i, attention) in attentions.iter().enumerate() {
        prompt.push_str(&format!(
            "--- Attention {} ---\n\
             id: {}\n\
             kind: {:?}\n\
             work_id: {}\n\
             work_version: {}\n\
             member_run_id: {}\n\
             source_event_ref: {}\n\
             attempt: {}\n\
             created_at: {}\n",
            i + 1,
            attention.id,
            attention.kind,
            attention.work_id,
            attention.work_version,
            attention
                .member_run_id
                .as_deref()
                .unwrap_or("none"),
            attention.source_event_ref,
            attention.attempt,
            attention.created_at,
        ));
    }

    prompt.push_str(
        "\n--- YOUR TASK ---\n\
         1. For each attention above, evaluate whether it requires human \
         decision (accept work, reject work, cancel work, change scope) or \
         can be acknowledged automatically.\n\
         2. If you can handle the attention:\n\
            - For informational attentions (WorkAccepted, WorkCancelled, \
              WorkPrerequisiteCompleted): note it in your reply.\n\
            - For member-facing attentions (WorkReviewRequested, \
              WorkChangesRequested, WorkBlocked): if you can verify the \
              work state from available evidence, reply to the member with \
              appropriate feedback.\n\
         3. If the attention NEEDS HUMAN DECISION, mark it as ESCALATION_REQUIRED.\n\
         4. After processing all attentions, output a structured summary in \
         this EXACT format at the end of your reply:\n\n\
         ===HOST_TRIAGE_SUMMARY===\n\
         HANDLED: <comma-separated attention ids, or NONE>\n\
         ESCALATED: <comma-separated attention ids, or NONE>\n\
         FAILED: <comma-separated attention ids, or NONE>\n\
         ===END_SUMMARY===\n",
    );

    prompt
}

/// Parse the structured summary from the headless host's response text.
fn parse_triage_summary(text: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut handled = Vec::new();
    let mut escalated = Vec::new();
    let mut failed = Vec::new();

    let summary_section = if let Some(start) = text.find("===HOST_TRIAGE_SUMMARY===") {
        if let Some(end) = text.find("===END_SUMMARY===") {
            &text[start + "===HOST_TRIAGE_SUMMARY===".len()..end]
        } else {
            text
        }
    } else {
        return (handled, escalated, failed);
    };

    for line in summary_section.lines() {
        let line = line.trim();
        if let Some(ids) = line.strip_prefix("HANDLED:") {
            for id in ids.split(',') {
                let id = id.trim();
                if !id.is_empty() && id != "NONE" {
                    handled.push(id.to_string());
                }
            }
        } else if let Some(ids) = line.strip_prefix("ESCALATED:") {
            for id in ids.split(',') {
                let id = id.trim();
                if !id.is_empty() && id != "NONE" {
                    escalated.push(id.to_string());
                }
            }
        } else if let Some(ids) = line.strip_prefix("FAILED:") {
            for id in ids.split(',') {
                let id = id.trim();
                if !id.is_empty() && id != "NONE" {
                    failed.push(id.to_string());
                }
            }
        }
    }

    (handled, escalated, failed)
}

/// Entry point called periodically by the supervisor daemon's drive loop.
///
/// 1. Checks for actionable [`HostAttention`] rows older than the configured
///    age threshold.
/// 2. Checks whether a live human Host session appears to hold the binding.
///    If so, skip — the human will handle it in-session.
/// 3. Spawns a headless host round to triage the pending attentions.
pub(crate) fn poll_and_dispatch(
    store: &HarnessStore,
    ledger: &TeamRunLedger,
    team_run_objective: &str,
    config: &HostDispatchConfig,
) -> CliResult<HostDispatchOutcome> {
    let now_ms = crate::current_unix_ms_u64();
    let cutoff_ms = now_ms.saturating_sub(config.attention_age_threshold_secs * 1000);

    let actionable = store.actionable_attentions_older_than(cutoff_ms)?;
    // Filter to this team run only.
    let attentions: Vec<HostAttention> = actionable
        .into_iter()
        .filter(|a| a.team_run_id == ledger.run_id)
        .collect();

    if attentions.is_empty() {
        return Ok(HostDispatchOutcome::empty());
    }

    // If a human appears to be actively holding the host binding, skip dispatch.
    if host_binding_appears_live(store, &ledger.run_id, config, now_ms) {
        ledger.fold_event(
            TeamRunEventSourceKind::Host,
            None,
            "team_run",
            &ledger.run_id,
            "host_dispatcher",
            &format!(
                "skipped headless dispatch for {} pending attention(s): live human host binding detected",
                attentions.len()
            ),
        )?;
        return Ok(HostDispatchOutcome::empty());
    }

    // Default cwd: use the store root as the working directory for the
    // headless host ACP session.
    let cwd = store.root().to_path_buf();

    run_headless_host_round(store, ledger, team_run_objective, config, &attentions, &cwd)
}

/// Run one headless host round: spawn a kimi ACP session with the triage
/// prompt, collect the response, and apply escalation state to the store.
///
/// Returns [`HostDispatchOutcome`] describing what was processed.
pub(crate) fn run_headless_host_round(
    store: &HarnessStore,
    ledger: &TeamRunLedger,
    team_run_objective: &str,
    config: &HostDispatchConfig,
    attentions: &[HostAttention],
    cwd: &PathBuf,
) -> CliResult<HostDispatchOutcome> {
    if attentions.is_empty() {
        return Ok(HostDispatchOutcome::empty());
    }

    let attention_ids: Vec<String> = attentions.iter().map(|a| a.id.clone()).collect();
    let inspected = attentions.len();

    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        None,
        "team_run",
        &ledger.run_id,
        "host_dispatcher",
        &format!(
            "headless host round starting for {} attention(s): {}",
            inspected,
            attention_ids.join(", ")
        ),
    )?;

    let prompt = build_headless_host_prompt(config, attentions, team_run_objective);

    // Spawn a headless kimi ACP session.
    let mut client = match KimiAcpClient::spawn(
        cwd,
        None, // model — use default
        None, // effort — use default
        None, // no resume
        &[],  // no collaboration env
    ) {
        Ok(client) => client,
        Err(error) => {
            return escalate_all(
                store,
                ledger,
                &attention_ids,
                inspected,
                &format!("headless host ACP spawn failed: {error}"),
            );
        }
    };

    // Collect the response text from agent_message_chunk updates.
    let response_text: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let text_collector = Arc::clone(&response_text);

    // Run the prompt (single-turn, headless host exits after).
    let outcome = match client.prompt(
        &prompt,
        HEADLESS_HOST_PROMPT_TIMEOUT,
        |_accepted| Ok(()),
        |update| {
            if let Some(kind) = update.get("sessionUpdate").and_then(|s| s.as_str()) {
                if kind == "agent_message_chunk" {
                    if let Some(text) = update
                        .get("content")
                        .and_then(|c| c.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        if let Ok(mut collected) = text_collector.lock() {
                            collected.push_str(text);
                        }
                    }
                }
            }
        },
        |_request| Ok(serde_json::json!({})),
        || Ok(PromptControl::Continue),
    ) {
        Ok(result) => {
            let text = response_text.lock().unwrap_or_else(|e| e.into_inner());
            let (handled_ids, escalated_ids, failed_ids) = parse_triage_summary(&text);

            let now = now_string();
            let mut outcome = HostDispatchOutcome {
                inspected,
                escalated: Vec::new(),
                handled: Vec::new(),
                failed: failed_ids.clone(),
                summary: Some(format!(
                    "stop_reason={}, handled={}, escalated={}, failed={}",
                    result.stop_reason,
                    handled_ids.len(),
                    escalated_ids.len(),
                    failed_ids.len(),
                )),
            };

            // Escalate attentions the headless host flagged.
            for id in &escalated_ids {
                if attention_ids.contains(id) {
                    match store.escalate_host_attention(
                        id,
                        "headless host determined human decision is required",
                        &now,
                    ) {
                        Ok(attention) => {
                            outcome.escalated.push(attention.id);
                        }
                        Err(e) => {
                            eprintln!(
                                "[host-dispatcher] escalate {id} failed: {e}"
                            );
                            outcome.failed.push(id.clone());
                        }
                    }
                }
            }

            // Record handled attentions (not escalated, not failed).
            for id in &handled_ids {
                if attention_ids.contains(id)
                    && !escalated_ids.contains(id)
                    && !failed_ids.contains(id)
                {
                    outcome.handled.push(id.clone());
                }
            }

            ledger.fold_event(
                TeamRunEventSourceKind::Host,
                None,
                "team_run",
                &ledger.run_id,
                "host_dispatcher",
                &format!(
                    "headless host round complete: inspected={}, handled={}, escalated={}, failed={}",
                    outcome.inspected,
                    outcome.handled.len(),
                    outcome.escalated.len(),
                    outcome.failed.len(),
                ),
            )?;

            outcome
        }
        Err(error) => {
            escalate_all(
                store,
                ledger,
                &attention_ids,
                inspected,
                &format!("headless host prompt failed: {error}"),
            )?
        }
    };

    Ok(outcome)
}

/// Fallback: escalate all given attentions when the headless host is
/// unavailable or fails.
fn escalate_all(
    store: &HarnessStore,
    ledger: &TeamRunLedger,
    attention_ids: &[String],
    inspected: usize,
    reason: &str,
) -> CliResult<HostDispatchOutcome> {
    let now = now_string();
    let mut outcome = HostDispatchOutcome {
        inspected,
        escalated: Vec::new(),
        handled: Vec::new(),
        failed: attention_ids.to_vec(),
        summary: Some(reason.to_string()),
    };
    for attention_id in attention_ids {
        match store.escalate_host_attention(attention_id, reason, &now) {
            Ok(attention) => {
                outcome.escalated.push(attention.id);
            }
            Err(e) => {
                eprintln!("[host-dispatcher] escalate {attention_id} failed: {e}");
            }
        }
    }
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        None,
        "team_run",
        &ledger.run_id,
        "host_dispatcher",
        reason,
    )?;
    Ok(outcome)
}

/// Check whether a live human Host session appears to be holding the binding
/// for `team_run_id`. Returns `true` when the human is actively present and
/// the dispatcher should skip headless dispatch.
///
/// The heuristic: if the team run has an explicit host binding
/// (`host_thread_id` is set) and a recent TeamRunEvent of kind `host_binding`
/// exists within `config.attention_age_threshold_secs`, the human is
/// considered present.
///
/// Future: replaced by an explicit host binding lease (P0-1).
pub(crate) fn host_binding_appears_live(
    store: &HarnessStore,
    team_run_id: &str,
    config: &HostDispatchConfig,
    now_unix_ms: u64,
) -> bool {
    let run = match latest_team_run(store, team_run_id) {
        Ok(run) => run,
        Err(_) => return false,
    };
    if run.host_thread_id.is_none() {
        return false;
    }

    // Check for a recent host_binding event.
    let events = match store.team_run_events() {
        Ok(events) => events,
        Err(_) => return false,
    };
    let threshold_ms = config.attention_age_threshold_secs * 1000;
    for event in events.iter().rev() {
        if event.team_run_id == team_run_id && event.operation == "host_binding" {
            if let Some(ts) = event
                .occurred_at
                .strip_prefix("unix-ms:")
                .and_then(|v| v.parse::<u64>().ok())
            {
                return now_unix_ms.saturating_sub(ts) < threshold_ms;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_triage_summary_handled() {
        let response = "Some text\n===HOST_TRIAGE_SUMMARY===\nHANDLED: attn-1, attn-2\nESCALATED: attn-3\nFAILED: NONE\n===END_SUMMARY===\nMore text";
        let (handled, escalated, failed) = parse_triage_summary(response);
        assert_eq!(handled, vec!["attn-1", "attn-2"]);
        assert_eq!(escalated, vec!["attn-3"]);
        assert!(failed.is_empty());
    }

    #[test]
    fn parse_triage_summary_all_escalated() {
        let response = "===HOST_TRIAGE_SUMMARY===\nHANDLED: NONE\nESCALATED: attn-1, attn-2, attn-3\nFAILED: NONE\n===END_SUMMARY===";
        let (handled, escalated, failed) = parse_triage_summary(response);
        assert!(handled.is_empty());
        assert_eq!(escalated, vec!["attn-1", "attn-2", "attn-3"]);
        assert!(failed.is_empty());
    }

    #[test]
    fn parse_triage_summary_missing() {
        let response = "No structured summary here.";
        let (handled, escalated, failed) = parse_triage_summary(response);
        assert!(handled.is_empty());
        assert!(escalated.is_empty());
        assert!(failed.is_empty());
    }

    #[test]
    fn host_dispatch_outcome_empty() {
        let outcome = HostDispatchOutcome::empty();
        assert!(outcome.is_noop());
    }
}
