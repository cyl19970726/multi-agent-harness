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
                occurred_at: crate::now_string(),
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

#[cfg(test)]
mod tests {
    use super::super::adoption_tests::adoption_fixture;
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn authority_renewal_failure_is_returned_by_the_team_run_events_reader() {
        let fixture = adoption_fixture("self-stop-event");
        let daemon = &fixture.daemon;
        fixture
            .store
            .insert_execution_node(&harness_core::ExecutionNode {
                id: daemon.node_id.clone(),
                display_name: "Self-stop test Node".to_string(),
                status: harness_core::ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".to_string(),
                updated_at: "unix-ms:1".to_string(),
            })
            .expect("insert self-stop test Node");
        fixture
            .store
            .register_node_project(
                &harness_core::NodeProjectRegistration {
                    node_id: daemon.node_id.clone(),
                    execution_space_id: fixture.execution_space_id.clone(),
                    project_binding_id: "unit-test-project".to_string(),
                    status: harness_core::NodeProjectRegistrationStatus::Active,
                    created_at: "unix-ms:1".to_string(),
                    updated_at: "unix-ms:1".to_string(),
                },
                &fixture.execution_space_id,
            )
            .expect("register self-stop test project");
        let space = daemon
            .registered_spaces()
            .expect("list self-stop test Spaces")
            .into_iter()
            .find_map(|(space, _)| (space.id == fixture.execution_space_id).then_some(space))
            .expect("self-stop test Space");
        let owned_lease = daemon
            .ensure_node_authority(&space, &fixture.store)
            .expect("acquire owned daemon lease");
        daemon
            .contexts
            .lock()
            .expect("lock managed contexts")
            .push(MultiTeamContext {
                execution_space_id: fixture.execution_space_id.clone(),
                project_binding_id: "unit-test-project".to_string(),
                run_id: fixture.run_id.clone(),
                daemon_generation: owned_lease.generation,
                supervisor_id: "self-stop-supervisor".to_string(),
                supervisor_generation: 1,
                heartbeat_valid: Arc::new(AtomicBool::new(true)),
                thread: None,
                started_at: Instant::now(),
                serving_status: Arc::new(Mutex::new("running".to_string())),
            });

        daemon
            .supersede_node_authority_for_test(&fixture.store)
            .expect("replace lease with a successor that fences renewal");

        let error = daemon
            .refresh_held_node_authorities()
            .expect_err("heartbeat renewal must lose exact machine authority");
        assert!(error
            .to_string()
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"));
        daemon.journal_machine_authority_loss_phase("process_groups_terminated", &[4242]);

        let events = fixture
            .store
            .current_team_run_events(&fixture.run_id)
            .expect("read TeamRun events after self-stop");
        let self_stop = events
            .iter()
            .filter(|event| {
                event.source_kind == harness_core::TeamRunEventSourceKind::Service
                    && event.entity_type == "node_daemon"
                    && event.operation == "self_stopped"
            })
            .map(|event| {
                serde_json::from_str::<serde_json::Value>(&event.summary)
                    .expect("self-stop summary is structured JSON")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            self_stop.len(),
            3,
            "renewal, lease-loss, and process-group phases"
        );
        assert_eq!(self_stop[0]["kind"], "node_daemon_self_stop");
        assert_eq!(self_stop[0]["reason"], "NODE_DAEMON_MACHINE_AUTHORITY_LOST");
        assert_eq!(self_stop[0]["phase"], "renewal_failed");
        assert_eq!(self_stop[1]["phase"], "lease_lost");
        assert_eq!(self_stop[1]["daemon_id"], daemon.daemon_id);
        assert_eq!(self_stop[1]["daemon_instance_id"], daemon.instance_id);
        assert_eq!(self_stop[1]["daemon_generation"], owned_lease.generation);
        assert!(self_stop[1]["error"]
            .as_str()
            .is_some_and(|error| error.contains("exact Node authority moved")));
        assert_eq!(
            self_stop[1]["terminated_provider_process_groups"],
            serde_json::json!([])
        );
        assert_eq!(self_stop[2]["phase"], "process_groups_terminated");
        assert_eq!(
            self_stop[2]["terminated_provider_process_groups"],
            serde_json::json!([4242])
        );
    }
}
