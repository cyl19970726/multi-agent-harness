use super::*;

impl MultiTeamDaemon {
    /// Stop every machine-owned runtime before releasing this daemon generation.
    pub(super) fn graceful_shutdown(&self) -> CliResult<()> {
        self.graceful_shutdown_with_deadline(Duration::from_secs(30))
    }

    fn graceful_shutdown_with_deadline(&self, cooperative_timeout: Duration) -> CliResult<()> {
        self.graceful_shutdown_with_deadlines(cooperative_timeout, Duration::from_secs(5))
    }

    fn graceful_shutdown_with_deadlines(
        &self,
        cooperative_timeout: Duration,
        forced_timeout: Duration,
    ) -> CliResult<()> {
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
    use std::sync::OnceLock;

    fn shutdown_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn shutdown_force_reaps_an_owned_group_before_returning() {
        let _test_lock = shutdown_test_lock();
        let heartbeat = Arc::new(AtomicBool::new(true));
        let thread_heartbeat = Arc::clone(&heartbeat);
        let (pid_tx, pid_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || -> CliResult<()> {
            let mut command = std::process::Command::new("sh");
            command.arg("-c").arg("sleep 30").process_group(0);
            let mut child = command.spawn()?;
            let pid = child.id();
            let mut registration =
                harness_runtime_host::OwnedProcessGroupRegistration::new(&mut child)
                    .expect("register shutdown test process group");
            pid_tx.send(pid).expect("publish owned process group");
            let status = registration.kill_and_reap(&mut child)?;
            assert!(status.is_some(), "shutdown child must be terminal-reaped");
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
            live_provider_activity_endpoint: Arc::new(Mutex::new(HashMap::new())),
            max_concurrency: 1,
            idle_timeout_secs: 1,
            scan_interval: Duration::from_secs(1),
            stop_requested: Arc::new(AtomicBool::new(false)),
            authority_shutdown: Arc::new(AtomicBool::new(false)),
            authority_lost: AtomicBool::new(false),
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

    #[test]
    fn shutdown_returns_drain_incomplete_when_provider_thread_does_not_converge() {
        let _test_lock = shutdown_test_lock();
        let heartbeat = Arc::new(AtomicBool::new(true));
        let release = Arc::new(AtomicBool::new(false));
        let thread_release = Arc::clone(&release);
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || -> CliResult<()> {
            while !thread_release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
            }
            finished_tx.send(()).expect("publish provider thread exit");
            Ok(())
        });
        let daemon = MultiTeamDaemon {
            firm_home: std::env::temp_dir(),
            node_id: "shutdown-timeout-test-node".into(),
            daemon_id: "node-daemon:shutdown-timeout-test-node".into(),
            instance_id: "shutdown-timeout-test-instance".into(),
            contexts: Mutex::new(vec![MultiTeamContext {
                execution_space_id: "shutdown-timeout-space".into(),
                project_binding_id: "shutdown-timeout-project".into(),
                run_id: "shutdown-timeout-run".into(),
                daemon_generation: 1,
                supervisor_id: "shutdown-timeout-supervisor".into(),
                supervisor_generation: 1,
                heartbeat_valid: heartbeat,
                thread: Some(thread),
                started_at: Instant::now(),
            }]),
            supervisor_start_gate: Mutex::new(()),
            session_runtimes: Mutex::new(HashMap::new()),
            live_provider_activity_endpoint: Arc::new(Mutex::new(HashMap::new())),
            max_concurrency: 1,
            idle_timeout_secs: 1,
            scan_interval: Duration::from_secs(1),
            stop_requested: Arc::new(AtomicBool::new(false)),
            authority_shutdown: Arc::new(AtomicBool::new(false)),
            authority_lost: AtomicBool::new(false),
            control_worker_failed: AtomicBool::new(false),
            recovery_blocked_runs: Mutex::new(HashSet::new()),
            lease_ttl_override_ms: None,
        };

        let started = Instant::now();
        let error = daemon
            .graceful_shutdown_with_deadlines(Duration::from_millis(10), Duration::from_millis(20))
            .expect_err("unfinished provider thread must fail closed");
        assert!(error.to_string().contains("NODE_DAEMON_DRAIN_INCOMPLETE"));
        assert!(started.elapsed() < Duration::from_secs(1));
        release.store(true, Ordering::Release);
        finished_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("detached test thread exits");
        harness_runtime_host::complete_registered_process_group_shutdown()
            .expect("reset process-group admission after timeout test");
    }
}
