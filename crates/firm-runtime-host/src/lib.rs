//! Provider-neutral process transport used by runtime implementations.
//!
//! This crate owns process-group isolation, bounded NDJSON collection, and
//! stderr draining. Provider command construction and event interpretation stay
//! in their provider packages.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
}
