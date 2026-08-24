use super::*;

impl MultiTeamDaemon {
    /// Stop every machine-owned runtime before releasing this daemon generation.
    pub(super) fn graceful_shutdown(&self) -> CliResult<()> {
        self.graceful_shutdown_with_deadline(Duration::from_secs(30))
    }

    fn graceful_shutdown_with_deadline(&self, cooperative_timeout: Duration) -> CliResult<()> {
        eprintln!("[node-daemon] graceful shutdown initiated");
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
        let forced_deadline = Instant::now() + Duration::from_secs(5);
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
        if failures.is_empty() {
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
    result: std::thread::Result<CliResult<()>>,
    phase: &str,
) {
    match result {
        Ok(Ok(())) => {}
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    #[test]
    fn shutdown_force_reaps_an_owned_group_before_returning() {
        let heartbeat = Arc::new(AtomicBool::new(true));
        let thread_heartbeat = Arc::clone(&heartbeat);
        let (pid_tx, pid_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || -> CliResult<()> {
            let mut command = std::process::Command::new("sh");
            command.arg("-c").arg("sleep 30").process_group(0);
            let mut child = command.spawn()?;
            let pid = child.id();
            let _registration = harness_runtime_host::OwnedProcessGroupRegistration::new(pid);
            pid_tx.send(pid).expect("publish owned process group");
            let _ = child.wait()?;
            assert!(!thread_heartbeat.load(Ordering::Acquire));
            Ok(())
        });
        let pid = pid_rx.recv().expect("owned provider group pid");
        let daemon = MultiTeamDaemon {
            firm_home: std::env::temp_dir(),
            node_id: "shutdown-test-node".into(),
            daemon_id: "node-daemon:shutdown-test-node".into(),
            instance_id: "shutdown-test-instance".into(),
            contexts: Mutex::new(vec![MultiTeamContext {
                execution_space_id: "shutdown-test-space".into(),
                project_binding_id: "shutdown-test-project".into(),
                run_id: "shutdown-test-run".into(),
                daemon_generation: 1,
                supervisor_id: "shutdown-test-supervisor".into(),
                supervisor_generation: 1,
                heartbeat_valid: heartbeat,
                thread: Some(thread),
                started_at: Instant::now(),
            }]),
            supervisor_start_gate: Mutex::new(()),
            session_runtimes: Mutex::new(HashMap::new()),
            live_provider_activity_endpoint: Arc::new(Mutex::new(None)),
            max_concurrency: 1,
            idle_timeout_secs: 1,
            scan_interval: Duration::from_secs(1),
            stop_requested: Arc::new(AtomicBool::new(false)),
            authority_shutdown: Arc::new(AtomicBool::new(false)),
            control_worker_failed: AtomicBool::new(false),
            recovery_blocked_runs: Mutex::new(HashSet::new()),
            lease_ttl_override_ms: None,
        };

        daemon
            .graceful_shutdown_with_deadline(Duration::from_millis(20))
            .expect("shutdown reaps exact owned provider group");
        let alive = unsafe { libc::kill(-(pid as libc::pid_t), 0) };
        assert_eq!(alive, -1, "owned provider process group survived shutdown");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
