//! Host attention dispatcher — polls the store for actionable HostAttention
//! records and dispatches them to the appropriate handler.
//!
//! Also runs daemon-side member progress probes: reads each member's
//! provider-native session wire, classifies their state, and generates
//! host attentions for FAILING members or steer suggestions for WAIT_LOOP.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use harness_core::{
    HostAttention, HostAttentionKind, HostAttentionStatus, HostDispatchConfig, MemberRun,
};

use crate::member_probe::{probe_member, MemberProbeResult};
use crate::{now_string, CliError, CliResult, TeamRunLedger};

/// Outcome of a single dispatch poll.
#[derive(Debug)]
pub struct DispatchOutcome {
    pub inspected: usize,
    pub handled: Vec<String>,
    pub escalated: Vec<String>,
    pub failed: Vec<String>,
    /// Members that were probed and their classifications (for logging).
    pub probes: Vec<MemberProbeResult>,
}

impl DispatchOutcome {
    pub fn is_noop(&self) -> bool {
        self.handled.is_empty()
            && self.escalated.is_empty()
            && self.failed.is_empty()
            && self.probes.is_empty()
    }
}

/// Per-member probe tracking state (cooldown, wait-loop cycles).
#[derive(Debug, Clone)]
struct ProbeState {
    last_probe_at: Instant,
    /// Consecutive WAIT_LOOP classifications.
    wait_loop_streak: u32,
}

/// Mutable probe state shared across poll cycles.
pub struct ProbeTracker {
    states: Mutex<HashMap<String, ProbeState>>,
    /// Minimum time between probes for a single member.
    cooldown: Duration,
    /// After this many consecutive WAIT_LOOP probes, a steer suggestion is emitted.
    wait_loop_steer_threshold: u32,
}

impl ProbeTracker {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
            cooldown,
            wait_loop_steer_threshold: 3,
        }
    }

    /// Check whether a member is due for a probe.
    fn is_due(&self, member_id: &str) -> bool {
        let states = self.states.lock().unwrap_or_else(|e| e.into_inner());
        match states.get(member_id) {
            Some(state) => state.last_probe_at.elapsed() >= self.cooldown,
            None => true,
        }
    }

    /// Record a probe result and return the wait-loop streak for this cycle.
    fn record(
        &self,
        member_id: &str,
        classification: &harness_core::MemberProbeClassification,
    ) -> u32 {
        let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
        let entry = states.entry(member_id.to_string()).or_insert(ProbeState {
            last_probe_at: Instant::now(),
            wait_loop_streak: 0,
        });
        entry.last_probe_at = Instant::now();

        if matches!(classification, harness_core::MemberProbeClassification::WaitLoop { .. }) {
            entry.wait_loop_streak = entry.wait_loop_streak.saturating_add(1);
        } else {
            entry.wait_loop_streak = 0;
        }
        entry.wait_loop_streak
    }
}

/// Poll the store for actionable host attention records and dispatch them.
/// Also probes all active members whose sessions are due.
pub fn poll_and_dispatch(
    store: &harness_store::HarnessStore,
    ledger: &std::sync::Arc<TeamRunLedger>,
    _objective: &str,
    config: &HostDispatchConfig,
    probe_tracker: &ProbeTracker,
    active_members: &[MemberRun],
) -> Result<DispatchOutcome, CliError> {
    let mut outcome = DispatchOutcome {
        inspected: 0,
        handled: vec![],
        escalated: vec![],
        failed: vec![],
        probes: vec![],
    };

    // ── 1. Probe active members ───────────────────────────────────────────
    let stale_threshold = Duration::from_secs(config.attention_age_threshold_secs);
    for member in active_members {
        if !probe_tracker.is_due(&member.id) {
            continue;
        }
        let Some(session) = member.native_session.as_ref() else {
            continue;
        };

        let result = match probe_member(session, &member.id, 200, stale_threshold) {
            Ok(r) => r,
            Err(e) => {
                // Wire read failure is not fatal — log and skip.
                eprintln!(
                    "[host-dispatcher] probe failed for member {}: {e}",
                    member.id
                );
                continue;
            }
        };

        let classification = result.classification.clone();
        let wait_loop_streak = probe_tracker.record(&member.id, &classification);

        match &classification {
            harness_core::MemberProbeClassification::Failing {
                total_tool_calls,
                failed_tool_calls,
            } => {
                // Generate a member_distress host attention.
                let attention_id = format!("host-attention-probe-{}", member.id);
                let attention = HostAttention {
                    id: attention_id.clone(),
                    team_run_id: member.team_run_id.clone(),
                    kind: HostAttentionKind::MemberDistress,
                    work_id: member
                        .id
                        .clone(), // Use member_run_id as work_id for distress
                    work_version: 0,
                    source_event_ref: format!(
                        "probe:{}:failing:{}/{}",
                        member.id, failed_tool_calls, total_tool_calls
                    ),
                    member_run_id: Some(member.id.clone()),
                    status: HostAttentionStatus::Actionable,
                    attempt: 0,
                    claim_id: None,
                    claimed_host_surface: None,
                    claimed_host_thread_id: None,
                    provider_receipt_id: None,
                    last_failure_reason: Some(format!(
                        "member {} is FAILING: {}/{} tool calls failed, zero edits",
                        member.name, failed_tool_calls, total_tool_calls
                    )),
                    created_at: now_string(),
                    updated_at: now_string(),
                };
                if let Err(e) = store.append_host_attention(&attention) {
                    eprintln!(
                        "[host-dispatcher] failed to write distress attention for {}: {e}",
                        member.id
                    );
                    outcome.failed.push(member.id.clone());
                } else {
                    outcome.escalated.push(member.id.clone());
                }
            }
            harness_core::MemberProbeClassification::WaitLoop {
                repeated_call,
                repetition_count,
            } if wait_loop_streak >= probe_tracker.wait_loop_steer_threshold => {
                // Emit a steer suggestion as an event on the ledger.
                let _ = ledger.fold_event(
                    harness_core::TeamRunEventSourceKind::Host,
                    Some(member.id.clone()),
                    "member_run",
                    &member.id,
                    "steer_suggestion",
                    &format!(
                        "member {} is in WAIT_LOOP ({}x repeated '{}' for {} cycles). \
                         Consider steering with a control message.",
                        member.name, repetition_count, repeated_call, wait_loop_streak
                    ),
                );
                outcome.handled.push(member.id.clone());
            }
            harness_core::MemberProbeClassification::Dead {
                last_modified_secs_ago,
            } => {
                // Dead session — emit a distress attention.
                let attention_id = format!("host-attention-probe-dead-{}", member.id);
                let attention = HostAttention {
                    id: attention_id.clone(),
                    team_run_id: member.team_run_id.clone(),
                    kind: HostAttentionKind::MemberDistress,
                    work_id: member.id.clone(),
                    work_version: 0,
                    source_event_ref: format!(
                        "probe:{}:dead:{}s",
                        member.id, last_modified_secs_ago
                    ),
                    member_run_id: Some(member.id.clone()),
                    status: HostAttentionStatus::Actionable,
                    attempt: 0,
                    claim_id: None,
                    claimed_host_surface: None,
                    claimed_host_thread_id: None,
                    provider_receipt_id: None,
                    last_failure_reason: Some(format!(
                        "member {} appears DEAD: session unmodified for {}s",
                        member.name, last_modified_secs_ago
                    )),
                    created_at: now_string(),
                    updated_at: now_string(),
                };
                if let Err(e) = store.append_host_attention(&attention) {
                    eprintln!(
                        "[host-dispatcher] failed to write dead-member attention for {}: {e}",
                        member.id
                    );
                    outcome.failed.push(member.id.clone());
                } else {
                    outcome.escalated.push(member.id.clone());
                }
            }
            _ => {
                // PRODUCING / INVESTIGATING — no action needed.
            }
        }

        outcome.probes.push(result);
        outcome.inspected += 1;
    }

    // ── 2. Existing stored attentions (future: dispatch to host) ──────────
    // Stub: stored attentions are read by the CLI `host-inbox` command.
    // Full headless dispatch tracked in #387 P0-2.

    Ok(outcome)
}
