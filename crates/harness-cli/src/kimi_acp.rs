//! kimi ACP (Agent Client Protocol) driver — Agent Team v0.
//!
//! One [`KimiAcpClient`] owns one `kimi acp` child process: line-delimited
//! JSON-RPC over stdio (verified live against kimi 0.27.0). The wire dance is:
//!
//! 1. `initialize` — protocol/capability handshake (10s timeout).
//! 2. `session/new` — opens a session rooted at a cwd; the returned
//!    `sessionId` is the handle every later frame carries.
//! 3. `session/prompt` — streams `session/update` notifications
//!    (`agent_message_chunk`, `agent_thought_chunk`, `tool_call`,
//!    `tool_call_update`, ...) and finishes with the request's response
//!    (`result.stopReason`).
//! 4. `session/cancel` — asks the agent to abort the in-flight prompt; a
//!    wedged process is killed as a fallback.
//!
//! Two deliberate v0 decisions:
//!
//! - `clientCapabilities` is advertised EMPTY. Advertising
//!   `fs.readTextFile/writeTextFile` tells the agent to route file IO through
//!   this client; harness v0 does not serve client methods, so the agent must
//!   use its own built-in tools instead. Any agent→client REQUEST that still
//!   arrives (e.g. `session/request_permission`) is answered with a JSON-RPC
//!   "method not implemented" error so the agent can never wedge waiting on
//!   us — consistent with the v0 posture of no interactive approvals.
//! - Reasoning streams (`agent_thought_chunk`) are passed through to the
//!   caller verbatim. The team-run orchestrator deliberately does not persist
//!   them: thinking is not evidence, replayable history, or peer-visible
//!   state. The driver itself stays a faithful transport.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{kill_worker_tree, resolve_kimi_bin, CliError, CliResult};

/// Default idle timeout for one `session/prompt` turn: no ACP frame at all
/// for this long means the session is wedged (auth stall, network hang).
pub(crate) const DEFAULT_PROMPT_IDLE_TIMEOUT_SECS: u64 = 180;

/// Handshake (`initialize` / `session/new`) response timeout.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Grace window between `session/cancel` and killing the process group.
const CANCEL_GRACE: Duration = Duration::from_secs(15);

/// Terminal result of one `session/prompt` round.
pub(crate) struct PromptOutcome {
    /// `result.stopReason` as reported by the agent (`end_turn`, `cancelled`,
    /// `refusal`, `max_tokens`, ...); `"unknown"` when the frame omitted it.
    pub(crate) stop_reason: String,
}

/// One `kimi acp` child process speaking line-delimited JSON-RPC. Not `Sync`:
/// one owner drives request/response rounds sequentially (`session/prompt`
/// streams on the same stdout every frame arrives on).
pub(crate) struct KimiAcpClient {
    child: Child,
    stdin: ChildStdin,
    next_request_id: u64,
    /// In-flight request id → channel the reader thread delivers the matching
    /// response frame on. The entry is removed when the response arrives (or
    /// the waiter times out), so a late response is dropped, never misrouted.
    pending: Arc<Mutex<HashMap<u64, Sender<serde_json::Value>>>>,
    /// Notifications (and agent→client requests) from the reader thread.
    updates: Receiver<serde_json::Value>,
    reader: Option<JoinHandle<()>>,
    /// Rolling tail of the child's stderr, for error messages.
    stderr_tail: Arc<Mutex<String>>,
    session_id: Option<String>,
    /// Requested model. v0 RECORDS it only — kimi acp model selection lands in
    /// a later iteration (session/set_config_option or spawn env).
    #[allow(dead_code)]
    model: Option<String>,
}

impl KimiAcpClient {
    /// Spawn `kimi acp` rooted at `cwd` and run the `initialize` +
    /// `session/new` handshake. The binary resolves exactly like the one-shot
    /// path ([`resolve_kimi_bin`]: KIMI_CODE_BIN → PATH → ~/.kimi-code/bin), so
    /// a test PATH shim intercepts the spawn. The child is its own process
    /// group leader so a wedged session can be killed tree-wide.
    pub(crate) fn spawn(cwd: &Path, model: Option<&str>) -> CliResult<Self> {
        let mut cmd = Command::new(resolve_kimi_bin());
        cmd.arg("acp")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .spawn()
            .map_err(|error| CliError::Usage(format!("failed to spawn kimi acp: {error}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CliError::Usage("kimi acp stdin not available".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CliError::Usage("kimi acp stdout not available".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CliError::Usage("kimi acp stderr not available".to_string()))?;

        let pending: Arc<Mutex<HashMap<u64, Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (update_tx, updates) = channel::<serde_json::Value>();

        // Reader thread: one JSON-RPC frame per stdout line. A frame with
        // `method` is a notification or an agent→client request → update
        // queue; a frame with only `id` is a response → the pending waiter.
        // stdout closing (child killed/exited) ends the loop and drops
        // `update_tx`, which is how `prompt` learns the session died.
        let reader_pending = Arc::clone(&pending);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(frame) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                    continue;
                };
                if frame.get("method").is_some() {
                    if update_tx.send(frame).is_err() {
                        break;
                    }
                } else if let Some(id) = frame.get("id").and_then(|v| v.as_u64()) {
                    let waiter = lock(&reader_pending).remove(&id);
                    if let Some(waiter) = waiter {
                        let _ = waiter.send(frame);
                    }
                }
            }
        });

        // Drain stderr so a chatty child cannot fill the pipe and block; keep
        // a small tail for diagnostics (auth errors land here, not on stdout).
        let stderr_tail: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let tail = Arc::clone(&stderr_tail);
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                let mut buf = lock(&tail);
                buf.push_str(&line);
                buf.push('\n');
                let over = buf.len().saturating_sub(4096);
                if over > 0 {
                    buf.drain(..over);
                }
            }
        });

        let mut client = Self {
            child,
            stdin,
            next_request_id: 1,
            pending,
            updates,
            reader: Some(reader),
            stderr_tail,
            session_id: None,
            model: model.map(str::to_string),
        };
        client.handshake(cwd)?;
        Ok(client)
    }

    /// The ACP session id negotiated at spawn (`session/new`).
    pub(crate) fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// `initialize` + `session/new`, each with a 10s response timeout.
    fn handshake(&mut self, cwd: &Path) -> CliResult<()> {
        let initialize = self.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "harness", "version": "0.1.0" },
            }),
        )?;
        let frame = await_response(initialize, HANDSHAKE_TIMEOUT, "initialize")
            .inspect_err(|_| self.kill_quiet())?;
        if let Some(error) = frame.get("error") {
            self.kill_quiet();
            return Err(CliError::Usage(format!(
                "kimi acp initialize rejected: {error}"
            )));
        }

        let session_new = self.request(
            "session/new",
            serde_json::json!({
                "cwd": cwd.to_string_lossy(),
                "mcpServers": [],
            }),
        )?;
        let frame = await_response(session_new, HANDSHAKE_TIMEOUT, "session/new")
            .inspect_err(|_| self.kill_quiet())?;
        let session_id = frame
            .get("result")
            .and_then(|result| result.get("sessionId"))
            .and_then(|id| id.as_str())
            .map(str::to_string);
        match session_id {
            Some(session_id) => {
                self.session_id = Some(session_id);
                Ok(())
            }
            None => {
                self.kill_quiet();
                Err(CliError::Usage(format!(
                    "kimi acp session/new returned no sessionId: {frame}"
                )))
            }
        }
    }

    /// Write one JSON-RPC request frame and return the receiver its response
    /// will arrive on. Ids are assigned sequentially from 1, matching the
    /// protocol trace in the module banner.
    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> CliResult<(u64, Receiver<serde_json::Value>)> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        let (tx, rx) = channel();
        lock(&self.pending).insert(id, tx);
        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(error) = write_frame(&mut self.stdin, &frame) {
            lock(&self.pending).remove(&id);
            return Err(error);
        }
        Ok((id, rx))
    }

    /// Run one `session/prompt` turn to completion.
    ///
    /// `on_update` fires for every `session/update` notification, passed the
    /// `params.update` object (so callers pattern-match `sessionUpdate`
    /// directly). Frames are also counted as ACTIVITY: any frame resets the
    /// idle clock, so a slow-but-streaming turn never times out.
    ///
    /// On `idle_timeout` (0 = default 180s) the client first sends
    /// `session/cancel` and waits [`CANCEL_GRACE`] for the prompt response;
    /// a still-silent session is then killed tree-wide and an error returned.
    pub(crate) fn prompt(
        &mut self,
        text: &str,
        idle_timeout: Duration,
        mut on_update: impl FnMut(&serde_json::Value),
    ) -> CliResult<PromptOutcome> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| CliError::Usage("kimi acp session not established".to_string()))?;
        let (prompt_id, response) = self.request(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )?;
        let idle_limit = if idle_timeout.is_zero() {
            Duration::from_secs(DEFAULT_PROMPT_IDLE_TIMEOUT_SECS)
        } else {
            idle_timeout
        };

        let mut last_activity = Instant::now();
        let mut cancelled_at: Option<Instant> = None;
        loop {
            // Response FIRST: the reader thread can deliver the terminal
            // response and immediately hit EOF (child exit), which disconnects
            // the updates channel — checking updates first would mistake a
            // completed turn for a dead session.
            match response.try_recv() {
                Ok(frame) => {
                    // ...but the reader dispatched every update that preceded
                    // the response on the wire BEFORE enqueueing it, so a full
                    // drain here replays the tail of the stream in order.
                    while let Ok(update) = self.updates.try_recv() {
                        self.handle_incoming(&update, &mut on_update)?;
                    }
                    return Ok(prompt_outcome(&frame));
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    return Err(self.session_ended_error("prompt"));
                }
            }
            match self.updates.try_recv() {
                Ok(frame) => {
                    last_activity = Instant::now();
                    self.handle_incoming(&frame, &mut on_update)?;
                    continue;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    lock(&self.pending).remove(&prompt_id);
                    return Err(self.session_ended_error("prompt"));
                }
            }

            if let Some(cancelled) = cancelled_at {
                if cancelled.elapsed() > CANCEL_GRACE {
                    self.kill_quiet();
                    lock(&self.pending).remove(&prompt_id);
                    return Err(CliError::Usage(format!(
                        "kimi acp prompt idle for {}s and ignored session/cancel; session killed{}",
                        idle_limit.as_secs(),
                        self.stderr_suffix(),
                    )));
                }
            } else if last_activity.elapsed() > idle_limit {
                // First strike: ask the agent to cancel, keep waiting briefly.
                self.cancel()?;
                cancelled_at = Some(Instant::now());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Handle one queued frame: agent→client REQUESTS (frames carrying both
    /// `id` and `method`, e.g. `fs/read_text_file` or
    /// `session/request_permission`) get a JSON-RPC "method not implemented"
    /// error so the agent never wedges on a client v0 does not serve (the id
    /// is echoed verbatim — JSON-RPC allows non-numeric ids);
    /// `session/update` notifications go to the caller's callback.
    fn handle_incoming(
        &mut self,
        frame: &serde_json::Value,
        on_update: &mut impl FnMut(&serde_json::Value),
    ) -> CliResult<()> {
        if frame.get("method").and_then(|m| m.as_str()) == Some("session/update") {
            let update = frame
                .get("params")
                .and_then(|params| params.get("update"))
                .unwrap_or(frame);
            on_update(update);
            return Ok(());
        }
        if let Some(id) = frame.get("id") {
            let method = frame
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            let error = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("harness v0 does not implement client method {method}"),
                },
            });
            write_frame(&mut self.stdin, &error)?;
        }
        Ok(())
    }

    /// Send `session/cancel` for the current session (request form, per the
    /// verified wire trace). Does not wait for the response: the caller's
    /// prompt loop is already in its cancel-grace window.
    pub(crate) fn cancel(&mut self) -> CliResult<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| CliError::Usage("kimi acp session not established".to_string()))?;
        // Register the pending entry so a response is consumed (then dropped)
        // instead of leaking in the map.
        let (_id, _rx) = self.request(
            "session/cancel",
            serde_json::json!({ "sessionId": session_id }),
        )?;
        Ok(())
    }

    /// Kill the process group and reap the child; joins the reader thread.
    pub(crate) fn shutdown(mut self) {
        self.kill_quiet();
    }

    /// Kill the whole process group unless the child already exited. Safe to
    /// call repeatedly: a reaped child makes this a no-op (so a recycled pid
    /// is never signalled by a late Drop).
    fn kill_quiet(&mut self) {
        match self.child.try_wait() {
            Ok(None) => kill_worker_tree(&mut self.child),
            _ => {
                let _ = self.child.wait();
            }
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }

    fn session_ended_error(&self, what: &str) -> CliError {
        CliError::Usage(format!(
            "kimi acp {what} failed: session ended{}",
            self.stderr_suffix()
        ))
    }

    fn stderr_suffix(&self) -> String {
        let tail = lock(&self.stderr_tail);
        let trimmed = tail.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            format!("; stderr tail: {trimmed}")
        }
    }
}

impl Drop for KimiAcpClient {
    /// Safety net: a client dropped without `shutdown` (error path mid-turn)
    /// must never leak a kimi process.
    fn drop(&mut self) {
        self.kill_quiet();
    }
}

/// Lock a mutex, recovering from poisoning: every payload here is a plain
/// buffer/map where a panicking writer cannot leave a lie behind.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Write one frame as a single line + flush (the agent reads line-delimited).
fn write_frame(stdin: &mut ChildStdin, frame: &serde_json::Value) -> CliResult<()> {
    let mut line = serde_json::to_string(frame)?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .and_then(|()| stdin.flush())
        .map_err(|error| CliError::Usage(format!("kimi acp stdin write failed: {error}")))
}

/// Block for a handshake response up to `timeout`.
fn await_response(
    (_id, rx): (u64, Receiver<serde_json::Value>),
    timeout: Duration,
    what: &str,
) -> CliResult<serde_json::Value> {
    rx.recv_timeout(timeout).map_err(|error| {
        let reason = match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => {
                format!("timed out after {}s", timeout.as_secs())
            }
            std::sync::mpsc::RecvTimeoutError::Disconnected => "session ended".to_string(),
        };
        CliError::Usage(format!("kimi acp {what} {reason}"))
    })
}

/// Fold the terminal `session/prompt` response into a [`PromptOutcome`].
fn prompt_outcome(frame: &serde_json::Value) -> PromptOutcome {
    let stop_reason = frame
        .get("result")
        .and_then(|result| result.get("stopReason"))
        .and_then(|reason| reason.as_str())
        .unwrap_or("unknown")
        .to_string();
    PromptOutcome { stop_reason }
}
