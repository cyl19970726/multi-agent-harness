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

#[derive(Debug)]
struct OwnedProcessGroups {
    accepting: bool,
    next_token: u64,
    groups: HashMap<u32, u64>,
    shutdown_signaled: Vec<u32>,
    shutdown_signal_failures: Vec<(u32, i32)>,
}

impl Default for OwnedProcessGroups {
    fn default() -> Self {
        Self {
            accepting: true,
            next_token: 0,
            groups: HashMap::new(),
            shutdown_signaled: Vec::new(),
            shutdown_signal_failures: Vec::new(),
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
/// Normal Close/Drop removes the pid; the NodeDaemon may drain the remaining
/// exact registrations before its process exits and skips Rust destructors in
/// a still-running Supervisor thread.
#[derive(Debug)]
pub struct OwnedProcessGroupRegistration {
    pid: u32,
    token: u64,
    registered: bool,
}

impl OwnedProcessGroupRegistration {
    pub fn new(pid: u32) -> Self {
        let mut groups = owned_process_groups()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        groups.next_token = groups.next_token.wrapping_add(1).max(1);
        let token = groups.next_token;
        let registered = groups.accepting;
        if registered {
            groups.groups.insert(pid, token);
        } else {
            // Shutdown closes provider-spawn admission under the same mutex as
            // registration. A Supervisor that raced past its final lease
            // check cannot leave a late process group outside the drain.
            groups.shutdown_signaled.push(pid);
            if let Some(errno) = signal_process_group(pid) {
                groups.shutdown_signal_failures.push((pid, errno));
            }
        }
        Self {
            pid,
            token,
            registered,
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

    /// Reap a child and unregister its exact token as one linearized action.
    pub fn wait_and_release(
        &mut self,
        child: &mut std::process::Child,
    ) -> std::io::Result<std::process::ExitStatus> {
        let mut groups = owned_process_groups()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let status = child.wait()?;
        self.release_locked(&mut groups);
        Ok(status)
    }

    /// Terminate and reap this exact registered process group without exposing
    /// a reap-to-unregister pid-reuse window to daemon shutdown.
    pub fn kill_and_reap(&mut self, child: &mut std::process::Child) {
        let mut groups = owned_process_groups()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        self.release_locked(&mut groups);
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
        self.release();
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
    let registered = std::mem::take(&mut groups.groups);
    for pid in registered.keys().copied() {
        groups.shutdown_signaled.push(pid);
        if let Some(errno) = signal_process_group(pid) {
            groups.shutdown_signal_failures.push((pid, errno));
        }
    }
    let mut pids = std::mem::take(&mut groups.shutdown_signaled);
    pids.sort_unstable();
    pids.dedup();
    ProcessGroupTermination {
        pids,
        signal_failures: std::mem::take(&mut groups.shutdown_signal_failures),
    }
}

/// Reopen registration only after every old Supervisor thread has joined and
/// a final closed-admission drain observed no signal failure.
pub fn complete_registered_process_group_shutdown() {
    let mut groups = owned_process_groups()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    debug_assert!(groups.groups.is_empty());
    debug_assert!(groups.shutdown_signaled.is_empty());
    debug_assert!(groups.shutdown_signal_failures.is_empty());
    groups.accepting = true;
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
        let mut registration = OwnedProcessGroupRegistration::new(pid);

        assert_eq!(
            terminate_registered_process_groups(),
            ProcessGroupTermination {
                pids: vec![pid],
                signal_failures: Vec::new(),
            }
        );
        assert!(child
            .wait()
            .expect("reap owned provider group")
            .code()
            .is_none());
        assert!(terminate_registered_process_groups().pids.is_empty());
        registration.release();
        complete_registered_process_group_shutdown();
    }

    #[test]
    fn stale_guard_cannot_remove_a_reused_pid_registration() {
        use std::os::unix::process::CommandExt;

        let _test_lock = registry_test_lock();

        let mut command = shell("sleep 30");
        command.process_group(0);
        let mut child = command.spawn().expect("spawn reused owned group");
        let pid = child.id();
        let mut stale = OwnedProcessGroupRegistration::new(pid);
        let mut current = OwnedProcessGroupRegistration::new(pid);
        stale.release();

        assert_eq!(terminate_registered_process_groups().pids, vec![pid]);
        let _ = child.wait().expect("reap reused owned group");
        current.release();
        complete_registered_process_group_shutdown();
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
        let _late = OwnedProcessGroupRegistration::new(pid);
        let _ = child.wait().expect("reap late owned group");

        assert_eq!(terminate_registered_process_groups().pids, vec![pid]);
        complete_registered_process_group_shutdown();
    }

    #[test]
    fn shutdown_cannot_enter_between_reap_and_exact_unregister() {
        let _test_lock = registry_test_lock();
        let registration = OwnedProcessGroupRegistration::new(999_999);
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
        complete_registered_process_group_shutdown();
    }
}
