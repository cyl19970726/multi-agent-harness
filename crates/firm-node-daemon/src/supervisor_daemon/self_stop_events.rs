//! Durable, per-TeamRun evidence for a NodeDaemon self-stop.
//!
//! Machine authority is one process-wide fence, while TeamRun events live in
//! per-Execution-Space stores. Capture the served runs at the loss boundary,
//! then journal every observed shutdown phase through the canonical
//! `team_run_events.jsonl` writer. A failed journal write is repeated only a
//! bounded number of times and is always emitted to stderr, which is the
//! detached daemon's durable log.

use super::*;

const SELF_STOP_EVENT_WRITE_ATTEMPTS: usize = 3;
const SELF_STOP_EVENT_WRITE_BACKOFF: Duration = Duration::from_millis(25);
pub(super) const MACHINE_AUTHORITY_LOST_REASON: &str = "NODE_DAEMON_MACHINE_AUTHORITY_LOST";
/// A stop whose own drain did not converge is not an authority loss, but it
/// leaves the same kind of hole: this generation stops without proving its
/// process groups terminal. It journals the identical phase sequence under its
/// own honest reason (ADR 0073).
pub(super) const DRAIN_INCOMPLETE_REASON: &str = "NODE_DAEMON_DRAIN_INCOMPLETE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ServedTeamRun {
    execution_space_id: String,
    team_run_id: String,
    daemon_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineAuthorityLoss {
    /// Why this generation is stopping without settling. Never invented: an
    /// ordinary drain failure must not be journaled as a lost lease.
    reason: &'static str,
    trigger_error: String,
    served_runs: Vec<ServedTeamRun>,
}

impl MultiTeamDaemon {
    /// Preserve only the first renewal failure: later drain errors are useful
    /// diagnostics, but must not overwrite the trigger that caused self-stop.
    pub(super) fn capture_machine_authority_loss(&self, failures: &[String]) -> bool {
        self.capture_self_stop(MACHINE_AUTHORITY_LOST_REASON, failures)
    }

    /// Same snapshot, different honest reason: a stop that could not prove its
    /// own drain. Capturing it is what makes the shutdown phases land at all —
    /// `journal_machine_authority_loss_phase` writes nothing without a
    /// captured self-stop, so an unconverged drain used to leave no journal.
    ///
    /// Unlike the loss latch, this capture can only be made *after* the drain
    /// verdict is known, and by then `graceful_shutdown` has already emptied
    /// `contexts`. The caller therefore takes the snapshot with
    /// `snapshot_served_runs` before the drain starts and hands it in here.
    pub(super) fn capture_incomplete_drain(
        &self,
        served_runs: Vec<ServedTeamRun>,
        failures: &[String],
    ) -> bool {
        self.capture_self_stop_with(DRAIN_INCOMPLETE_REASON, served_runs, failures)
    }

    /// The runs this generation is serving right now, deduplicated and ordered.
    /// Reading it costs nothing and holds no lock across the drain, so a stop
    /// path can take it before `graceful_shutdown` drains `contexts`.
    pub(super) fn snapshot_served_runs(&self) -> Vec<ServedTeamRun> {
        let mut served_runs = self
            .contexts
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .map(|context| ServedTeamRun {
                execution_space_id: context.execution_space_id.clone(),
                team_run_id: context.run_id.clone(),
                daemon_generation: context.daemon_generation,
            })
            .collect::<Vec<_>>();
        served_runs.sort_by(|left, right| {
            (&left.execution_space_id, &left.team_run_id)
                .cmp(&(&right.execution_space_id, &right.team_run_id))
        });
        served_runs.dedup_by(|left, right| {
            left.execution_space_id == right.execution_space_id
                && left.team_run_id == right.team_run_id
        });
        served_runs
    }

    fn capture_self_stop(&self, reason: &'static str, failures: &[String]) -> bool {
        self.capture_self_stop_with(reason, self.snapshot_served_runs(), failures)
    }

    fn capture_self_stop_with(
        &self,
        reason: &'static str,
        served_runs: Vec<ServedTeamRun>,
        failures: &[String],
    ) -> bool {
        let mut loss = self
            .machine_authority_loss
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if loss.is_some() {
            return false;
        }

        *loss = Some(MachineAuthorityLoss {
            reason,
            trigger_error: failures
                .first()
                .cloned()
                .unwrap_or_else(|| "machine authority renewal failed".to_string()),
            served_runs,
        });
        true
    }

    pub(super) fn journal_machine_authority_loss_phase(
        &self,
        phase: &str,
        terminated_provider_process_groups: &[u32],
    ) {
        self.journal_machine_authority_loss_phase_with_detail(
            phase,
            terminated_provider_process_groups,
            None,
        );
    }

    /// The same self-stop event plus one phase-specific `detail` object.
    /// Phases that carry no extra evidence keep exactly the summary shape they
    /// already had; `detail` is additive and absent by default.
    pub(super) fn journal_machine_authority_loss_phase_with_detail(
        &self,
        phase: &str,
        terminated_provider_process_groups: &[u32],
        detail: Option<serde_json::Value>,
    ) {
        let loss = self
            .machine_authority_loss
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Some(loss) = loss else {
            return;
        };

        let reason = loss.reason;
        for target in loss.served_runs {
            let summary = serde_json::json!({
                "kind": "node_daemon_self_stop",
                "reason": reason,
                "error": loss.trigger_error,
                "phase": phase,
                "daemon_id": self.daemon_id,
                "daemon_instance_id": self.instance_id,
                "daemon_generation": target.daemon_generation,
                "terminated_provider_process_groups": terminated_provider_process_groups,
            });
            let summary = with_phase_detail(summary, detail.as_ref()).to_string();
            let stable_key = format!(
                "node-daemon-self-stop:{}:{}:{}",
                self.instance_id, target.team_run_id, phase
            );
            self.journal_node_daemon_team_run_event(
                &target.execution_space_id,
                &target.team_run_id,
                &stable_key,
                "self_stopped",
                &summary,
                reason,
            );
        }
    }

    /// Write one durable `node_daemon` TeamRunEvent through the canonical
    /// `team_run_events.jsonl` writer, retrying a bounded number of times and
    /// always falling back to stderr, which is the detached daemon's durable
    /// log. Both the self-stop phases and automatic predecessor recovery
    /// journal through this one writer.
    pub(super) fn journal_node_daemon_team_run_event(
        &self,
        execution_space_id: &str,
        team_run_id: &str,
        stable_key: &str,
        operation: &str,
        summary: &str,
        log_label: &str,
    ) {
        let event = harness_core::TeamRunEvent {
            id: String::new(),
            seq: 0,
            team_run_id: team_run_id.to_string(),
            source_kind: harness_core::TeamRunEventSourceKind::Service,
            member_run_id: None,
            delegation_run_id: None,
            entity_type: "node_daemon".to_string(),
            entity_id: self.instance_id.clone(),
            operation: operation.to_string(),
            summary: summary.to_string(),
            occurred_at: crate::daemon_support::now_string(),
        };

        let mut last_error = None;
        for attempt in 1..=SELF_STOP_EVENT_WRITE_ATTEMPTS {
            let result = self.store_for_space(execution_space_id).and_then(|store| {
                store
                    .ensure_team_run_event_next(stable_key, event.clone())
                    .map(|_| ())
                    .map_err(CliError::Store)
            });
            match result {
                Ok(()) => {
                    eprintln!("[node-daemon] {log_label}: {summary}");
                    last_error = None;
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    if attempt < SELF_STOP_EVENT_WRITE_ATTEMPTS {
                        std::thread::sleep(SELF_STOP_EVENT_WRITE_BACKOFF);
                    }
                }
            }
        }
        if let Some(error) = last_error {
            eprintln!(
                "[node-daemon] NODE_DAEMON_EVENT_WRITE_FAILED: operation={operation}; attempts={SELF_STOP_EVENT_WRITE_ATTEMPTS}; execution_space_id={execution_space_id}; team_run_id={team_run_id}; error={error}; event={summary}"
            );
        }
    }
}

/// Attach one phase-specific `detail` object to a self-stop summary.
///
/// Kept out of the `json!` literal on purpose: the literal itself is untouched
/// by this change, so a sibling slice editing its fields rebases cleanly.
fn with_phase_detail(
    mut summary: serde_json::Value,
    detail: Option<&serde_json::Value>,
) -> serde_json::Value {
    if let (Some(object), Some(detail)) = (summary.as_object_mut(), detail) {
        object.insert("detail".to_string(), detail.clone());
    }
    summary
}
