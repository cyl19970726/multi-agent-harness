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
const MACHINE_AUTHORITY_LOST_REASON: &str = "NODE_DAEMON_MACHINE_AUTHORITY_LOST";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ServedTeamRun {
    execution_space_id: String,
    team_run_id: String,
    daemon_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineAuthorityLoss {
    trigger_error: String,
    served_runs: Vec<ServedTeamRun>,
}

impl MultiTeamDaemon {
    /// Preserve only the first renewal failure: later drain errors are useful
    /// diagnostics, but must not overwrite the trigger that caused self-stop.
    pub(super) fn capture_machine_authority_loss(&self, failures: &[String]) -> bool {
        let mut loss = self
            .machine_authority_loss
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if loss.is_some() {
            return false;
        }

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

        *loss = Some(MachineAuthorityLoss {
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
        let loss = self
            .machine_authority_loss
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Some(loss) = loss else {
            return;
        };

        for target in loss.served_runs {
            let summary = serde_json::json!({
                "kind": "node_daemon_self_stop",
                "reason": MACHINE_AUTHORITY_LOST_REASON,
                "error": loss.trigger_error,
                "phase": phase,
                "daemon_id": self.daemon_id,
                "daemon_instance_id": self.instance_id,
                "daemon_generation": target.daemon_generation,
                "terminated_provider_process_groups": terminated_provider_process_groups,
            })
            .to_string();
            let stable_key = format!(
                "node-daemon-self-stop:{}:{}:{}",
                self.instance_id, target.team_run_id, phase
            );
            let event = harness_core::TeamRunEvent {
                id: String::new(),
                seq: 0,
                team_run_id: target.team_run_id.clone(),
                source_kind: harness_core::TeamRunEventSourceKind::Service,
                member_run_id: None,
                delegation_run_id: None,
                entity_type: "node_daemon".to_string(),
                entity_id: self.instance_id.clone(),
                operation: "self_stopped".to_string(),
                summary: summary.clone(),
                occurred_at: crate::daemon_support::now_string(),
            };

            let mut last_error = None;
            for attempt in 1..=SELF_STOP_EVENT_WRITE_ATTEMPTS {
                let result = self
                    .store_for_space(&target.execution_space_id)
                    .and_then(|store| {
                        store
                            .ensure_team_run_event_next(&stable_key, event.clone())
                            .map(|_| ())
                            .map_err(CliError::Store)
                    });
                match result {
                    Ok(()) => {
                        eprintln!("[node-daemon] {MACHINE_AUTHORITY_LOST_REASON}: {summary}");
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
                    "[node-daemon] NODE_DAEMON_SELF_STOP_EVENT_WRITE_FAILED: attempts={SELF_STOP_EVENT_WRITE_ATTEMPTS}; execution_space_id={}; team_run_id={}; error={error}; event={summary}",
                    target.execution_space_id, target.team_run_id
                );
            }
        }
    }
}
