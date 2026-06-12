//! Resident `claude` stream-json process (opt-in, additive).
//!
//! The default Claude delivery path (`run_claude_exec_delivery_real` in
//! `main.rs`) spawns a fresh `claude -p …` per turn and lets the process exit
//! to terminate the stream. That re-pays model + MCP warmup on every turn.
//!
//! This module holds a `claude` process *open across turns* by switching the
//! input contract from a one-shot `-p <prompt>` argv to
//! `--input-format stream-json`: each turn is a single user-message JSON frame
//! written to the child's stdin, and the per-turn output ends at the `result`
//! event. The child never sees stdin EOF (we keep [`ChildStdin`] alive) so it
//! stays resident; closing stdin is the explicit shutdown signal.
//!
//! Keep-alive principle (the load-bearing detail): a resident process never
//! reaches stdout/stderr EOF between turns, so we must NOT drain those pipes to
//! EOF (the default path's `read_to_string` on stderr would block forever and a
//! full stderr pipe would deadlock against a blocked stdout read). Instead:
//!   * stderr is **redirected to a file** so the OS absorbs it (no backpressure,
//!     no EOF dependency);
//!   * stdout is read **incrementally and stopped at the per-turn `result`
//!     event**, leaving the reader positioned for the next turn.
//!
//! Hot / cold hybrid: the hot path reuses a pooled child for every queued
//! message of one member; cold recovery respawns with `--resume <session_id>`
//! when the child has died (so memory carries across a crash); idle reclaim
//! drops a pool entry's stdin (clean EOF shutdown) after a max-idle duration.
//!
//! Everything here is synchronous `std::process` — no tokio — matching the rest
//! of the CLI. The module is self-contained: it takes primitive launch inputs
//! (a [`ResidentConfig`]) rather than the bin-private `LaunchSpec`/`AgentMember`
//! types so it stays unit-testable against a fake `claude` script.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

/// A single resident-turn output frame, mirroring `ClaudeStreamEvent` in
/// `main.rs` but kept local so this module has no bin-private dependency. The
/// caller maps these back into `ClaudeStreamEvent` via `payload`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResidentEvent {
    pub event_type: String,
    pub payload: serde_json::Value,
}

impl ResidentEvent {
    /// Parse one NDJSON line, or `None` to skip (blank / malformed), matching
    /// the resilient parsing of `ClaudeStreamEvent::parse_line`.
    fn parse_line(line: &str) -> Option<ResidentEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let payload = serde_json::from_str::<serde_json::Value>(trimmed).ok()?;
        let event_type = payload
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown")
            .to_string();
        Some(ResidentEvent {
            event_type,
            payload,
        })
    }

    /// `session_id` from a `system` init frame (same contract as the default
    /// path: only `type == "system"` carries the resumable id).
    fn session_id(&self) -> Option<String> {
        if self.event_type == "system" {
            self.payload
                .get("session_id")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

/// Result of one resident turn, shaped so the caller can produce the SAME
/// `(success, events, session_id, stderr)` tuple the default path returns.
#[derive(Debug)]
pub struct ResidentTurn {
    /// True when a `result` frame without an `error` field was seen.
    pub success: bool,
    /// All frames for this turn, in order.
    pub events: Vec<ResidentEvent>,
    /// The session id known after this turn (real id from a `system` frame).
    pub session_id: Option<String>,
}

/// Launch inputs for a resident, expressed in primitive terms so the module is
/// testable without the bin-private `LaunchSpec`. The caller in `main.rs`
/// translates `LaunchSpec` + `AgentMember` into this.
///
/// The fields after `binary` form the **config fingerprint** ([`Self::fingerprint`]):
/// two turns may only share a resident if their fingerprints match, because a
/// running child cannot honor a changed model / permission / cwd / tools / mcp /
/// system prompt mid-flight (those are start-time only).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResidentConfig {
    /// The command to spawn (`"claude"` in production; the fake script in tests).
    pub binary: String,
    /// `--model <model>` when set.
    pub model: Option<String>,
    /// `--effort <effort>` when set.
    pub effort: Option<String>,
    /// `--json-schema <inline>` when set (the pre-normalized JSON Schema string,
    /// mirroring the fresh `claude -p` persistent path's structured-output flag).
    pub output_schema_json: Option<String>,
    /// `--permission-mode <mode>`; always emitted (mirrors the default path).
    pub permission_mode: String,
    /// `--allowedTools <csv>` when non-empty.
    pub tools: Vec<String>,
    /// `--append-system-prompt <prompt>` when non-empty.
    pub system_prompt: String,
    /// `--mcp-config <path>` when set (caller writes the temp file).
    pub mcp_config_path: Option<String>,
    /// `--add-dir` roots (workspace first, then writable roots).
    pub add_dirs: Vec<String>,
    /// Working directory for the child.
    pub cwd: String,
    /// `--resume <id>` to attach to a prior session on (re)spawn; `None` = fresh.
    pub resume: Option<String>,
}

impl ResidentConfig {
    /// Stable fingerprint of the *start-time* invocation surface. Excludes
    /// `resume` (a resident may legitimately respawn with a new resume id after
    /// a crash without being a different config) and `binary` (the pool key
    /// already carries the member id; binary is constant in production).
    ///
    /// Used by [`ResidentPool`] (the cross-invocation seam) and tests; the v1
    /// single-turn delivery path does not key on it yet.
    #[allow(dead_code)]
    pub fn fingerprint(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("model={}", self.model.as_deref().unwrap_or("-")));
        parts.push(format!("effort={}", self.effort.as_deref().unwrap_or("-")));
        parts.push(format!(
            "schema={}",
            self.output_schema_json.as_deref().unwrap_or("-")
        ));
        parts.push(format!("perm={}", self.permission_mode));
        parts.push(format!("tools={}", self.tools.join(",")));
        parts.push(format!("sys={}", self.system_prompt));
        parts.push(format!(
            "mcp={}",
            self.mcp_config_path.as_deref().unwrap_or("-")
        ));
        parts.push(format!("add={}", self.add_dirs.join(",")));
        parts.push(format!("cwd={}", self.cwd));
        parts.join("|")
    }

    /// Build the resident argv (no prompt; the prompt is a stdin frame). Mirrors
    /// the flag-mapping block of `run_claude_exec_delivery_real`, with `-p`
    /// replaced by `--input-format stream-json`.
    fn build_command(&self, stderr_file: File) -> Command {
        let mut cmd = Command::new(&self.binary);
        // `-p` (print/headless, non-interactive) is kept so the resident still
        // runs in print mode; the prompt source is the stdin stream-json frames.
        cmd.arg("-p")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");

        if let Some(resume_id) = &self.resume {
            cmd.arg("--resume").arg(resume_id);
        }
        if !self.system_prompt.is_empty() {
            cmd.arg("--append-system-prompt").arg(&self.system_prompt);
        }
        if let Some(model) = &self.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(effort) = &self.effort {
            cmd.arg("--effort").arg(effort);
        }
        if let Some(schema) = &self.output_schema_json {
            cmd.arg("--json-schema").arg(schema);
        }
        cmd.arg("--permission-mode").arg(&self.permission_mode);
        if !self.tools.is_empty() {
            cmd.arg("--allowedTools").arg(self.tools.join(","));
        }
        if let Some(mcp_path) = &self.mcp_config_path {
            cmd.arg("--mcp-config").arg(mcp_path);
        }
        for dir in &self.add_dirs {
            cmd.arg("--add-dir").arg(dir);
        }
        cmd.current_dir(&self.cwd);

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_file));
        cmd
    }
}

/// The recorded argv (for `ProviderSession.args`) of a resident invocation,
/// the resident-mode sibling of `ClaudeAdapter::recorded_args` in `main.rs`. This is
/// recording-only and honestly reflects `--input-format stream-json` instead of
/// `-p <prompt>`.
pub fn resident_recorded_args(resume_id: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "-p".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    if let Some(id) = resume_id {
        args.push("--resume".into());
        args.push(id.into());
    }
    args
}

/// Build the per-turn user-message stdin frame for `--input-format stream-json`.
pub fn user_turn_frame(text: &str) -> String {
    let frame = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{ "type": "text", "text": text }],
        }
    });
    // Defensive: `json!` of a string can't fail to serialize.
    serde_json::to_string(&frame).unwrap_or_else(|_| "{}".to_string())
}

/// Read exactly ONE turn from a resident stdout stream: append every frame and
/// stop after the `result` frame, leaving the reader positioned for the next
/// turn. Factored out (taking `impl BufRead`) so it is unit-testable over an
/// in-memory buffer, exactly like `parse_claude_stream_json`.
///
/// Returns the frames read. A turn with no `result` before EOF returns whatever
/// frames were seen (the caller treats a missing `result` as failure).
fn read_one_turn(reader: &mut impl BufRead) -> io::Result<Vec<ResidentEvent>> {
    let mut events = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            // EOF: child exited / pipe closed mid-turn.
            break;
        }
        if let Some(event) = ResidentEvent::parse_line(&line) {
            let is_result = event.event_type == "result";
            events.push(event);
            if is_result {
                break;
            }
        }
    }
    Ok(events)
}

/// A held-open `claude` process speaking the stream-json line protocol.
pub struct ResidentClaude {
    child: Child,
    /// Held open = no stdin EOF = the child stays resident. Dropping it (or
    /// [`Self::shutdown`]) sends EOF and lets the child exit.
    stdin: Option<ChildStdin>,
    /// `Option` so [`Self::send_turn`] can move the reader into a worker thread
    /// for a timeout-bounded blocking read and move it back afterwards. Only
    /// `None` transiently inside `send_turn`; a turn that times out (the reader
    /// thread is still parked on `read_line`) kills the child and leaves this
    /// `None`, marking the resident permanently unusable so the pool evicts it.
    stdout: Option<BufReader<ChildStdout>>,
    /// The real session id, learned from the first `system` frame and updated if
    /// later frames carry one. Source of truth for `--resume` on respawn.
    session_id: Option<String>,
    /// The config this child was spawned with (for fingerprint matching by the
    /// pool; not read on the v1 single-turn path).
    #[allow(dead_code)]
    config: ResidentConfig,
    /// Last successful turn time, for idle reclaim.
    last_used: Instant,
}

impl ResidentClaude {
    /// Spawn a fresh resident with stderr redirected to `stderr_path`.
    pub fn spawn(config: ResidentConfig, stderr_path: &Path) -> io::Result<ResidentClaude> {
        let stderr_file = File::create(stderr_path)?;
        let mut cmd = config.build_command(stderr_file);
        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("resident claude stdin not available"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("resident claude stdout not available"))?;
        Ok(ResidentClaude {
            child,
            stdin: Some(stdin),
            stdout: Some(BufReader::new(stdout)),
            session_id: config.resume.clone(),
            config,
            last_used: Instant::now(),
        })
    }

    /// The session id known so far (real id only; never a synthetic fallback).
    pub fn session_id(&self) -> Option<String> {
        self.session_id.clone()
    }

    /// True if the child is still running (cheap, non-blocking). Used by the
    /// pool's crash recovery; the v1 single-turn path does not call it.
    #[allow(dead_code)]
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Send one turn and read its frames until the `result` event.
    ///
    /// `timeout` bounds the *actual* blocking read of this turn's frames. Because
    /// a resident child never closes stdout between turns, `read_line` can block
    /// forever if the child accepts the frame but never emits a `result` (model
    /// wedged, MCP stall, partial line). To bound that, the blocking read runs on
    /// a worker thread that owns the stdout reader; this call joins it with
    /// `recv_timeout(timeout)`. On expiry we kill the child (unblocking the
    /// parked thread via EOF) and return a `TimedOut` error so the pool evicts +
    /// respawns. On a dead child this likewise returns an error.
    pub fn send_turn(&mut self, user_text: &str, timeout: Duration) -> io::Result<ResidentTurn> {
        // Write the user frame + newline and flush.
        {
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| io::Error::other("resident claude stdin already closed"))?;
            let mut frame = user_turn_frame(user_text);
            frame.push('\n');
            stdin.write_all(frame.as_bytes())?;
            stdin.flush()?;
        }

        // Move the reader into a worker thread so the blocking read is bounded by
        // `recv_timeout`. A timed-out turn leaves `stdout == None` (the thread is
        // still parked); after we kill the child below the read returns EOF and
        // the thread exits, dropping the reader.
        let mut reader = self
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("resident claude stdout already closed"))?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = read_one_turn(&mut reader);
            // If the receiver timed out and went away, this just drops `reader`
            // (closing our end of the pipe); never panic on a closed channel.
            let _ = tx.send((reader, result));
        });

        let events = match rx.recv_timeout(timeout) {
            Ok((reader, result)) => {
                // Restore the reader for the next turn before propagating any
                // read error (a transport error still leaves a usable reader).
                self.stdout = Some(reader);
                result?
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // The child accepted the frame but produced no `result` in time.
                // Kill it so the parked reader thread unblocks (EOF) and so the
                // pool/caller never reuses a wedged child. `stdout` stays `None`.
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "resident claude produced no result frame within timeout",
                ));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // The reader thread dropped the sender without sending (should
                // not happen): treat as a dead transport so the pool respawns.
                return Err(io::Error::other(
                    "resident claude reader thread ended unexpectedly",
                ));
            }
        };

        // Update session id from any system frame in this turn.
        if let Some(id) = events.iter().find_map(|e| e.session_id()) {
            self.session_id = Some(id);
        }

        let saw_result = events.iter().any(|e| e.event_type == "result");
        let result_errored = events
            .iter()
            .find(|e| e.event_type == "result")
            .and_then(|e| e.payload.get("error"))
            .is_some();

        // No result but stdout hit EOF before the timeout => failed turn (the
        // child closed its pipe mid-turn). A real timeout is handled above.
        if !saw_result {
            return Ok(ResidentTurn {
                success: false,
                events,
                session_id: self.session_id.clone(),
            });
        }

        self.last_used = Instant::now();
        Ok(ResidentTurn {
            success: !result_errored,
            events,
            session_id: self.session_id.clone(),
        })
    }

    /// How long since the last successful turn (for the pool's idle reclaim).
    #[allow(dead_code)]
    fn idle_for(&self) -> Duration {
        self.last_used.elapsed()
    }

    /// Clean shutdown: drop stdin (EOF) and wait for the child to exit.
    pub fn shutdown(mut self) {
        self.shutdown_in_place();
    }

    fn shutdown_in_place(&mut self) {
        // Drop stdin first so the child sees EOF and exits on its own.
        self.stdin = None;
        // Give it a brief grace period; kill if it lingers.
        let deadline = Instant::now() + Duration::from_millis(2000);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
        }
    }
}

impl Drop for ResidentClaude {
    fn drop(&mut self) {
        // Never leak a resident child: closing stdin + reaping on drop is the
        // backstop for the pool, since AgentRuntime.pid is not persisted today.
        self.shutdown_in_place();
    }
}

/// Default max idle before a pooled resident is reclaimed.
pub const DEFAULT_MAX_IDLE: Duration = Duration::from_secs(300);

/// A pool of resident `claude` children keyed by `(member_id, config
/// fingerprint)`.
///
/// This is the cross-loop residency seam. The `resident_daemon` module hosts one
/// of these behind a Unix socket so short-lived `harness deliver` invocations
/// share warm children; the in-process single-turn delivery path still spawns a
/// one-shot resident when no daemon is running.
pub struct ResidentPool {
    children: HashMap<String, ResidentClaude>,
    max_idle: Duration,
}

/// Classification of one turn attempt, used to decide whether to respawn.
enum DriveOutcome {
    Ok(ResidentTurn),
    /// The child died (before or during the turn); eligible for resume respawn.
    DeadChild {
        last_session_id: Option<String>,
    },
    /// A transport error against a still-alive child.
    Err(io::Error),
}

impl ResidentPool {
    pub fn new() -> ResidentPool {
        ResidentPool {
            children: HashMap::new(),
            max_idle: DEFAULT_MAX_IDLE,
        }
    }

    pub fn with_max_idle(max_idle: Duration) -> ResidentPool {
        ResidentPool {
            children: HashMap::new(),
            max_idle,
        }
    }

    fn key(member_id: &str, config: &ResidentConfig) -> String {
        format!("{member_id}::{}", config.fingerprint())
    }

    /// Drive one turn for `member_id` through a pooled resident, spawning or
    /// recovering the child as needed. The returned tuple matches the default
    /// path's `(success, events, session_id)` (stderr lives in the file).
    ///
    /// Recovery rules:
    ///   * idle reclaim: a matching child idle beyond `max_idle` is dropped
    ///     (stdin EOF) before this turn and respawned.
    ///   * crash recovery: a dead child is respawned with `--resume
    ///     <session_id>` so memory carries across the crash.
    pub fn run_turn(
        &mut self,
        member_id: &str,
        config: ResidentConfig,
        stderr_path: &Path,
        user_text: &str,
        timeout: Duration,
    ) -> io::Result<ResidentTurn> {
        let key = Self::key(member_id, &config);

        // Reclaim an idle entry up front (clean EOF shutdown via Drop).
        if let Some(existing) = self.children.get(&key) {
            if existing.idle_for() > self.max_idle {
                self.children.remove(&key); // Drop -> shutdown_in_place.
            }
        }

        // Crash recovery (eager): if the held child is already known-dead before
        // this turn, respawn with `--resume <session_id>` so the turn below runs
        // against a fresh process. A child that dies *during* the turn is caught
        // by `drive_turn`'s retry instead (the liveness check is racy on its
        // own, so both paths exist).
        if let Some(existing) = self.children.get_mut(&key) {
            if !existing.is_alive() {
                let recovered = existing.session_id().or_else(|| config.resume.clone());
                self.children.remove(&key);
                let mut recovery_config = config.clone();
                recovery_config.resume = recovered;
                let resident = ResidentClaude::spawn(recovery_config, stderr_path)?;
                self.children.insert(key.clone(), resident);
            }
        }

        // Get-or-spawn.
        if !self.children.contains_key(&key) {
            let resident = ResidentClaude::spawn(config.clone(), stderr_path)?;
            self.children.insert(key.clone(), resident);
        }

        // Drive the turn. A turn can fail because the held child died after the
        // liveness check above (the check is inherently racy: a child that
        // exited a moment ago may not be reaped yet). In that case respawn once
        // with `--resume <session_id>` and retry, so a crash is transparently
        // recovered with memory intact. A second failure (or a healthy turn
        // that simply errored) evicts the child and surfaces the failure.
        match self.drive_turn(&key, user_text, timeout) {
            DriveOutcome::Ok(turn) => Ok(turn),
            DriveOutcome::DeadChild { last_session_id } => {
                self.children.remove(&key); // Drop -> shutdown.
                let mut recovery_config = config.clone();
                recovery_config.resume = last_session_id.or(config.resume.clone());
                let resident = ResidentClaude::spawn(recovery_config, stderr_path)?;
                self.children.insert(key.clone(), resident);
                let resident = self
                    .children
                    .get_mut(&key)
                    .expect("resident present after respawn");
                resident.send_turn(user_text, timeout)
            }
            DriveOutcome::Err(error) => {
                self.children.remove(&key); // Drop -> shutdown.
                Err(error)
            }
        }
    }

    /// Run one turn against the held child and classify the outcome. A turn
    /// that produced no `result` while the child is dead is a crash (eligible
    /// for respawn); a turn that produced no `result` while the child is alive
    /// is a genuine failure.
    fn drive_turn(&mut self, key: &str, user_text: &str, timeout: Duration) -> DriveOutcome {
        let resident = self
            .children
            .get_mut(key)
            .expect("resident present after spawn");
        match resident.send_turn(user_text, timeout) {
            Ok(turn) if turn.success => DriveOutcome::Ok(turn),
            Ok(turn) => {
                if resident.is_alive() {
                    // Alive but the turn really failed: surface it as-is.
                    DriveOutcome::Ok(turn)
                } else {
                    DriveOutcome::DeadChild {
                        last_session_id: resident.session_id(),
                    }
                }
            }
            Err(error) => {
                if resident.is_alive() {
                    DriveOutcome::Err(error)
                } else {
                    DriveOutcome::DeadChild {
                        last_session_id: resident.session_id(),
                    }
                }
            }
        }
    }

    /// Best-effort idle sweep callers may invoke between members.
    pub fn reclaim_idle(&mut self) {
        let stale: Vec<String> = self
            .children
            .iter()
            .filter(|(_, child)| child.idle_for() > self.max_idle)
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            self.children.remove(&key); // Drop -> shutdown.
        }
    }

    /// Number of live pooled children (observability helper; exercised in tests).
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Test-only: forcibly kill the single pooled child (simulating a crash)
    /// without removing the pool entry, so the next `run_turn` exercises crash
    /// recovery against a deterministically-dead child.
    #[cfg(test)]
    fn kill_only_child_for_test(&mut self) {
        for child in self.children.values_mut() {
            let _ = child.child.kill();
            let _ = child.child.wait();
        }
    }
}

impl Default for ResidentPool {
    fn default() -> ResidentPool {
        ResidentPool::new()
    }
}

/// A small RAII tempdir for tests (no extra dev-dependency). Removed on drop.
/// `pub(crate)` so the sibling `resident_daemon` test module reuses it.
#[cfg(test)]
pub(crate) struct TempDir {
    pub(crate) path: PathBuf,
}

#[cfg(test)]
impl TempDir {
    pub(crate) fn new(tag: &str) -> TempDir {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!(
            "resident-test-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

/// Write a fake `claude` script honoring the stream-json line protocol, shared
/// by this module's tests and the sibling `resident_daemon` tests:
///   * emit one `system` init frame with a fixed session_id at startup,
///   * record the PID to `pid_file` exactly once,
///   * loop reading stdin user frames; per frame echo an `assistant` text frame
///     + a `result` frame and write a line to stderr,
///   * stay alive until stdin EOF (resident), then exit 0.
///
/// Returns the path to the executable script.
#[cfg(test)]
pub(crate) fn write_fake_claude(dir: &Path, pid_file: &Path, session_id: &str) -> PathBuf {
    let script = format!(
        r#"#!/usr/bin/env bash
echo "$$" >> "{pid_file}"
printf '%s\n' '{{"type":"system","session_id":"{session_id}"}}'
echo "fake claude up" 1>&2
while IFS= read -r line; do
  echo "got: $line" 1>&2
  printf '%s\n' '{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"ok"}}]}}}}'
  printf '%s\n' '{{"type":"result","subtype":"success","session_id":"{session_id}"}}'
done
exit 0
"#,
        pid_file = pid_file.display(),
        session_id = session_id,
    );
    let path = dir.join("fake-claude.sh");
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
    }
    path
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn config_for(binary: &Path, cwd: &Path) -> ResidentConfig {
        ResidentConfig {
            binary: binary.display().to_string(),
            model: None,
            effort: None,
            output_schema_json: None,
            permission_mode: "plan".into(),
            tools: vec![],
            system_prompt: String::new(),
            mcp_config_path: None,
            add_dirs: vec![],
            cwd: cwd.display().to_string(),
            resume: None,
        }
    }

    // ---- pure unit tests (no process) ----

    #[test]
    fn user_turn_frame_is_valid_stream_json() {
        let frame = user_turn_frame("hello world");
        let value: serde_json::Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["role"], "user");
        assert_eq!(value["message"]["content"][0]["type"], "text");
        assert_eq!(value["message"]["content"][0]["text"], "hello world");
    }

    #[test]
    fn read_one_turn_stops_at_result_and_leaves_next_turn() {
        // Two turns concatenated; reading one turn must leave the second intact.
        let stream = concat!(
            r#"{"type":"system","session_id":"s-1"}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success"}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success"}"#,
            "\n",
        );
        let mut reader = Cursor::new(stream.as_bytes());

        let first = read_one_turn(&mut reader).unwrap();
        assert_eq!(first.last().unwrap().event_type, "result");
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].session_id().as_deref(), Some("s-1"));

        let second = read_one_turn(&mut reader).unwrap();
        assert_eq!(second.len(), 2);
        assert_eq!(second.last().unwrap().event_type, "result");

        // Third read hits EOF -> empty.
        let third = read_one_turn(&mut reader).unwrap();
        assert!(third.is_empty());
    }

    #[test]
    fn resident_recorded_args_uses_input_format_not_dash_p_prompt() {
        let args = resident_recorded_args(Some("sess-9"));
        assert!(args.contains(&"--input-format".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"sess-9".to_string()));
    }

    #[test]
    fn fingerprint_changes_with_model_and_cwd() {
        let base = ResidentConfig {
            binary: "claude".into(),
            model: None,
            effort: None,
            output_schema_json: None,
            permission_mode: "plan".into(),
            tools: vec![],
            system_prompt: String::new(),
            mcp_config_path: None,
            add_dirs: vec![],
            cwd: "/a".into(),
            resume: None,
        };
        let mut other = base.clone();
        other.model = Some("opus".into());
        assert_ne!(base.fingerprint(), other.fingerprint());

        let mut effort_changed = base.clone();
        effort_changed.effort = Some("high".into());
        assert_ne!(base.fingerprint(), effort_changed.fingerprint());

        let mut schema_changed = base.clone();
        schema_changed.output_schema_json = Some("{\"type\":\"object\"}".into());
        assert_ne!(base.fingerprint(), schema_changed.fingerprint());

        let mut cwd_changed = base.clone();
        cwd_changed.cwd = "/b".into();
        assert_ne!(base.fingerprint(), cwd_changed.fingerprint());

        // resume does NOT affect the fingerprint (crash respawn stays same key).
        let mut resumed = base.clone();
        resumed.resume = Some("sess".into());
        assert_eq!(base.fingerprint(), resumed.fingerprint());
    }

    // ---- process tests against a fake claude (unix only) ----

    #[cfg(unix)]
    #[test]
    fn same_child_services_two_turns_and_tracks_session_id() {
        let dir = TempDir::new("two-turns");
        let pid_file = dir.join("pid");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_fake_claude(dir.path.as_path(), &pid_file, "sess-AAA");

        let mut resident =
            ResidentClaude::spawn(config_for(&fake, dir.path.as_path()), &stderr_path).unwrap();

        let t1 = resident.send_turn("first", Duration::from_secs(5)).unwrap();
        assert!(t1.success);
        assert_eq!(t1.session_id.as_deref(), Some("sess-AAA"));

        let t2 = resident
            .send_turn("second", Duration::from_secs(5))
            .unwrap();
        assert!(t2.success);
        assert_eq!(resident.session_id().as_deref(), Some("sess-AAA"));

        // The fake recorded its PID exactly once => one process served both.
        let pid_contents = std::fs::read_to_string(&pid_file).unwrap();
        assert_eq!(pid_contents.lines().count(), 1);

        // stderr was redirected to the file (not deadlocked) and has content.
        let stderr = std::fs::read_to_string(&stderr_path).unwrap();
        assert!(stderr.contains("fake claude up"));

        resident.shutdown();
    }

    /// Write a fake `claude` that stays resident and CONSUMES stdin frames but
    /// never emits a `result` (model wedged / MCP stall). Without an enforced
    /// per-turn timeout, `send_turn`'s blocking `read_line` would hang forever;
    /// the test asserts it returns a `TimedOut` error within the bound instead.
    #[cfg(unix)]
    fn write_hanging_claude(dir: &Path, session_id: &str) -> PathBuf {
        let script = format!(
            r#"#!/usr/bin/env bash
printf '%s\n' '{{"type":"system","session_id":"{session_id}"}}'
# Read and discard stdin frames forever; never print a result.
while IFS= read -r line; do
  :
done
exit 0
"#,
            session_id = session_id,
        );
        let path = dir.join("hanging-claude.sh");
        std::fs::write(&path, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn send_turn_times_out_when_child_never_emits_result() {
        let dir = TempDir::new("turn-timeout");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_hanging_claude(dir.path.as_path(), "sess-HANG");

        let mut resident =
            ResidentClaude::spawn(config_for(&fake, dir.path.as_path()), &stderr_path).unwrap();

        let start = Instant::now();
        let result = resident.send_turn("wedge me", Duration::from_millis(300));
        let elapsed = start.elapsed();

        let error = result.expect_err("a child that never emits result must time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        // The bound was actually enforced (not a forever-hang); allow generous
        // slack for thread scheduling / the kill.
        assert!(
            elapsed < Duration::from_secs(5),
            "send_turn should return near the timeout, took {elapsed:?}"
        );
        // The wedged child was killed so it cannot be reused.
        assert!(!resident.is_alive());
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_closes_stdin_and_child_exits() {
        let dir = TempDir::new("shutdown");
        let pid_file = dir.join("pid");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_fake_claude(dir.path.as_path(), &pid_file, "sess-X");

        let mut resident =
            ResidentClaude::spawn(config_for(&fake, dir.path.as_path()), &stderr_path).unwrap();
        resident.send_turn("hi", Duration::from_secs(5)).unwrap();
        assert!(resident.is_alive());
        // shutdown drops stdin (EOF) -> the `while read` loop ends -> exit 0.
        resident.shutdown();
        // (shutdown consumed self; reaching here without hang is the assertion.)
    }

    #[cfg(unix)]
    #[test]
    fn pool_reuses_child_across_turns() {
        let dir = TempDir::new("pool-reuse");
        let pid_file = dir.join("pid");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_fake_claude(dir.path.as_path(), &pid_file, "sess-POOL");
        let config = config_for(&fake, dir.path.as_path());

        let mut pool = ResidentPool::new();
        let t1 = pool
            .run_turn(
                "member-1",
                config.clone(),
                &stderr_path,
                "one",
                Duration::from_secs(5),
            )
            .unwrap();
        assert!(t1.success);
        let t2 = pool
            .run_turn(
                "member-1",
                config.clone(),
                &stderr_path,
                "two",
                Duration::from_secs(5),
            )
            .unwrap();
        assert!(t2.success);
        assert_eq!(pool.len(), 1);

        let pid_contents = std::fs::read_to_string(&pid_file).unwrap();
        assert_eq!(
            pid_contents.lines().count(),
            1,
            "one child served both turns"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pool_recovers_from_crash_with_resume() {
        let dir = TempDir::new("pool-crash");
        let pid_file = dir.join("pid");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_fake_claude(dir.path.as_path(), &pid_file, "sess-CRASH");
        let config = config_for(&fake, dir.path.as_path());

        let mut pool = ResidentPool::new();
        let t1 = pool
            .run_turn(
                "m",
                config.clone(),
                &stderr_path,
                "one",
                Duration::from_secs(5),
            )
            .unwrap();
        assert!(t1.success);
        assert_eq!(t1.session_id.as_deref(), Some("sess-CRASH"));

        // Simulate a crash: kill the held child deterministically, then drive
        // another turn. The pool must respawn (with --resume) and serve it.
        pool.kill_only_child_for_test();

        let t2 = pool
            .run_turn(
                "m",
                config.clone(),
                &stderr_path,
                "two",
                Duration::from_secs(5),
            )
            .unwrap();
        assert!(
            t2.success,
            "t2 events={:?}",
            t2.events
                .iter()
                .map(|e| e.event_type.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(pool.len(), 1);

        // The fake appends its PID per process; a respawn => two distinct PIDs.
        let pids = std::fs::read_to_string(&pid_file).unwrap();
        assert!(
            pids.lines().count() >= 2,
            "crash forced a respawn (saw {} pid lines)",
            pids.lines().count()
        );
    }

    #[cfg(unix)]
    #[test]
    fn pool_idle_reclaim_respawns() {
        let dir = TempDir::new("pool-idle");
        let pid_file = dir.join("pid");
        let stderr_path = dir.join("claude.stderr");
        let fake = write_fake_claude(dir.path.as_path(), &pid_file, "sess-IDLE");
        let config = config_for(&fake, dir.path.as_path());

        // Zero idle window => every turn reclaims and respawns.
        let mut pool = ResidentPool::with_max_idle(Duration::from_millis(0));
        pool.run_turn(
            "m",
            config.clone(),
            &stderr_path,
            "one",
            Duration::from_secs(5),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        pool.run_turn(
            "m",
            config.clone(),
            &stderr_path,
            "two",
            Duration::from_secs(5),
        )
        .unwrap();

        // With this fake (resident loop, appending its PID) the first child is
        // killed on reclaim and a fresh one spawned, so two PIDs are recorded.
        let pids = std::fs::read_to_string(&pid_file).unwrap();
        assert_eq!(pids.lines().count(), 2, "idle reclaim forced a respawn");
        assert_eq!(pool.len(), 1);
    }
}
