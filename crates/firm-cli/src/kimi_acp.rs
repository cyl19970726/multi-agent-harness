//! kimi ACP (Agent Client Protocol) driver — Agent Team v0.
//!
//! One [`KimiAcpClient`] owns one `kimi acp` child process: line-delimited
//! JSON-RPC over stdio (verified live against kimi 0.27.0). The wire dance is:
//!
//! 1. `initialize` — protocol/capability handshake (10s timeout).
//! 2. `session/new` opens a session rooted at a cwd. Reattachment prefers
//!    `session/resume` (no history replay) and falls back to `session/load`
//!    only when an older ACP server reports method-not-found.
//! 3. `session/prompt` — streams `session/update` notifications
//!    (`agent_message_chunk`, `agent_thought_chunk`, `tool_call`,
//!    `tool_call_update`, ...) and finishes with the request's response
//!    (`result.stopReason`).
//! 4. `session/cancel` is a JSON-RPC notification (no request id). It asks the
//!    agent to abort the in-flight prompt; a wedged process is killed as a
//!    fallback. Host Close remains distinct and terminates only the
//!    Harness-owned ACP runtime.
//!
//! Two deliberate v0 decisions:
//!
//! - `clientCapabilities` is advertised EMPTY. Advertising
//!   `fs.readTextFile/writeTextFile` tells the agent to route file IO through
//!   this client; harness v0 does not serve client methods, so the agent must
//!   use its own built-in tools instead. `session/request_permission` is the
//!   one reverse-RPC method the Team Member adapter serves: the orchestrator
//!   persists new interactions as provider request/response TeamMessages and
//!   the same ACP turn waits for an answer. PendingInteraction remains only a
//!   legacy compatibility read/resolve path. Unknown reverse-RPC methods still
//!   fail closed with method-not-found.
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
    /// Set when the provider failed the turn itself: a JSON-RPC error
    /// response, or a terminal response with no `result.stopReason`. The
    /// streamed text then holds provider error output, not a member report,
    /// and Harness must record a provider_error round instead of fabricating
    /// an empty or partial Handoff (parity with the Claude provider-error
    /// contract, issue #293).
    pub(crate) provider_error: Option<String>,
}

pub(crate) enum PromptControl {
    Continue,
    Cancel,
    TerminateRuntime,
}

#[derive(Clone, Copy)]
struct PromptTimeouts {
    idle: Duration,
    cancel_grace: Duration,
}

impl PromptTimeouts {
    fn production(idle: Duration) -> Self {
        Self {
            idle,
            cancel_grace: CANCEL_GRACE,
        }
    }
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
    /// Requested model alias, applied through ACP
    /// `session/set_config_option(configId=model)` after session creation.
    model: Option<String>,
    /// Provider-neutral reasoning effort. Kimi ACP exposes this as the
    /// `thinking` config option; the adapter keeps that wire spelling local.
    effort: Option<String>,
    effective_model: Option<String>,
    effective_effort: Option<String>,
    config_options: Vec<serde_json::Value>,
    provider_version: Option<String>,
}

impl KimiAcpClient {
    /// Spawn `kimi acp` rooted at `cwd` and run the `initialize` +
    /// `session/new` handshake. The binary resolves exactly like the one-shot
    /// path ([`resolve_kimi_bin`]: KIMI_CODE_BIN → PATH → ~/.kimi-code/bin), so
    /// a test PATH shim intercepts the spawn. The child is its own process
    /// group leader so a wedged session can be killed tree-wide.
    pub(crate) fn spawn(
        cwd: &Path,
        model: Option<&str>,
        effort: Option<&str>,
        resume_session_id: Option<&str>,
        collaboration_env: &[(String, String)],
    ) -> CliResult<Self> {
        let mut cmd = Command::new(resolve_kimi_bin());
        cmd.arg("acp")
            .envs(collaboration_env.iter().cloned())
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
            effort: effort.map(str::to_string),
            effective_model: None,
            effective_effort: None,
            config_options: Vec::new(),
            provider_version: None,
        };
        client.handshake(cwd, resume_session_id)?;
        Ok(client)
    }

    /// The ACP session id negotiated at spawn (`session/new`).
    pub(crate) fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub(crate) fn provider_version(&self) -> Option<&str> {
        self.provider_version.as_deref()
    }

    pub(crate) fn model(&self) -> Option<&str> {
        self.effective_model.as_deref()
    }

    pub(crate) fn effort(&self) -> Option<&str> {
        self.effective_effort.as_deref()
    }

    pub(crate) fn ensure_transport_alive(&mut self) -> CliResult<()> {
        let reader_ended = self
            .reader
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished);
        let child_ended = self
            .child
            .try_wait()
            .map_err(|error| CliError::Usage(format!("failed to inspect kimi acp: {error}")))?;
        if reader_ended || child_ended.is_some() {
            return Err(self.session_ended_error("idle supervisor"));
        }
        Ok(())
    }

    /// `initialize` plus one session attach operation, each with a 10s
    /// response timeout. A known session prefers the lightweight
    /// `session/resume` contract added by current Kimi ACP. Older reviewed
    /// servers may expose only `session/load`; that path is retained as an
    /// explicit method-not-found fallback and its replay notifications are
    /// drained before a new Harness prompt begins.
    fn handshake(&mut self, cwd: &Path, resume_session_id: Option<&str>) -> CliResult<()> {
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
        self.provider_version = frame
            .pointer("/result/agentInfo/version")
            .and_then(|version| version.as_str())
            .map(str::to_string);

        let session_params = |session_id: &str| {
            serde_json::json!({
                "sessionId": session_id,
                "cwd": cwd.to_string_lossy(),
                "mcpServers": [],
            })
        };
        let (method, frame) = match resume_session_id {
            Some(session_id) => {
                let resume = self.request("session/resume", session_params(session_id))?;
                let resume_frame = await_response(resume, HANDSHAKE_TIMEOUT, "session/resume")
                    .inspect_err(|_| self.kill_quiet())?;
                if acp_method_not_found(&resume_frame) {
                    let load = self.request("session/load", session_params(session_id))?;
                    (
                        "session/load",
                        await_response(load, HANDSHAKE_TIMEOUT, "session/load")
                            .inspect_err(|_| self.kill_quiet())?,
                    )
                } else {
                    ("session/resume", resume_frame)
                }
            }
            None => {
                let response = self.request(
                    "session/new",
                    serde_json::json!({
                        "cwd": cwd.to_string_lossy(),
                        "mcpServers": [],
                    }),
                )?;
                (
                    "session/new",
                    await_response(response, HANDSHAKE_TIMEOUT, "session/new")
                        .inspect_err(|_| self.kill_quiet())?,
                )
            }
        };
        if let Some(error) = frame.get("error") {
            self.kill_quiet();
            return Err(CliError::Usage(format!(
                "kimi acp {method} rejected: {error}"
            )));
        }
        self.config_options = frame
            .pointer("/result/configOptions")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let session_id = frame
            .get("result")
            .and_then(|result| result.get("sessionId"))
            .and_then(|id| id.as_str())
            .map(str::to_string)
            .or_else(|| resume_session_id.map(str::to_string));
        // `session/load` replays historical session/update records before its
        // response. They are provider-native history, not activity from the
        // next Harness round. The stdout reader preserves wire order, so the
        // matching response proves every preceding replay frame is already in
        // this queue and can be discarded deterministically.
        self.drain_attach_updates();
        match session_id {
            Some(session_id) => {
                self.session_id = Some(session_id);
                self.apply_requested_controls()
            }
            None => {
                self.kill_quiet();
                Err(CliError::Usage(format!(
                    "kimi acp {method} returned no sessionId: {frame}"
                )))
            }
        }
    }

    fn drain_attach_updates(&mut self) {
        while self.updates.try_recv().is_ok() {}
    }

    /// Apply the requested Kimi model to the newly-created ACP session. A
    /// named model is a real execution constraint, not display metadata: an
    /// unknown/unavailable alias fails before the first prompt.
    fn apply_requested_controls(&mut self) -> CliResult<()> {
        let mut model_changed_without_refreshed_options = false;
        if let Some(model) = self.model.clone() {
            let advertised_model = current_config_value(&self.config_options, "model");
            let model_changed = advertised_model.as_deref() != Some(model.as_str());
            let refreshed_options = self.apply_config_option("model", "model", &model)?;
            model_changed_without_refreshed_options = model_changed && !refreshed_options;
            if model_changed_without_refreshed_options {
                // `configOptions` belongs to the model that advertised it. If
                // changing the model did not return a refreshed option set,
                // the old model's thinking default and supported values are
                // no longer evidence. Keep an explicit requested effort for
                // the provider to validate, but never project the stale
                // default as effective on the new model.
                self.config_options.retain(|option| {
                    option.get("id").and_then(|value| value.as_str()) != Some("thinking")
                });
            }
            self.effective_model = Some(model);
        } else {
            self.effective_model = current_config_value(&self.config_options, "model");
        }
        if let Some(effort) = self.effort.clone() {
            self.apply_config_option("thinking", "reasoning effort", &effort)?;
            self.effective_effort = Some(effort);
        } else if model_changed_without_refreshed_options {
            self.effective_effort = None;
        } else {
            self.effective_effort = current_config_value(&self.config_options, "thinking");
        }
        Ok(())
    }

    fn apply_config_option(
        &mut self,
        config_id: &str,
        label: &str,
        requested: &str,
    ) -> CliResult<bool> {
        if let Some(false) = config_option_supports(&self.config_options, config_id, requested) {
            self.kill_quiet();
            return Err(CliError::Usage(format!(
                "kimi acp does not advertise requested {label} `{requested}` in config option `{config_id}`"
            )));
        }
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| CliError::Usage("kimi acp session not established".to_string()))?;
        let response = self.request(
            "session/set_config_option",
            serde_json::json!({
                "sessionId": session_id,
                "configId": config_id,
                "value": requested,
            }),
        )?;
        let frame = await_response(response, HANDSHAKE_TIMEOUT, "session/set_config_option")
            .inspect_err(|_| self.kill_quiet())?;
        if let Some(error) = frame.get("error") {
            self.kill_quiet();
            return Err(CliError::Usage(format!(
                "kimi acp rejected requested {label} {requested}: {error}"
            )));
        }
        let refreshed_options = frame
            .pointer("/result/configOptions")
            .and_then(|value| value.as_array())
            .cloned();
        if let Some(options) = refreshed_options {
            self.config_options = options;
            Ok(true)
        } else {
            Ok(false)
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

    /// Write one JSON-RPC notification frame. ACP defines `session/cancel` as
    /// a notification, so adding a request id changes dispatch semantics and
    /// makes a conforming server report method-not-found.
    fn notify(&mut self, method: &str, params: serde_json::Value) -> CliResult<()> {
        write_frame(
            &mut self.stdin,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            }),
        )
    }

    /// Run one `session/prompt` turn to completion.
    ///
    /// `on_update` fires for every `session/update` notification, passed the
    /// `params.update` object (so callers pattern-match `sessionUpdate`
    /// directly). Well-formed frames for the active session count as activity,
    /// so a slow-but-streaming turn never times out. Frames for stale or other
    /// sessions are ignored and cannot keep a wedged prompt alive.
    ///
    /// On `idle_timeout` (0 = default 180s) the client first sends
    /// `session/cancel` and waits [`CANCEL_GRACE`] for the prompt response;
    /// a still-silent session is then killed tree-wide and an error returned.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prompt(
        &mut self,
        text: &str,
        idle_timeout: Duration,
        mut on_accepted: impl FnMut(&str) -> CliResult<()>,
        mut on_update: impl FnMut(&serde_json::Value),
        mut on_request: impl FnMut(&serde_json::Value) -> CliResult<serde_json::Value>,
        mut on_request_written: impl FnMut(&serde_json::Value) -> CliResult<()>,
        mut control: impl FnMut() -> CliResult<PromptControl>,
    ) -> CliResult<PromptOutcome> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| CliError::Usage("kimi acp session not established".to_string()))?;
        let request = self.request(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )?;
        self.drive_prompt(
            request,
            PromptTimeouts::production(idle_timeout),
            &mut on_accepted,
            &mut on_update,
            &mut on_request,
            &mut on_request_written,
            &mut control,
        )
    }

    /// Drive the channel side of one already-written prompt request.
    ///
    /// Kept separate from [`Self::prompt`] so receipt ordering can be tested
    /// deterministically against scripted response/update channels without a
    /// timing-sensitive provider process. Production calls this immediately
    /// after writing `session/prompt`; there is no alternate receipt path.
    #[allow(clippy::too_many_arguments)]
    fn drive_prompt(
        &mut self,
        request: (u64, Receiver<serde_json::Value>),
        timeouts: PromptTimeouts,
        on_accepted: &mut impl FnMut(&str) -> CliResult<()>,
        on_update: &mut impl FnMut(&serde_json::Value),
        on_request: &mut impl FnMut(&serde_json::Value) -> CliResult<serde_json::Value>,
        on_request_written: &mut impl FnMut(&serde_json::Value) -> CliResult<()>,
        control: &mut impl FnMut() -> CliResult<PromptControl>,
    ) -> CliResult<PromptOutcome> {
        let (prompt_id, response) = request;
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| CliError::Usage("kimi acp session not established".to_string()))?;
        let idle_limit = if timeouts.idle.is_zero() {
            Duration::from_secs(DEFAULT_PROMPT_IDLE_TIMEOUT_SECS)
        } else {
            timeouts.idle
        };
        let provider_receipt_id = format!("kimi-acp-prompt:{prompt_id}");
        let mut accepted = false;

        let mut last_activity = Instant::now();
        let mut cancelled_at: Option<Instant> = None;
        loop {
            if cancelled_at.is_none() {
                match control()? {
                    PromptControl::Continue => {}
                    PromptControl::Cancel => {
                        self.cancel()?;
                        cancelled_at = Some(Instant::now());
                    }
                    PromptControl::TerminateRuntime => {
                        self.kill_quiet();
                        lock(&self.pending).remove(&prompt_id);
                        return Ok(PromptOutcome {
                            stop_reason: "harness_runtime_closed".to_string(),
                            provider_error: None,
                        });
                    }
                }
            }
            // Response FIRST: the reader thread can deliver the terminal
            // response and immediately hit EOF (child exit), which disconnects
            // the updates channel — checking updates first would mistake a
            // completed turn for a dead session.
            match response.try_recv() {
                Ok(frame) => {
                    let outcome = prompt_outcome(&frame);
                    // ...but the reader dispatched every update that preceded
                    // the response on the wire BEFORE enqueueing it, so
                    // draining here recovers the tail of the stream in order.
                    // Drain BEFORE deciding acceptance: buffered prompt output
                    // or a reverse request proves the provider started this
                    // turn even when the terminal frame won the same poll.
                    let mut tail = Vec::new();
                    while let Ok(update) = self.updates.try_recv() {
                        tail.push(update);
                    }
                    // Only a turn the provider actually started may publish a
                    // receipt. A terminal frame with NO preceding session
                    // update that carries a provider error (403/429, immediate
                    // rejection) never started work: publishing a receipt for
                    // it would complete the Assignment delivery and burn the
                    // assignment with no Handoff and nothing to replay.
                    if !accepted
                        && (tail
                            .iter()
                            .any(|frame| prompt_acceptance_evidence(frame, &session_id))
                            || outcome.provider_error.is_none())
                    {
                        // Publish the receipt before handling the tail so tools
                        // invoked by this turn may immediately send a
                        // correlation-valid handoff or peer message.
                        on_accepted(&provider_receipt_id)?;
                    }
                    for update in &tail {
                        self.handle_incoming(update, on_update, on_request, on_request_written)?;
                    }
                    return Ok(outcome);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    return Err(self.session_ended_error("prompt"));
                }
            }
            match self.updates.try_recv() {
                Ok(frame) => {
                    if !accepted && prompt_acceptance_evidence(&frame, &session_id) {
                        // ACP has no separate prompt-start acknowledgement.
                        // Its first prompt-scoped update or matching-session
                        // permission request is the earliest honest evidence
                        // that the prompt was accepted. Publish before
                        // handling the frame so tools invoked by this turn can
                        // immediately send a correlation-valid handoff or peer
                        // message.
                        on_accepted(&provider_receipt_id)?;
                        accepted = true;
                    }
                    // The reader multiplexes every ACP frame onto this
                    // channel. Refresh the prompt clock only after validating
                    // that the frame is meaningful activity for this session;
                    // otherwise wrong-session traffic can hide a wedged turn.
                    if active_session_activity(&frame, &session_id) {
                        last_activity = Instant::now();
                    }
                    self.handle_incoming(&frame, on_update, on_request, on_request_written)?;
                    continue;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    lock(&self.pending).remove(&prompt_id);
                    return Err(self.session_ended_error("prompt"));
                }
            }

            if let Some(cancelled) = cancelled_at {
                if cancelled.elapsed() > timeouts.cancel_grace {
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

    /// Handle one queued frame. `session/request_permission` is routed to the
    /// orchestrator so it can durably pause for Lead/Human input. Other client
    /// methods fail closed because this client deliberately advertises no FS or
    /// terminal capability.
    fn handle_incoming(
        &mut self,
        frame: &serde_json::Value,
        on_update: &mut impl FnMut(&serde_json::Value),
        on_request: &mut impl FnMut(&serde_json::Value) -> CliResult<serde_json::Value>,
        on_request_written: &mut impl FnMut(&serde_json::Value) -> CliResult<()>,
    ) -> CliResult<()> {
        if frame.get("method").and_then(|m| m.as_str()) == Some("session/update") {
            if frame
                .pointer("/params/sessionId")
                .and_then(|id| id.as_str())
                != self.session_id.as_deref()
            {
                return Ok(());
            }
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
            let response = if method == "session/request_permission" {
                if frame
                    .pointer("/params/sessionId")
                    .and_then(|id| id.as_str())
                    != self.session_id.as_deref()
                {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32602, "message": "session/request_permission sessionId does not match the active session"},
                    })
                } else {
                    match on_request(frame) {
                        Ok(result) => serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result,
                        }),
                        Err(error) => serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {"code": -32000, "message": error.to_string()},
                        }),
                    }
                }
            } else {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("harness does not implement client method {method}"),
                    },
                })
            };
            write_frame(&mut self.stdin, &response)?;
            if method == "session/request_permission" && response.get("result").is_some() {
                on_request_written(frame)?;
            }
        }
        Ok(())
    }

    /// Send the ACP `session/cancel` notification for the current session.
    /// There is no cancel response: the caller keeps waiting for the terminal
    /// `session/prompt` response during its cancel-grace window.
    pub(crate) fn cancel(&mut self) -> CliResult<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| CliError::Usage("kimi acp session not established".to_string()))?;
        self.notify(
            "session/cancel",
            serde_json::json!({ "sessionId": session_id }),
        )
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

/// Whether a queued agent frame is valid liveness evidence for `session_id`.
///
/// The update channel is process-wide, so session identity must be checked
/// before the prompt idle clock is refreshed. Keep this broader than receipt
/// evidence: session-level updates are legitimate liveness but do not prove
/// that the current prompt was accepted.
fn active_session_activity(frame: &serde_json::Value, session_id: &str) -> bool {
    let method = frame.get("method").and_then(|method| method.as_str());
    let matches_session = frame
        .pointer("/params/sessionId")
        .and_then(|id| id.as_str())
        == Some(session_id);
    match method {
        Some("session/update") => {
            matches_session
                && frame
                    .pointer("/params/update/sessionUpdate")
                    .and_then(|kind| kind.as_str())
                    .is_some()
        }
        Some("session/request_permission") => {
            matches_session && frame.get("id").is_some_and(|id| !id.is_null())
        }
        _ => false,
    }
}

/// Whether an agent→client frame proves that the current prompt started.
///
/// ACP multiplexes prompt output with session-scoped notifications on the
/// same `session/update` stream. In particular, an
/// `available_commands_update` may be emitted after session creation or after
/// asynchronous skill discovery, including immediately before an unrelated
/// prompt rejection. Such session state must still be handled, but cannot be
/// used as a provider receipt. Fail closed for unknown update kinds: only
/// prompt output defined by ACP v1, or the exact `session/request_permission`
/// reverse request for this session, is acceptance evidence. Unknown reverse
/// methods and requests for another session fail closed.
fn prompt_acceptance_evidence(frame: &serde_json::Value, session_id: &str) -> bool {
    let method = frame.get("method").and_then(|method| method.as_str());
    if frame.get("id").is_some_and(|id| !id.is_null())
        && method == Some("session/request_permission")
        && frame
            .pointer("/params/sessionId")
            .and_then(|id| id.as_str())
            == Some(session_id)
    {
        return true;
    }
    if method != Some("session/update") {
        return false;
    }
    if frame
        .pointer("/params/sessionId")
        .and_then(|id| id.as_str())
        != Some(session_id)
    {
        return false;
    }
    matches!(
        frame
            .pointer("/params/update/sessionUpdate")
            .and_then(|kind| kind.as_str()),
        Some(
            "user_message_chunk"
                | "agent_message_chunk"
                | "agent_thought_chunk"
                | "tool_call"
                | "tool_call_update"
                | "plan"
                | "plan_update"
                | "plan_removed"
        )
    )
}

fn current_config_value(options: &[serde_json::Value], config_id: &str) -> Option<String> {
    options
        .iter()
        .find(|option| option.get("id").and_then(|value| value.as_str()) == Some(config_id))
        .and_then(|option| option.get("currentValue"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

/// `None` means the provider omitted its option catalog; the subsequent ACP
/// request/response remains the authoritative receipt. `Some(false)` means the
/// provider advertised the option and explicitly excluded the requested value.
fn config_option_supports(
    options: &[serde_json::Value],
    config_id: &str,
    requested: &str,
) -> Option<bool> {
    let option = options
        .iter()
        .find(|option| option.get("id").and_then(|value| value.as_str()) == Some(config_id))?;
    let values = option.get("options")?.as_array()?;
    Some(
        values
            .iter()
            .any(|value| value.get("value").and_then(|value| value.as_str()) == Some(requested)),
    )
}

/// Lock a mutex, recovering from poisoning: every payload here is a plain
/// buffer/map where a panicking writer cannot leave a lie behind.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn acp_method_not_found(frame: &serde_json::Value) -> bool {
    frame
        .pointer("/error/code")
        .and_then(|value| value.as_i64())
        == Some(-32601)
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

/// Fold the terminal `session/prompt` response into a [`PromptOutcome`]. A
/// JSON-RPC error frame, a response without `result.stopReason`, or a
/// `stopReason` that did not complete the turn means the provider failed the
/// turn: surface it as `provider_error` so the caller records a failed
/// provider_error round rather than a fabricated Handoff.
fn prompt_outcome(frame: &serde_json::Value) -> PromptOutcome {
    // `error: null` is a present-but-empty key: many JSON-RPC servers
    // serialize every field. Only a non-null `error` object is a failure.
    if let Some(error) = frame.get("error").filter(|error| !error.is_null()) {
        let code = error
            .get("code")
            .and_then(|code| code.as_i64())
            .map(|code| format!(" (code {code})"))
            .unwrap_or_default();
        let message = error
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or("unknown provider error");
        return PromptOutcome {
            stop_reason: "error".to_string(),
            provider_error: Some(format!("session/prompt rejected{code}: {message}")),
        };
    }
    let Some(stop_reason) = frame
        .get("result")
        .and_then(|result| result.get("stopReason"))
        .and_then(|reason| reason.as_str())
    else {
        return PromptOutcome {
            stop_reason: "unknown".to_string(),
            provider_error: Some(format!(
                "session/prompt response missing result.stopReason: {frame}"
            )),
        };
    };
    PromptOutcome {
        stop_reason: stop_reason.to_string(),
        provider_error: stop_reason_failure(stop_reason),
    }
}

/// Classify an ACP `stopReason`. Only `end_turn` completed the turn's work.
/// `cancelled` is a Harness-requested outcome the caller records separately, so
/// it is not a provider failure. Every other reason (`max_tokens`, `refusal`,
/// `max_turn_requests`, or anything unknown) stopped the turn early: the
/// member's output is truncated or absent, so it must record a failed
/// provider_error round rather than a fabricated Handoff plus false completion.
fn stop_reason_failure(stop_reason: &str) -> Option<String> {
    match stop_reason {
        "end_turn" | "cancelled" | "canceled" => None,
        "max_tokens" => Some(
            "session/prompt stopped on `max_tokens`: the provider truncated the turn, so any \
             report it produced is incomplete"
                .to_string(),
        ),
        "refusal" => Some(
            "session/prompt stopped on `refusal`: the provider declined the turn and produced no \
             completed work"
                .to_string(),
        ),
        "max_turn_requests" => Some(
            "session/prompt stopped on `max_turn_requests`: the provider hit its request budget \
             before finishing the turn"
                .to_string(),
        ),
        other => Some(format!(
            "session/prompt stopped on unsupported stopReason `{other}`: Harness cannot claim the \
             turn completed"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn scripted_client() -> (KimiAcpClient, Sender<serde_json::Value>) {
        // The child is only a sink for the prompt/reverse-request response
        // writes. Scripted frames enter through the exact channels populated
        // by the production reader thread, making ordering deterministic
        // without sleeps or scheduler guesses.
        let mut child = Command::new("sh")
            .args(["-c", "cat >/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn scripted ACP sink");
        let stdin = child.stdin.take().expect("scripted ACP stdin");
        let (update_tx, updates) = channel();
        (
            KimiAcpClient {
                child,
                stdin,
                next_request_id: 2,
                pending: Arc::new(Mutex::new(HashMap::new())),
                updates,
                reader: None,
                stderr_tail: Arc::new(Mutex::new(String::new())),
                session_id: Some("scripted-session".to_string()),
                model: None,
                effort: None,
                effective_model: None,
                effective_effort: None,
                config_options: Vec::new(),
                provider_version: None,
            },
            update_tx,
        )
    }

    fn session_update(kind: &str) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "scripted-session",
                "update": {"sessionUpdate": kind}
            }
        })
    }

    fn provider_error_response() -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32000, "message": "scripted provider error"}
        })
    }

    fn terminal_success_response() -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"stopReason": "end_turn"}
        })
    }

    #[cfg(unix)]
    fn drive_scripted_prompt(
        client: &mut KimiAcpClient,
        response: Receiver<serde_json::Value>,
        on_accepted: &mut impl FnMut(&str) -> CliResult<()>,
        on_update: &mut impl FnMut(&serde_json::Value),
        on_request: &mut impl FnMut(&serde_json::Value) -> CliResult<serde_json::Value>,
    ) -> PromptOutcome {
        client
            .drive_prompt(
                (1, response),
                PromptTimeouts::production(Duration::from_secs(30)),
                on_accepted,
                on_update,
                on_request,
                &mut |_| Ok(()),
                &mut || Ok(PromptControl::Continue),
            )
            .expect("scripted prompt completes")
    }

    fn config_options() -> Vec<serde_json::Value> {
        serde_json::json!([
            {
                "id": "model",
                "currentValue": "kimi-code/k3",
                "options": [{"value": "kimi-code/k3"}]
            },
            {
                "id": "thinking",
                "currentValue": "high",
                "options": [{"value": "low"}, {"value": "high"}, {"value": "max"}]
            }
        ])
        .as_array()
        .expect("array")
        .clone()
    }

    #[test]
    fn kimi_acp_maps_neutral_effort_to_the_advertised_thinking_option() {
        let options = config_options();
        assert_eq!(
            current_config_value(&options, "thinking").as_deref(),
            Some("high")
        );
        assert_eq!(
            config_option_supports(&options, "thinking", "max"),
            Some(true)
        );
        assert_eq!(
            config_option_supports(&options, "thinking", "ultra"),
            Some(false)
        );
    }

    #[test]
    fn prompt_outcome_marks_error_frames_and_missing_stop_reason_as_provider_errors() {
        let normal = prompt_outcome(&serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "result": {"stopReason": "end_turn"}
        }));
        assert_eq!(normal.stop_reason, "end_turn");
        assert_eq!(normal.provider_error, None);

        let rejected = prompt_outcome(&serde_json::json!({
            "jsonrpc": "2.0", "id": 5,
            "error": {"code": -32000, "message": "provider API 403: usage limit reached"}
        }));
        assert_eq!(rejected.stop_reason, "error");
        assert_eq!(
            rejected.provider_error.as_deref(),
            Some("session/prompt rejected (code -32000): provider API 403: usage limit reached")
        );

        let malformed = prompt_outcome(&serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "result": {}
        }));
        assert_eq!(malformed.stop_reason, "unknown");
        assert!(malformed
            .provider_error
            .as_deref()
            .is_some_and(|error| error.contains("missing result.stopReason")));
    }

    #[test]
    fn prompt_outcome_ignores_a_null_error_key() {
        // Servers that serialize every field (serde without
        // skip_serializing_if, Python dataclasses.asdict) emit `error: null`
        // on success. `frame.get("error").is_some()` is true for that key, so
        // a non-null filter is what separates success from failure.
        let success = prompt_outcome(&serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "result": {"stopReason": "end_turn"}, "error": null
        }));
        assert_eq!(success.stop_reason, "end_turn");
        assert_eq!(success.provider_error, None);

        // The mirrored shape: a real error alongside a null result.
        let failure = prompt_outcome(&serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "result": null,
            "error": {"code": -32000, "message": "rate limited"}
        }));
        assert_eq!(failure.stop_reason, "error");
        assert!(failure.provider_error.is_some());
    }

    #[test]
    fn prompt_outcome_refuses_to_call_an_incomplete_stop_reason_a_success() {
        for (stop_reason, expected_fragment) in [
            ("max_tokens", "truncated the turn"),
            ("refusal", "declined the turn"),
            ("max_turn_requests", "request budget"),
            ("wat", "unsupported stopReason"),
        ] {
            let outcome = prompt_outcome(&serde_json::json!({
                "jsonrpc": "2.0", "id": 5, "result": {"stopReason": stop_reason}
            }));
            assert_eq!(outcome.stop_reason, stop_reason);
            assert!(
                outcome
                    .provider_error
                    .as_deref()
                    .is_some_and(|error| error.contains(expected_fragment)),
                "{stop_reason} must record a provider_error, got {:?}",
                outcome.provider_error
            );
        }
        // Harness-requested cancellation is recorded by the caller as a
        // cancelled round, not as a provider failure.
        for stop_reason in ["cancelled", "canceled"] {
            let outcome = prompt_outcome(&serde_json::json!({
                "jsonrpc": "2.0", "id": 5, "result": {"stopReason": stop_reason}
            }));
            assert_eq!(outcome.provider_error, None, "{stop_reason}");
        }
    }

    #[test]
    fn prompt_receipt_evidence_excludes_session_level_and_unknown_updates() {
        let session_level = [
            "available_commands_update",
            "current_mode_update",
            "config_option_update",
            "session_info_update",
            "usage_update",
            "future_session_state_update",
        ];
        let prompt_scoped = [
            "user_message_chunk",
            "agent_message_chunk",
            "agent_thought_chunk",
            "tool_call",
            "tool_call_update",
            "plan",
            "plan_update",
            "plan_removed",
        ];

        // Exercise the exact predicate repeatedly without timing or sleeps so
        // scheduler order cannot hide a regression in this acceptance gate.
        for _ in 0..200 {
            for kind in session_level {
                let frame = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {"sessionId": "s", "update": {"sessionUpdate": kind}}
                });
                assert!(!prompt_acceptance_evidence(&frame, "s"), "{kind}");
            }
            for kind in prompt_scoped {
                let frame = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {"sessionId": "s", "update": {"sessionUpdate": kind}}
                });
                assert!(prompt_acceptance_evidence(&frame, "s"), "{kind}");
                let mut wrong_session = frame.clone();
                wrong_session["params"]["sessionId"] = serde_json::json!("other");
                assert!(
                    !prompt_acceptance_evidence(&wrong_session, "s"),
                    "wrong-session {kind}"
                );
            }
            assert!(prompt_acceptance_evidence(
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 41,
                    "method": "session/request_permission",
                    "params": {"sessionId": "s"}
                }),
                "s"
            ));
            assert!(!prompt_acceptance_evidence(
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "session/cancel",
                    "params": {"sessionId": "s"}
                }),
                "s"
            ));
            assert!(!prompt_acceptance_evidence(
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 42,
                    "method": "session/request_permission",
                    "params": {"sessionId": "other"}
                }),
                "s"
            ));
            assert!(!prompt_acceptance_evidence(
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 43,
                    "method": "future/reverse_rpc",
                    "params": {"sessionId": "s"}
                }),
                "s"
            ));
        }
    }

    #[test]
    fn prompt_idle_activity_requires_a_well_formed_frame_for_the_active_session() {
        assert!(active_session_activity(
            &session_update("available_commands_update"),
            "scripted-session"
        ));
        assert!(active_session_activity(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "session/request_permission",
                "params": {"sessionId": "scripted-session"}
            }),
            "scripted-session"
        ));

        for frame in [
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": "another-session",
                    "update": {"sessionUpdate": "agent_message_chunk"}
                }
            }),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "session/request_permission",
                "params": {"sessionId": "another-session"}
            }),
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {"sessionId": "scripted-session", "update": {}}
            }),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "future/reverse_rpc",
                "params": {"sessionId": "scripted-session"}
            }),
        ] {
            assert!(
                !active_session_activity(&frame, "scripted-session"),
                "{frame}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn wrong_session_frame_flood_cannot_prevent_prompt_idle_timeout() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let (mut client, update_tx) = scripted_client();
        let (_response_tx, response) = channel::<serde_json::Value>();
        let stop = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(AtomicUsize::new(0));
        let producer_stop = Arc::clone(&stop);
        let producer_sent = Arc::clone(&sent);
        let producer = std::thread::spawn(move || {
            let mut id = 100_u64;
            while !producer_stop.load(Ordering::Relaxed) {
                let frame = if id.is_multiple_of(2) {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": "stale-session",
                            "update": {"sessionUpdate": "agent_message_chunk"}
                        }
                    })
                } else {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": "session/request_permission",
                        "params": {"sessionId": "stale-session"}
                    })
                };
                if update_tx.send(frame).is_err() {
                    break;
                }
                producer_sent.fetch_add(1, Ordering::Relaxed);
                id += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
        });

        let started = Instant::now();
        let result = client.drive_prompt(
            (1, response),
            PromptTimeouts {
                idle: Duration::from_millis(40),
                cancel_grace: Duration::from_millis(40),
            },
            &mut |_| panic!("wrong-session traffic must not publish a receipt"),
            &mut |_| panic!("wrong-session updates must not reach the callback"),
            &mut |_| panic!("wrong-session requests must not reach the callback"),
            &mut |_| panic!("wrong-session requests must not be written"),
            &mut || Ok(PromptControl::Continue),
        );
        stop.store(true, Ordering::Relaxed);
        producer.join().expect("join wrong-session producer");

        let error = match result {
            Ok(_) => panic!("wrong-session flood must not mask idle timeout"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("prompt idle"), "{error}");
        assert!(
            sent.load(Ordering::Relaxed) >= 10,
            "test must sustain a real wrong-session flood"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "test timeout path should be bounded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn scripted_session_only_update_then_provider_error_publishes_no_receipt() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(session_update("available_commands_update"))
            .expect("queue session update");
        response_tx
            .send(provider_error_response())
            .expect("queue provider error");

        let mut accepted = 0;
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |_| {
                accepted += 1;
                Ok(())
            },
            &mut |_| {},
            &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
        );

        assert!(outcome.provider_error.is_some());
        assert_eq!(accepted, 0);
    }

    #[cfg(unix)]
    #[test]
    fn scripted_plan_update_in_response_tail_publishes_receipt_exactly_once() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(session_update("plan_update"))
            .expect("queue plan update");
        response_tx
            .send(provider_error_response())
            .expect("queue provider error");

        let events = std::cell::RefCell::new(Vec::new());
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |receipt| {
                events.borrow_mut().push(format!("receipt:{receipt}"));
                Ok(())
            },
            &mut |update| {
                events.borrow_mut().push(format!(
                    "update:{}",
                    update["sessionUpdate"].as_str().unwrap_or("unknown")
                ));
            },
            &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
        );

        assert!(outcome.provider_error.is_some());
        assert_eq!(
            events.into_inner(),
            ["receipt:kimi-acp-prompt:1", "update:plan_update"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn scripted_plan_removed_before_provider_error_publishes_receipt_exactly_once() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(session_update("plan_removed"))
            .expect("queue plan removal");
        let mut response_tx = Some(response_tx);

        let events = std::cell::RefCell::new(Vec::new());
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |receipt| {
                events.borrow_mut().push(format!("receipt:{receipt}"));
                Ok(())
            },
            &mut |update| {
                events.borrow_mut().push(format!(
                    "update:{}",
                    update["sessionUpdate"].as_str().unwrap_or("unknown")
                ));
                response_tx
                    .take()
                    .expect("one terminal response")
                    .send(provider_error_response())
                    .expect("queue provider error after update");
            },
            &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
        );

        assert!(outcome.provider_error.is_some());
        assert_eq!(
            events.into_inner(),
            ["receipt:kimi-acp-prompt:1", "update:plan_removed"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn scripted_reverse_request_before_provider_error_publishes_receipt_exactly_once() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 41,
                "method": "session/request_permission",
                "params": {"sessionId": "scripted-session"}
            }))
            .expect("queue reverse request");
        let mut response_tx = Some(response_tx);

        let mut receipts = Vec::new();
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |receipt| {
                receipts.push(receipt.to_string());
                Ok(())
            },
            &mut |_| {},
            &mut |_| {
                response_tx
                    .take()
                    .expect("one terminal response")
                    .send(provider_error_response())
                    .expect("queue provider error after reverse request");
                Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}}))
            },
        );

        assert!(outcome.provider_error.is_some());
        assert_eq!(receipts, ["kimi-acp-prompt:1"]);
    }

    #[cfg(unix)]
    #[test]
    fn reverse_request_written_callback_runs_only_after_native_write() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 44,
                "method": "session/request_permission",
                "params": {"sessionId": "scripted-session"}
            }))
            .expect("queue reverse request");
        let mut response_tx = Some(response_tx);
        let events = std::cell::RefCell::new(Vec::new());

        let outcome = client
            .drive_prompt(
                (1, response),
                PromptTimeouts::production(Duration::from_secs(30)),
                &mut |_| Ok(()),
                &mut |_| {},
                &mut |_| {
                    events.borrow_mut().push("handler_returned");
                    response_tx
                        .take()
                        .expect("one terminal response")
                        .send(provider_error_response())
                        .expect("queue provider error");
                    Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}}))
                },
                &mut |request| {
                    assert_eq!(request["id"].as_u64(), Some(44));
                    events.borrow_mut().push("response_written");
                    Ok(())
                },
                &mut || Ok(PromptControl::Continue),
            )
            .expect("scripted prompt completes");

        assert!(outcome.provider_error.is_some());
        assert_eq!(
            events.into_inner(),
            ["handler_returned", "response_written"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn reverse_request_write_failure_never_publishes_written_callback() {
        let (mut client, update_tx) = scripted_client();
        client.child.kill().expect("kill scripted ACP sink");
        client.child.wait().expect("reap scripted ACP sink");
        let (_response_tx, response) = channel();
        update_tx
            .send(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 45,
                "method": "session/request_permission",
                "params": {"sessionId": "scripted-session"}
            }))
            .expect("queue reverse request");
        let mut written = 0;

        let result = client.drive_prompt(
            (1, response),
            PromptTimeouts::production(Duration::from_secs(30)),
            &mut |_| Ok(()),
            &mut |_| {},
            &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
            &mut |_| {
                written += 1;
                Ok(())
            },
            &mut || Ok(PromptControl::Continue),
        );

        assert!(
            result.is_err(),
            "broken native pipe must fail the response write"
        );
        assert_eq!(written, 0, "failed native write must not publish a receipt");
    }

    #[cfg(unix)]
    #[test]
    fn scripted_permission_request_for_another_session_publishes_no_receipt() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 42,
                "method": "session/request_permission",
                "params": {"sessionId": "another-session"}
            }))
            .expect("queue mismatched reverse request");
        response_tx
            .send(provider_error_response())
            .expect("queue provider error");

        let mut accepted = 0;
        let mut permission_callbacks = 0;
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |_| {
                accepted += 1;
                Ok(())
            },
            &mut |_| {},
            &mut |_| {
                permission_callbacks += 1;
                Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}}))
            },
        );

        assert!(outcome.provider_error.is_some());
        assert_eq!(accepted, 0);
        assert_eq!(permission_callbacks, 0);
    }

    #[cfg(unix)]
    #[test]
    fn scripted_unknown_reverse_method_publishes_no_receipt() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 43,
                "method": "future/reverse_rpc",
                "params": {"sessionId": "scripted-session"}
            }))
            .expect("queue unknown reverse request");
        response_tx
            .send(provider_error_response())
            .expect("queue provider error");

        let mut accepted = 0;
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |_| {
                accepted += 1;
                Ok(())
            },
            &mut |_| {},
            &mut |_| panic!("unknown reverse method must not reach permission callback"),
        );

        assert!(outcome.provider_error.is_some());
        assert_eq!(accepted, 0);
    }

    #[cfg(unix)]
    #[test]
    fn scripted_prompt_update_and_terminal_success_publish_receipt_exactly_once() {
        let (mut client, update_tx) = scripted_client();
        let (response_tx, response) = channel();
        update_tx
            .send(session_update("agent_message_chunk"))
            .expect("queue prompt update");
        response_tx
            .send(terminal_success_response())
            .expect("queue terminal success");

        let mut receipts = Vec::new();
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |receipt| {
                receipts.push(receipt.to_string());
                Ok(())
            },
            &mut |_| {},
            &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
        );

        assert_eq!(outcome.provider_error, None);
        assert_eq!(outcome.stop_reason, "end_turn");
        assert_eq!(receipts, ["kimi-acp-prompt:1"]);
    }

    #[cfg(unix)]
    #[test]
    fn scripted_terminal_success_without_updates_publishes_receipt_exactly_once() {
        let (mut client, _update_tx) = scripted_client();
        let (response_tx, response) = channel();
        response_tx
            .send(terminal_success_response())
            .expect("queue terminal success");

        let mut receipts = Vec::new();
        let outcome = drive_scripted_prompt(
            &mut client,
            response,
            &mut |receipt| {
                receipts.push(receipt.to_string());
                Ok(())
            },
            &mut |_| {},
            &mut |_| Ok(serde_json::json!({"outcome": {"outcome": "cancelled"}})),
        );

        assert_eq!(outcome.provider_error, None);
        assert_eq!(outcome.stop_reason, "end_turn");
        assert_eq!(receipts, ["kimi-acp-prompt:1"]);
    }
}
