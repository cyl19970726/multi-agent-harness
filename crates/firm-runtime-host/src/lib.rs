//! Provider-neutral process transport used by runtime implementations.
//!
//! This crate owns process-group isolation, bounded NDJSON collection, and
//! stderr draining. Provider command construction and event interpretation stay
//! in their provider packages.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct NdjsonRun {
    pub process_success: bool,
    pub events: Vec<serde_json::Value>,
    pub stderr: String,
    pub timed_out: bool,
    pub wall_timed_out: bool,
    pub dropped_stdout_lines: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessTransportError {
    #[error("failed to spawn {context}: {source}")]
    Spawn {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{context} {stream} not available")]
    MissingStream {
        context: String,
        stream: &'static str,
    },
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ProcessGroupRegistrationError {
    #[error(
        "PROVIDER_PROCESS_GROUP_ADMISSION_CLOSED: process group {pid} was spawned after daemon shutdown admission closed (signal_errno={signal_errno:?}, reap_failed={reap_failed}, reap_errno={reap_errno:?}, reap_timed_out={reap_timed_out})"
    )]
    AdmissionClosed {
        pid: u32,
        signal_errno: Option<i32>,
        reap_failed: bool,
        reap_errno: Option<i32>,
        reap_timed_out: bool,
    },
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error(
    "provider process-group shutdown has residual state (registered={registered_pids:?}, pending_signals={pending_signal_pids:?}, signal_failures={signal_failures:?}, reap_failures={reap_failures:?}, reap_timeouts={reap_timeout_pids:?})"
)]
pub struct ProcessGroupShutdownCompletionError {
    pub registered_pids: Vec<u32>,
    pub pending_signal_pids: Vec<u32>,
    pub signal_failures: Vec<(u32, i32)>,
    pub reap_failures: Vec<(u32, Option<i32>)>,
    pub reap_timeout_pids: Vec<u32>,
}

#[derive(Debug)]
struct OwnedProcessGroups {
    accepting: bool,
    next_token: u64,
    groups: HashMap<u32, u64>,
    shutdown_signaled: Vec<u32>,
    shutdown_signal_failures: Vec<(u32, i32)>,
    shutdown_reap_failures: Vec<(u32, Option<i32>)>,
    shutdown_reap_timeouts: Vec<u32>,
}

impl Default for OwnedProcessGroups {
    fn default() -> Self {
        Self {
            accepting: true,
            next_token: 0,
            groups: HashMap::new(),
            shutdown_signaled: Vec::new(),
            shutdown_signal_failures: Vec::new(),
            shutdown_reap_failures: Vec::new(),
            shutdown_reap_timeouts: Vec::new(),
        }
    }
}

fn owned_process_groups() -> &'static Mutex<OwnedProcessGroups> {
    static GROUPS: OnceLock<Mutex<OwnedProcessGroups>> = OnceLock::new();
    GROUPS.get_or_init(|| Mutex::new(OwnedProcessGroups::default()))
}

fn signal_process_group(pid: u32) -> Option<i32> {
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
        if result == -1 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if errno != libc::ESRCH {
                return Some(errno);
            }
        }
    }
    None
}

/// Process-local registration for a child that is the leader of its own
/// process group. Provider transports keep this guard next to their `Child`.
/// A proven terminal reap removes the pid; Drop without that proof leaves the
/// exact registration for NodeDaemon drain before process exit skips Rust
/// destructors in a still-running Supervisor thread.
#[derive(Debug)]
pub struct OwnedProcessGroupRegistration {
    pid: u32,
    token: u64,
    registered: bool,
}

impl OwnedProcessGroupRegistration {
    pub fn new(child: &mut std::process::Child) -> Result<Self, ProcessGroupRegistrationError> {
        let pid = child.id();
        let signal_errno = {
            let mut groups = owned_process_groups()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            groups.next_token = groups.next_token.wrapping_add(1).max(1);
            let token = groups.next_token;
            if groups.accepting {
                groups.groups.insert(pid, token);
                return Ok(Self {
                    pid,
                    token,
                    registered: true,
                });
            }
            // Shutdown closes provider-spawn admission under the same mutex as
            // registration. A Supervisor that raced past its final lease
            // check is made visible to completion before the lock is released.
            groups.shutdown_signaled.push(pid);
            let signal_errno = signal_process_group(pid);
            if let Some(errno) = signal_errno {
                groups.shutdown_signal_failures.push((pid, errno));
            }
            signal_errno
        };
        let _ = child.kill();
        let deadline = Instant::now() + Duration::from_secs(2);
        let (reap_failed, reap_errno, reap_timed_out) = loop {
            match child.try_wait() {
                Ok(Some(_)) => break (false, None, false),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => break (false, None, true),
                Err(error) => break (true, error.raw_os_error(), false),
            }
        };
        {
            let mut groups = owned_process_groups()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if reap_timed_out {
                groups.shutdown_reap_timeouts.push(pid);
            } else if reap_failed {
                groups.shutdown_reap_failures.push((pid, reap_errno));
            } else if signal_errno.is_none() {
                groups.shutdown_signaled.retain(|pending| *pending != pid);
            }
        }

        Err(ProcessGroupRegistrationError::AdmissionClosed {
            pid,
            signal_errno,
            reap_failed,
            reap_errno,
            reap_timed_out,
        })
    }

    #[cfg(test)]
    fn register_pid_for_test(pid: u32) -> Self {
        let mut groups = owned_process_groups()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert!(
            groups.accepting,
            "test registration requires open admission"
        );
        groups.next_token = groups.next_token.wrapping_add(1).max(1);
        let token = groups.next_token;
        groups.groups.insert(pid, token);
        Self {
            pid,
            token,
            registered: true,
        }
    }

    pub fn release(&mut self) {
        if self.registered {
            let mut groups = owned_process_groups()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if groups.groups.get(&self.pid) == Some(&self.token) {
                groups.groups.remove(&self.pid);
            }
            self.registered = false;
        }
    }

    /// Inspect and, when terminal, reap the child while holding the same
    /// registry mutex used by daemon shutdown. The pid cannot become reusable
    /// between kernel reap and exact-token removal.
    pub fn try_wait_and_release(
        &mut self,
        child: &mut std::process::Child,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.try_reap_and_release_with(|| child.try_wait())
    }

    fn try_reap_and_release_with<T, E>(
        &mut self,
        operation: impl FnOnce() -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        let mut groups = owned_process_groups()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let status = operation()?;
        if status.is_some() {
            self.release_locked(&mut groups);
        }
        Ok(status)
    }

    /// Terminate and reap this exact registered process group without exposing
    /// a reap-to-unregister pid-reuse window to daemon shutdown. The registry
    /// lock is held only for exact-token checks, signalling, and each
    /// non-blocking reap observation. Poll waits happen outside the lock; on
    /// timeout the registration remains so daemon shutdown can observe and
    /// fail closed.
    pub fn kill_and_reap(
        &mut self,
        child: &mut std::process::Child,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        if let Some(status) = self.try_wait_and_release(child)? {
            return Ok(Some(status));
        }
        let signal_errno = {
            let groups = owned_process_groups()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if groups.groups.get(&self.pid) == Some(&self.token) {
                signal_process_group(self.pid)
            } else {
                None
            }
        };
        self.finish_kill_and_reap_after_signal(child, signal_errno)
    }

    fn finish_kill_and_reap_after_signal(
        &mut self,
        child: &mut std::process::Child,
        signal_errno: Option<i32>,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        let _ = child.kill();
        if let Some(errno) = signal_errno {
            // Reaping the immediate child cannot prove that the process-group
            // descendants received SIGKILL. Preserve a separate completion
            // diagnostic even if the best-effort child reap succeeds.
            let mut groups = owned_process_groups()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if groups.groups.get(&self.pid) == Some(&self.token) {
                let failure = (self.pid, errno);
                if !groups.shutdown_signal_failures.contains(&failure) {
                    groups.shutdown_signal_failures.push(failure);
                }
            }
            drop(groups);
            // Terminal child proof still removes the exact pid/token so a
            // later drain cannot signal a foreign process after PID reuse.
            // The signal diagnostic independently keeps completion closed.
            let _ = self.bounded_reap_and_release_with(Duration::from_secs(2), || child.try_wait());
            return Err(std::io::Error::from_raw_os_error(errno));
        }
        self.bounded_reap_and_release_with(Duration::from_secs(2), || child.try_wait())
    }

    fn bounded_reap_and_release_with<T, E>(
        &mut self,
        timeout: Duration,
        operation: impl FnMut() -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        self.bounded_reap_and_release_with_interval(timeout, Duration::from_millis(10), operation)
    }

    fn bounded_reap_and_release_with_interval<T, E>(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
        mut operation: impl FnMut() -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut groups = owned_process_groups()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if let Some(status) = operation()? {
                    self.release_locked(&mut groups);
                    return Ok(Some(status));
                }
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(poll_interval);
        }
    }

    fn release_locked(&mut self, groups: &mut OwnedProcessGroups) {
        if self.registered {
            if groups.groups.get(&self.pid) == Some(&self.token) {
                groups.groups.remove(&self.pid);
            }
            self.registered = false;
        }
    }
}

impl Drop for OwnedProcessGroupRegistration {
    fn drop(&mut self) {
        // Absence of a kernel reap is not evidence that the pid is safe to
        // unregister. Provider owners explicitly use the atomic reap methods;
        // otherwise the daemon registry retains the group for final cleanup.
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ProcessGroupTermination {
    pub pids: Vec<u32>,
    pub signal_failures: Vec<(u32, i32)>,
}

/// Close provider process-group admission and terminate only groups registered
/// by transports in this exact process. Registration and signalling share one
/// mutex: a late spawn is killed synchronously instead of escaping the drain.
pub fn terminate_registered_process_groups() -> ProcessGroupTermination {
    let mut groups = owned_process_groups()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    groups.accepting = false;
    // Signalling is not terminal proof. Keep every exact pid/token ownership
    // entry until its provider guard observes and reaps the child in the same
    // critical section that removes the token. Late-admission children use the
    // separate pending list until their shared constructor proves reap.
    let mut pids = groups.groups.keys().copied().collect::<Vec<_>>();
    pids.extend(groups.shutdown_signaled.iter().copied());
    pids.sort_unstable();
    pids.dedup();
    let mut signal_failures = Vec::new();
    for pid in pids.iter().copied() {
        if let Some(errno) = signal_process_group(pid) {
            let failure = (pid, errno);
            if !groups.shutdown_signal_failures.contains(&failure) {
                groups.shutdown_signal_failures.push(failure);
            }
            signal_failures.push(failure);
        }
    }
    ProcessGroupTermination {
        pids,
        signal_failures,
    }
}

/// Reopen registration only after every old Supervisor thread has joined and
/// a final closed-admission drain observed no signal failure.
pub fn complete_registered_process_group_shutdown(
) -> Result<(), ProcessGroupShutdownCompletionError> {
    let mut groups = owned_process_groups()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    groups.accepting = false;
    if !groups.groups.is_empty()
        || !groups.shutdown_signaled.is_empty()
        || !groups.shutdown_signal_failures.is_empty()
        || !groups.shutdown_reap_failures.is_empty()
        || !groups.shutdown_reap_timeouts.is_empty()
    {
        let mut registered_pids = groups.groups.keys().copied().collect::<Vec<_>>();
        registered_pids.sort_unstable();
        let mut pending_signal_pids = groups.shutdown_signaled.clone();
        pending_signal_pids.sort_unstable();
        pending_signal_pids.dedup();
        return Err(ProcessGroupShutdownCompletionError {
            registered_pids,
            pending_signal_pids,
            signal_failures: groups.shutdown_signal_failures.clone(),
            reap_failures: groups.shutdown_reap_failures.clone(),
            reap_timeout_pids: groups.shutdown_reap_timeouts.clone(),
        });
    }
    groups.accepting = true;
    Ok(())
}

/// Kill the child process group, falling back to the immediate child.
pub fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn run_ndjson_child(
    mut command: Command,
    idle_timeout: Duration,
    wall_timeout: Option<Duration>,
    context: &str,
) -> Result<NdjsonRun, ProcessTransportError> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| ProcessTransportError::Spawn {
            context: context.to_owned(),
            source,
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProcessTransportError::MissingStream {
            context: context.to_owned(),
            stream: "stdout",
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProcessTransportError::MissingStream {
            context: context.to_owned(),
            stream: "stderr",
        })?;

    let start = Instant::now();
    let activity = Arc::new(AtomicU64::new(0));
    let reader_activity = Arc::clone(&activity);
    let stdout_handle = std::thread::spawn(move || {
        let mut events = Vec::new();
        let mut dropped = 0usize;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { continue };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            reader_activity.store(start.elapsed().as_millis() as u64, Ordering::Relaxed);
            match serde_json::from_str(line) {
                Ok(event) => events.push(event),
                Err(_) => dropped += 1,
            }
        }
        (events, dropped)
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut output);
        output
    });

    let idle_timeout = idle_timeout.max(Duration::from_millis(1));
    let wall_timeout = wall_timeout.map(|limit| limit.max(Duration::from_millis(1)));
    let mut timed_out = false;
    let mut wall_timed_out = false;
    let process_success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) => {
                if wall_timeout.is_some_and(|limit| start.elapsed() > limit) {
                    wall_timed_out = true;
                    kill_process_tree(&mut child);
                    break false;
                }
                let last_activity = Duration::from_millis(activity.load(Ordering::Relaxed));
                if start.elapsed().saturating_sub(last_activity) > idle_timeout {
                    timed_out = true;
                    kill_process_tree(&mut child);
                    break false;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break false,
        }
    };

    let (events, dropped_stdout_lines) = stdout_handle.join().unwrap_or_default();
    let mut stderr = stderr_handle.join().unwrap_or_default();
    if timed_out && stderr.is_empty() {
        stderr = format!("timeout waiting for {context}");
    } else if wall_timed_out && stderr.is_empty() {
        stderr = format!("{context} exceeded wall-clock timeout");
    }
    Ok(NdjsonRun {
        process_success,
        events,
        stderr,
        timed_out,
        wall_timed_out,
        dropped_stdout_lines,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn registry_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    #[test]
    fn collects_events_and_counts_invalid_lines() {
        let run = run_ndjson_child(
            shell("printf '{\"kind\":\"ready\"}\\nnot-json\\n'"),
            Duration::from_secs(1),
            None,
            "fixture",
        )
        .unwrap();
        assert!(run.process_success);
        assert_eq!(run.events.len(), 1);
        assert_eq!(run.dropped_stdout_lines, 1);
    }

    #[test]
    fn idle_timeout_kills_a_silent_process_tree() {
        let run = run_ndjson_child(
            shell("printf '{\"kind\":\"started\"}\\n'; sleep 10"),
            Duration::from_millis(100),
            None,
            "fixture",
        )
        .unwrap();
        assert!(!run.process_success);
        assert!(run.timed_out);
        assert_eq!(run.events.len(), 1);
    }

    #[test]
    fn wall_timeout_bounds_a_productive_process() {
        let run = run_ndjson_child(
            shell("while true; do printf '{\"kind\":\"tick\"}\\n'; sleep 0.02; done"),
            Duration::from_secs(1),
            Some(Duration::from_millis(120)),
            "fixture",
        )
        .unwrap();
        assert!(!run.process_success);
        assert!(run.wall_timed_out);
        assert!(!run.events.is_empty());
    }

    #[test]
    fn registered_provider_group_is_terminated_exactly_once() {
        use std::os::unix::process::CommandExt;

        let _test_lock = registry_test_lock();

        let mut command = shell("sleep 30");
        command.process_group(0);
        let mut child = command.spawn().expect("spawn owned provider group");
        let pid = child.id();
        let mut registration =
            OwnedProcessGroupRegistration::new(&mut child).expect("register owned provider group");

        assert_eq!(
            terminate_registered_process_groups(),
            ProcessGroupTermination {
                pids: vec![pid],
                signal_failures: Vec::new(),
            }
        );
        assert!(registration
            .kill_and_reap(&mut child)
            .expect("reap owned provider group")
            .expect("owned provider group became terminal")
            .code()
            .is_none());
        assert!(terminate_registered_process_groups().pids.is_empty());
        complete_registered_process_group_shutdown().expect("reopen process-group admission");
    }

    #[test]
    fn stale_guard_cannot_remove_a_reused_pid_registration() {
        use std::os::unix::process::CommandExt;

        let _test_lock = registry_test_lock();

        let mut command = shell("sleep 30");
        command.process_group(0);
        let mut child = command.spawn().expect("spawn reused owned group");
        let pid = child.id();
        let mut stale = OwnedProcessGroupRegistration::register_pid_for_test(pid);
        let mut current = OwnedProcessGroupRegistration::register_pid_for_test(pid);
        stale.release();

        assert_eq!(terminate_registered_process_groups().pids, vec![pid]);
        assert!(current
            .kill_and_reap(&mut child)
            .expect("reap reused owned group")
            .is_some());
        complete_registered_process_group_shutdown().expect("reopen process-group admission");
    }

    #[test]
    fn registration_after_shutdown_admission_closes_is_killed_and_observed() {
        use std::os::unix::process::CommandExt;

        let _test_lock = registry_test_lock();

        assert!(terminate_registered_process_groups().pids.is_empty());
        let mut command = shell("sleep 30");
        command.process_group(0);
        let mut child = command.spawn().expect("spawn late owned group");
        let pid = child.id();
        let error = OwnedProcessGroupRegistration::new(&mut child)
            .expect_err("late provider group admission must fail closed");
        assert_eq!(
            error,
            ProcessGroupRegistrationError::AdmissionClosed {
                pid,
                signal_errno: None,
                reap_failed: false,
                reap_errno: None,
                reap_timed_out: false,
            }
        );
        assert!(child
            .try_wait()
            .expect("observe already-reaped late owned group")
            .is_some());
        assert_eq!(unsafe { libc::kill(-(pid as libc::pid_t), 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );

        assert!(terminate_registered_process_groups().pids.is_empty());
        complete_registered_process_group_shutdown().expect("reopen process-group admission");
    }

    #[test]
    fn shutdown_cannot_enter_between_reap_and_exact_unregister() {
        let _test_lock = registry_test_lock();
        let registration = OwnedProcessGroupRegistration::register_pid_for_test(999_999);
        let (reaped_tx, reaped_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let reaper = std::thread::spawn(move || {
            let mut registration = registration;
            registration
                .try_reap_and_release_with(|| -> Result<Option<()>, ()> {
                    reaped_tx.send(()).expect("publish kernel reap boundary");
                    release_rx.recv().expect("release registry boundary");
                    Ok(Some(()))
                })
                .expect("linearized reap")
        });
        reaped_rx.recv().expect("reaper entered registry boundary");
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
        let shutdown = std::thread::spawn(move || {
            shutdown_tx
                .send(terminate_registered_process_groups())
                .expect("publish shutdown result");
        });
        assert!(matches!(
            shutdown_rx.recv_timeout(Duration::from_millis(30)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        release_tx.send(()).expect("finish exact unregister");
        assert_eq!(reaper.join().expect("join reaper"), Some(()));
        assert!(shutdown_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("shutdown finished after unregister")
            .pids
            .is_empty());
        shutdown.join().expect("join shutdown");
        complete_registered_process_group_shutdown().expect("reopen process-group admission");
    }

    #[test]
    fn shutdown_signal_without_terminal_reap_retains_exact_authority() {
        use std::os::unix::process::CommandExt;

        let _test_lock = registry_test_lock();
        let mut command = shell("sleep 30");
        command.process_group(0);
        let mut child = command.spawn().expect("spawn bounded-reap test group");
        let pid = child.id();
        let registration = OwnedProcessGroupRegistration::new(&mut child)
            .expect("register bounded-reap test group");
        let (first_poll_tx, first_poll_rx) = std::sync::mpsc::channel();
        let reaper = std::thread::spawn(move || {
            let mut registration = registration;
            let mut first_poll = Some(first_poll_tx);
            let status = registration
                .bounded_reap_and_release_with_interval(
                    Duration::from_millis(200),
                    Duration::from_millis(200),
                    || -> Result<Option<()>, ()> {
                        if let Some(sender) = first_poll.take() {
                            sender.send(()).expect("publish first reap poll");
                        }
                        Ok(None)
                    },
                )
                .expect("bounded reap result");
            (status, registration)
        });
        first_poll_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("reaper performed first poll");

        let started = Instant::now();
        assert_eq!(
            terminate_registered_process_groups(),
            ProcessGroupTermination {
                pids: vec![pid],
                signal_failures: Vec::new(),
            }
        );
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "shutdown waited for the reaper's unlocked poll interval"
        );
        let (status, mut registration) = reaper.join().expect("join bounded reaper");
        assert_eq!(status, None);
        let error = complete_registered_process_group_shutdown()
            .expect_err("Supervisor exit without terminal reap must keep admission closed");
        assert_eq!(error.registered_pids, vec![pid]);
        assert!(error.pending_signal_pids.is_empty());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if registration
                .try_wait_and_release(&mut child)
                .expect("observe real child terminal through exact guard")
                .is_some()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "signalled child did not become terminal"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        complete_registered_process_group_shutdown().expect("reopen process-group admission");
    }

    #[test]
    fn signal_failure_and_terminal_child_remove_pid_but_keep_completion_closed() {
        use std::os::unix::process::CommandExt;

        let _test_lock = registry_test_lock();
        let mut command = std::process::Command::new("sleep");
        command.arg("30").process_group(0);
        let mut child = command.spawn().expect("spawn signal-failure test group");
        let pid = child.id();
        let mut registration = OwnedProcessGroupRegistration::new(&mut child)
            .expect("register signal-failure test group");

        let error = registration
            .finish_kill_and_reap_after_signal(&mut child, Some(libc::EPERM))
            .expect_err("process-group signal failure must remain typed");
        assert_eq!(error.raw_os_error(), Some(libc::EPERM));

        let completion = complete_registered_process_group_shutdown()
            .expect_err("signal failure must keep admission closed after child reap");
        assert!(completion.registered_pids.is_empty());
        assert_eq!(completion.signal_failures, vec![(pid, libc::EPERM)]);
        assert_eq!(
            terminate_registered_process_groups(),
            ProcessGroupTermination {
                pids: Vec::new(),
                signal_failures: Vec::new(),
            },
            "a terminal-reaped pid must never be signalled again"
        );

        // The residual is intentionally unrecoverable in production. Remove
        // only the synthetic diagnostic so this process-global test registry
        // can exercise the remaining cases.
        owned_process_groups()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .shutdown_signal_failures
            .retain(|failure| *failure != (pid, libc::EPERM));
        complete_registered_process_group_shutdown().expect("reopen after test-only cleanup");
    }

    #[test]
    fn shutdown_completion_rejects_residual_registration() {
        let _test_lock = registry_test_lock();
        let mut registration = OwnedProcessGroupRegistration::register_pid_for_test(999_997);

        let error = complete_registered_process_group_shutdown()
            .expect_err("residual registration must keep admission closed");
        assert_eq!(error.registered_pids, vec![999_997]);
        assert!(error.pending_signal_pids.is_empty());
        assert!(error.signal_failures.is_empty());
        assert!(error.reap_failures.is_empty());
        assert!(error.reap_timeout_pids.is_empty());

        assert_eq!(terminate_registered_process_groups().pids, vec![999_997]);
        registration.release();
        complete_registered_process_group_shutdown().expect("reopen process-group admission");
    }
}
