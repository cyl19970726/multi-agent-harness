use super::*;

impl MultiTeamDaemon {
    /// Stop every machine-owned runtime before releasing this daemon generation.
    pub(super) fn graceful_shutdown(&self) -> CliResult<()> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some((cooperative_ms, forced_ms)) = self.drain_timeout_override_ms {
            return self.graceful_shutdown_with_deadlines(
                Duration::from_millis(cooperative_ms),
                Duration::from_millis(forced_ms),
            );
        }
        self.graceful_shutdown_with_deadlines(
            SUPERVISOR_DRAIN_TIMEOUT,
            FORCED_PROCESS_GROUP_DRAIN_TIMEOUT,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn graceful_shutdown_with_deadline(
        &self,
        cooperative_timeout: Duration,
    ) -> CliResult<()> {
        self.graceful_shutdown_with_deadlines(cooperative_timeout, Duration::from_secs(5))
    }

    pub(super) fn graceful_shutdown_with_deadlines(
        &self,
        cooperative_timeout: Duration,
        forced_timeout: Duration,
    ) -> CliResult<()> {
        eprintln!("[node-daemon] graceful shutdown initiated");
        self.journal_machine_authority_loss_phase("shutdown_initiated", &[]);
        self.session_runtimes
            .lock()
            .map_err(|_| {
                CliError::Usage(
                    "NODE_DAEMON_DRAIN_INCOMPLETE: provider runtime registry poisoned".into(),
                )
            })?
            .clear();

        let contexts: Vec<MultiTeamContext> = {
            let mut guard = self
                .contexts
                .lock()
                .map_err(|error| CliError::Usage(format!("context lock poisoned: {error}")))?;
            std::mem::take(&mut *guard)
        };
        if !contexts.is_empty() {
            eprintln!(
                "[node-daemon] waiting for {} run(s) to finish...",
                contexts.len()
            );
        }
        for context in &contexts {
            context.heartbeat_valid.store(false, Ordering::Release);
        }

        let deadline = Instant::now() + cooperative_timeout;
        let mut failures = Vec::new();
        let mut unfinished = Vec::new();
        for context in contexts {
            let Some(thread) = context.thread else {
                continue;
            };
            loop {
                if thread.is_finished() {
                    observe_join(
                        &mut failures,
                        &context.execution_space_id,
                        &context.run_id,
                        thread.join(),
                        "during shutdown",
                    );
                    break;
                }
                if Instant::now() >= deadline {
                    unfinished.push((context.execution_space_id, context.run_id, thread));
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        // A process exiting with live Rust threads skips those threads' Drop
        // guards. Terminate only PGIDs registered by providers in this exact
        // daemon process, then let their Supervisor threads observe EOF.
        let termination = harness_runtime_host::terminate_registered_process_groups();
        let mut terminated_process_groups = termination.pids.clone();
        if !termination.pids.is_empty() {
            eprintln!(
                "[node-daemon] terminated {} owned provider process group(s): {:?}",
                termination.pids.len(),
                termination.pids
            );
        }
        if !termination.signal_failures.is_empty() {
            failures.push(format!(
                "owned provider process-group signals failed: {:?}",
                termination.signal_failures
            ));
        }
        let forced_deadline = Instant::now() + forced_timeout;
        for (space_id, run_id, thread) in unfinished {
            while !thread.is_finished() && Instant::now() < forced_deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if thread.is_finished() {
                observe_join(
                    &mut failures,
                    &space_id,
                    &run_id,
                    thread.join(),
                    "after owned provider process-group termination",
                );
            } else {
                failures.push(format!(
                    "{space_id}/{run_id} remained live after owned provider process-group termination"
                ));
            }
        }
        // Collect groups that raced with the first drain. Admission remained
        // closed throughout the forced join window, so each late registration
        // was synchronously killed and is reported here.
        let late_termination = harness_runtime_host::terminate_registered_process_groups();
        terminated_process_groups.extend(late_termination.pids.iter().copied());
        terminated_process_groups.sort_unstable();
        terminated_process_groups.dedup();
        if !late_termination.pids.is_empty() {
            eprintln!(
                "[node-daemon] terminated {} late owned provider process group(s): {:?}",
                late_termination.pids.len(),
                late_termination.pids
            );
        }
        if !late_termination.signal_failures.is_empty() {
            failures.push(format!(
                "late owned provider process-group signals failed: {:?}",
                late_termination.signal_failures
            ));
        }
        self.journal_machine_authority_loss_phase(
            "process_groups_terminated",
            &terminated_process_groups,
        );
        if failures.is_empty() {
            // Every old Supervisor has joined and the final closed-admission
            // drain is empty of failures. A future daemon generation in this
            // same process may now register its own groups.
            if let Err(error) = harness_runtime_host::complete_registered_process_group_shutdown() {
                failures.push(format!(
                    "owned provider process-group admission cannot reopen: {error}"
                ));
            }
        }
        if failures.is_empty() {
            self.journal_machine_authority_loss_phase(
                "shutdown_complete",
                &terminated_process_groups,
            );
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "NODE_DAEMON_DRAIN_INCOMPLETE: {}",
                failures.join("; ")
            )))
        }
    }
}

fn observe_join(
    failures: &mut Vec<String>,
    space_id: &str,
    run_id: &str,
    result: std::thread::Result<CliResult<TeamRunDriveOutcome>>,
    phase: &str,
) {
    match result {
        Ok(Ok(_)) => {}
        // A Supervisor commonly observes the intentional heartbeat loss and
        // returns its typed runtime error while Stop is draining. Its owning
        // path already records recovery state; process release is the daemon
        // shutdown postcondition here.
        Ok(Err(error)) => {
            eprintln!("[node-daemon] {space_id}/{run_id} supervisor stopped {phase}: {error}")
        }
        Err(_) => failures.push(format!("{space_id}/{run_id} supervisor panicked {phase}")),
    }
}
