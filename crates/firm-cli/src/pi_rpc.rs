//! Pi RPC client for persistent Agent Team Members.
//!
//! One [`PiRpcClient`] owns one `pi --mode rpc` child process: strict JSONL
//! (LF-delimited) over stdin/stdout. The wire dance is:
//!
//! 1. Spawn `pi --mode rpc [--model] [--thinking] --session-dir <dir>`
//!    with `--no-context-files` and `--no-extensions`. All member instructions
//!    belong in the prompt text.
//! 2. `get_state` → extract `sessionFile` (absolute path, stored as
//!    `native_session_id`) + `autoCompactionEnabled` (disable immediately).
//! 3. `prompt` → streams `agent_start/end/settled`, `turn_start/end`
//!    (with full message), `tool_execution_start/update/end` and
//!    `message_update` notifications; finishes with `agent_settled`.
//! 4. `abort` — cancels the in-flight prompt. A wedged process is killed as a
//!    fallback. Host Close is distinct and terminates the process group.
//!
//! Pi's `--session <path>` CLI flag is used to resume from a previous
//! session's JSONL file. The file path is the `native_session_id`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::{kill_worker_tree, CliError, CliResult};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) struct PiRpcClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    next_request_id: u64,
    /// Response waiters: string request id → oneshot sender.
    pending: Arc<Mutex<HashMap<String, Sender<serde_json::Value>>>>,
    /// Streaming events / notifications from the reader thread.
    incoming: Receiver<serde_json::Value>,
    reader: Option<JoinHandle<()>>,
    stderr_tail: Arc<Mutex<String>>,
    /// Absolute path to the pi session JSONL file (native_session_id).
    session_file: String,
}

pub(crate) struct PiSpawnOptions<'a> {
    pub cwd: &'a Path,
    pub model: Option<&'a str>,
    pub resume_session_file: Option<&'a str>,
    pub session_dir: &'a Path,
    pub member_name: &'a str,
    pub collaboration_env: &'a [(String, String)],
    /// Compiled permission ceiling (`runtime_adapter::pi_tools_allowlist_for_ceiling`).
    /// `Some` passes `--tools <csv>` to the process; `None` runs the Pi
    /// default toolset and the profile must record
    /// `security_enforcement_locus = none_verified`.
    pub tools: Option<&'a str>,
}

pub(crate) struct PiTurnOutcome {
    pub final_text: String,
    pub interrupted: bool,
    pub close_requested_by_harness: bool,
    pub tool_call_count: u32,
}

fn stderr_suffix(tail: &Arc<Mutex<String>>) -> String {
    let t = tail.lock().unwrap_or_else(|error| error.into_inner());
    let trimmed = t.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(
            "; stderr: {}",
            trimmed
                .chars()
                .rev()
                .take(1200)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
    }
}

impl PiRpcClient {
    pub(crate) fn spawn(pi_bin: &str, options: PiSpawnOptions<'_>) -> CliResult<Self> {
        if let Some(session_file) = options.resume_session_file {
            ensure_session_has_no_persisted_thinking(Path::new(session_file))?;
        }
        let mut command = Command::new(pi_bin);
        command
            .arg("--mode")
            .arg("rpc")
            .arg("--session-dir")
            .arg(options.session_dir)
            .arg("--no-context-files")
            .arg("--no-extensions")
            // Pi persists provider thinking blocks in its native JSONL session
            // and replays that file on --session. The Harness product contract
            // permits thinking only in the transient sanitized live channel, so
            // the persistent Team adapter must force provider thinking off.
            .arg("--thinking")
            .arg("off")
            .current_dir(options.cwd)
            .envs(options.collaboration_env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(model) = options.model {
            command.arg("--model").arg(model);
        }
        if let Some(tools) = options.tools {
            // Real adapter-level permission enforcement: the allowlist is
            // compiled into the process, not just mapped and dropped.
            command.arg("--tools").arg(tools);
        }
        if let Some(session_file) = options.resume_session_file {
            command.arg("--session").arg(session_file);
        }

        #[cfg(unix)]
        {
            command.process_group(0);
        }

        let mut child = command.spawn().map_err(|error| {
            CliError::Usage(format!(
                "failed to spawn pi rpc for {}: {error}",
                options.member_name
            ))
        })?;

        let stdin = BufWriter::new(
            child
                .stdin
                .take()
                .ok_or_else(|| CliError::Usage("pi rpc stdin unavailable".to_string()))?,
        );
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CliError::Usage("pi rpc stdout unavailable".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CliError::Usage("pi rpc stderr unavailable".to_string()))?;

        let pending: Arc<Mutex<HashMap<String, Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_reader = Arc::clone(&pending);
        let (incoming_tx, incoming) = channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(frame) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                // Route by "type": "response" frames go to pending waiters;
                // everything else (events/notifications) goes to the events channel.
                if frame.get("type").and_then(|v| v.as_str()) == Some("response") {
                    if let Some(id) = frame.get("id").and_then(|v| v.as_str()) {
                        if let Some(sender) = pending_reader
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .remove(id)
                        {
                            let _ = sender.send(frame);
                            continue;
                        }
                    }
                }
                if incoming_tx.send(frame).is_err() {
                    break;
                }
            }
            pending_reader
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
        });

        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let stderr_writer = Arc::clone(&stderr_tail);
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            *stderr_writer
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = text;
        });

        let mut client = Self {
            child,
            stdin,
            next_request_id: 0,
            pending,
            incoming,
            reader: Some(reader),
            stderr_tail,
            session_file: String::new(),
        };

        // Handshake: get_state to discover session file.
        let state =
            client.request_blocking("get_state", serde_json::json!({}), HANDSHAKE_TIMEOUT)?;
        let data = state.get("data").ok_or_else(|| {
            CliError::Usage(format!(
                "pi get_state response missing data{}",
                stderr_suffix(&client.stderr_tail)
            ))
        })?;
        client.session_file = data
            .get("sessionFile")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "pi get_state response missing sessionFile{}",
                    stderr_suffix(&client.stderr_tail)
                ))
            })?;

        // Disable auto-compaction immediately so long prompts aren't
        // interrupted by unexpected compactions.
        if data
            .get("autoCompactionEnabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            client.request_blocking(
                "set_auto_compaction",
                serde_json::json!({"enabled": false}),
                HANDSHAKE_TIMEOUT,
            )?;
        }

        Ok(client)
    }

    pub(crate) fn session_file(&self) -> &str {
        &self.session_file
    }

    pub(crate) fn ensure_transport_alive(&mut self) -> CliResult<()> {
        let reader_ended = self.reader.as_ref().is_some_and(JoinHandle::is_finished);
        let child_ended = self.child.try_wait().map_err(|error| {
            CliError::Usage(format!("failed to inspect pi rpc process: {error}"))
        })?;
        if reader_ended || child_ended.is_some() {
            return Err(CliError::Usage(format!(
                "pi rpc transport disconnected{}",
                stderr_suffix(&self.stderr_tail)
            )));
        }
        Ok(())
    }

    /// Compile one cycle's control intents into Pi RPC frames. Steer bodies
    /// become `steer` frames (current-cycle injection at Pi's tool-call
    /// boundary); close/interrupt become `abort`. Both are fire-and-forget:
    /// acceptance is observable in the native session, and the Harness-side
    /// control reply already acknowledged compilation at this boundary.
    fn apply_cycle_control(&mut self, control: &crate::runtime_adapter::CycleControl) {
        for inject in &control.injects {
            let _ = self.write_frame(&serde_json::json!({
                "type": "steer",
                "message": inject,
            }));
        }
        if control.close || control.interrupt {
            let _ = self.write_frame(&serde_json::json!({
                "type": "abort"
            }));
        }
    }

    /// Queue input at Pi's native session boundary (`follow_up`): consumed by
    /// the current session-level run before it fully settles. This is NOT the
    /// Harness message queue — ordinary TeamMessages stay durable-side by
    /// design (DOC-89 §13.1), so production has no caller yet; the RPC-level
    /// unit test is the conformance consumer.
    #[allow(dead_code)]
    pub(crate) fn follow_up(&mut self, text: &str) -> CliResult<serde_json::Value> {
        self.request_blocking(
            "follow_up",
            serde_json::json!({"message": text}),
            HANDSHAKE_TIMEOUT,
        )
    }

    /// Point-in-time native queue observation (steering/follow-up mode and
    /// pending message count from `get_state`). Observation only; it is not a
    /// durable Harness fact. Consumed by the RPC-level unit test today.
    #[allow(dead_code)]
    pub(crate) fn queue_snapshot(&mut self) -> CliResult<serde_json::Value> {
        let state = self.request_blocking("get_state", serde_json::json!({}), HANDSHAKE_TIMEOUT)?;
        let data = state
            .get("data")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        Ok(serde_json::json!({
            "steering_mode": data.get("steeringMode"),
            "follow_up_mode": data.get("followUpMode"),
            "pending_message_count": data.get("pendingMessageCount"),
            "is_streaming": data.get("isStreaming"),
        }))
    }

    /// Send a prompt and block until `agent_settled`.
    ///
    /// `on_event` receives every non-response event so the orchestrator can
    /// project live tool activity. `poll_control` returns a
    /// [`CycleControl`](crate::runtime_adapter::CycleControl): explicit Steer
    /// bodies are compiled into `steer` frames at this boundary, and
    /// close/interrupt sends `abort` while the loop continues reading until
    /// `agent_settled`.
    ///
    /// Returns `PiTurnOutcome` with `final_text` extracted from the last
    /// `turn_end.message` content blocks.
    pub(crate) fn prompt<F, C>(
        &mut self,
        text: &str,
        idle_timeout: Duration,
        mut on_event: F,
        mut poll_control: C,
    ) -> CliResult<PiTurnOutcome>
    where
        F: FnMut(&serde_json::Value),
        C: FnMut() -> crate::runtime_adapter::CycleControl,
    {
        self.request_blocking(
            "prompt",
            serde_json::json!({"message": text}),
            HANDSHAKE_TIMEOUT,
        )?;

        let mut last_idle = Instant::now();
        let mut interrupted = false;
        let mut close_requested = false;
        let mut tool_call_count: u32 = 0;
        let mut final_text = String::new();

        loop {
            match self.incoming.recv_timeout(Duration::from_millis(500)) {
                Ok(frame) => {
                    last_idle = Instant::now();
                    let event_type = frame.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    // Check for control intents (close/interrupt/steer).
                    let control = poll_control();
                    if control.close || control.interrupt {
                        interrupted = true;
                        close_requested = control.close;
                    }
                    self.apply_cycle_control(&control);

                    match event_type {
                        "agent_settled" => break,
                        "tool_execution_start" => {
                            tool_call_count = tool_call_count.saturating_add(1);
                            on_event(&frame);
                        }
                        "turn_end" => {
                            // Extract text from the full message — replaces, since
                            // only the LAST turn's text matters for the report.
                            let extracted = Self::extract_turn_end_text(&frame);
                            if !extracted.trim().is_empty() {
                                final_text = extracted;
                            }
                            on_event(&frame);
                        }
                        _ => on_event(&frame),
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Poll control intents even during idle.
                    let control = poll_control();
                    self.apply_cycle_control(&control);
                    if control.close || control.interrupt {
                        interrupted = true;
                        close_requested = control.close;
                    }
                    if last_idle.elapsed() > idle_timeout {
                        // Wedged — abort, then kill.
                        let _ = self.write_frame(&serde_json::json!({
                            "type": "abort"
                        }));
                        // Give a short grace window, then kill the process tree.
                        std::thread::sleep(Duration::from_secs(2));
                        kill_worker_tree(&mut self.child);
                        return Err(CliError::Usage(format!(
                            "pi rpc prompt timed out after {}s idle{}",
                            idle_timeout.as_secs(),
                            stderr_suffix(&self.stderr_tail)
                        )));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Reader thread exited — transport dead.
                    let status = self.child.try_wait().ok().flatten();
                    return Err(CliError::Usage(format!(
                        "pi rpc transport disconnected (child: {}){}",
                        status
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                        stderr_suffix(&self.stderr_tail)
                    )));
                }
            }
        }

        // Drain any remaining events until the channel is empty
        // (non-blocking) in case agent_settled was preceded by events.
        while let Ok(frame) = self.incoming.try_recv() {
            let event_type = frame.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if event_type == "tool_execution_start" {
                tool_call_count = tool_call_count.saturating_add(1);
            }
            if event_type == "turn_end" {
                let extracted = Self::extract_turn_end_text(&frame);
                if !extracted.trim().is_empty() {
                    final_text = extracted;
                }
                on_event(&frame);
            } else if event_type != "agent_settled" {
                on_event(&frame);
            }
        }
        Ok(PiTurnOutcome {
            final_text,
            interrupted,
            close_requested_by_harness: close_requested,
            tool_call_count,
        })
    }

    /// Extract text from the last `turn_end.message` content blocks.
    pub(crate) fn extract_turn_end_text(frame: &serde_json::Value) -> String {
        let message = match frame.get("message") {
            Some(m) => m,
            None => return String::new(),
        };
        let content = match message.get("content") {
            Some(serde_json::Value::Array(blocks)) => blocks,
            _ => return String::new(),
        };
        let mut text = String::new();
        for block in content {
            if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(t);
                }
            }
        }
        text
    }

    /// Project a pi tool execution event to a typed, volatile live activity.
    pub(crate) fn project_live(
        event: &serde_json::Value,
    ) -> Option<(crate::provider_event_api::LiveProviderActivityKind, String)> {
        match event.get("type").and_then(|v| v.as_str()) {
            Some("tool_execution_start") => {
                let tool = event.get("toolName").and_then(|v| v.as_str())?;
                let summary = match tool {
                    "bash" => Some("Bash running".to_string()),
                    "edit" => Some("Editing file".to_string()),
                    "write" => Some("Writing file".to_string()),
                    "read" => Some("Reading file".to_string()),
                    "grep" => Some("Grep".to_string()),
                    "find" => Some("Find".to_string()),
                    "ls" => Some("Ls".to_string()),
                    _ => Some("Tool running".to_string()),
                }?;
                Some((
                    crate::provider_event_api::LiveProviderActivityKind::ToolStarted,
                    summary,
                ))
            }
            Some("tool_execution_end") => {
                let failed = event
                    .get("isError")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                Some((
                    if failed {
                        crate::provider_event_api::LiveProviderActivityKind::ToolFailed
                    } else {
                        crate::provider_event_api::LiveProviderActivityKind::ToolCompleted
                    },
                    if failed {
                        "tool failed".to_string()
                    } else {
                        "tool completed".to_string()
                    },
                ))
            }
            _ => None,
        }
    }

    fn request_blocking(
        &mut self,
        command: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> CliResult<serde_json::Value> {
        self.next_request_id += 1;
        let id = format!("pi-rpc-{}", self.next_request_id);
        let (tx, rx) = channel();
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id.clone(), tx);

        let mut frame = params;
        frame["id"] = serde_json::Value::String(id.clone());
        frame["type"] = serde_json::Value::String(command.to_string());
        self.write_frame(&frame)?;

        let frame = rx.recv_timeout(timeout).map_err(|error| {
            self.pending
                .lock()
                .unwrap_or_else(|lock_error| lock_error.into_inner())
                .remove(&id);
            let failure = match error {
                RecvTimeoutError::Timeout => "timed out",
                RecvTimeoutError::Disconnected => "transport disconnected",
            };
            CliError::Usage(format!(
                "pi rpc {command} {failure}{}",
                stderr_suffix(&self.stderr_tail)
            ))
        })?;

        let success = frame
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            let detail = frame
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(CliError::Usage(format!(
                "pi rpc {command} failed: {detail}{}",
                stderr_suffix(&self.stderr_tail)
            )));
        }
        Ok(frame)
    }

    fn write_frame(&mut self, frame: &serde_json::Value) -> CliResult<()> {
        serde_json::to_writer(&mut self.stdin, frame)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Provider-neutral adapter binding
// ---------------------------------------------------------------------------

/// The Pi binding of [`TeamRuntimeAdapter`](crate::runtime_adapter::TeamRuntimeAdapter):
/// a live `pi --mode rpc` child as the process-local RuntimeHandle. Durable
/// identity stays with AgentSession/NativeSessionRef; this handle is
/// disposable and never an authority.
pub(crate) struct PiTeamRuntime {
    client: PiRpcClient,
}

impl PiTeamRuntime {
    pub(crate) fn new(client: PiRpcClient) -> Self {
        Self { client }
    }
}

impl crate::runtime_adapter::TeamRuntimeAdapter for PiTeamRuntime {
    fn provider(&self) -> &'static str {
        "pi"
    }

    fn display_name(&self) -> &'static str {
        "Pi"
    }

    fn capability_bindings() -> Vec<crate::runtime_adapter::CapabilityBinding> {
        use crate::runtime_adapter::{CapabilityBinding, CapabilityStatus};
        vec![
            CapabilityBinding {
                capability: "open_or_resume_native_session",
                status: CapabilityStatus::Supported,
                evidence: "pi --mode rpc --session <file>; tests/pi_team_member.rs".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "start_cycle",
                status: CapabilityStatus::Supported,
                evidence: "prompt RPC → agent_settled; tests/pi_team_member.rs journey".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inject_current_cycle",
                status: CapabilityStatus::Supported,
                evidence: "steer RPC frame compiled at the cycle control boundary".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "queue_at_native_boundary",
                status: CapabilityStatus::Supported,
                evidence: "follow_up RPC; ordinary Harness Messages never use it".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "interrupt_current_cycle",
                status: CapabilityStatus::Supported,
                evidence: "abort RPC + agent_settled observation".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inspect_continuation",
                status: CapabilityStatus::Unsupported,
                evidence: "Pi has no native Goal/continuation object".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inhibit_continuation",
                status: CapabilityStatus::Unsupported,
                evidence: "Pi has no native Goal/continuation object".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "resume_continuation",
                status: CapabilityStatus::Unsupported,
                evidence: "Pi has no native Goal/continuation object".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "observe_native_queue",
                status: CapabilityStatus::Supported,
                evidence: "get_state steering/followUp/pendingMessageCount snapshot".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inspect_effect",
                status: CapabilityStatus::Degraded,
                evidence: "native JSONL entry observation only; no semantic effect proof".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "reconcile_effect",
                status: CapabilityStatus::Unsupported,
                evidence: "no provider-side operation id to reconcile against".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "permission_enforcement",
                status: CapabilityStatus::Supported,
                evidence: "ceiling compiled to --tools allowlist at spawn".into(),
                security_enforcement_locus: Some(
                    "adapter tool allowlist (restricted) / none_verified (full access)".into(),
                ),
            },
        ]
    }

    fn ensure_alive(&mut self) -> CliResult<()> {
        self.client.ensure_transport_alive()
    }

    fn native_session_locator(&self) -> &str {
        self.client.session_file()
    }

    fn native_locator_kind(&self) -> &'static str {
        "pi_session"
    }

    fn run_cycle(
        &mut self,
        input: &str,
        idle_timeout: Duration,
        on_event: &mut dyn FnMut(&serde_json::Value),
        poll_control: &mut dyn FnMut() -> crate::runtime_adapter::CycleControl,
    ) -> CliResult<crate::runtime_adapter::ExecutionCycleOutcome> {
        let outcome =
            self.client
                .prompt(input, idle_timeout, &mut *on_event, &mut *poll_control)?;
        Ok(crate::runtime_adapter::ExecutionCycleOutcome {
            final_text: outcome.final_text,
            interrupted: outcome.interrupted,
            close_requested_by_harness: outcome.close_requested_by_harness,
            tool_call_count: outcome.tool_call_count,
        })
    }

    fn project_live(
        event: &serde_json::Value,
    ) -> Option<(crate::provider_event_api::LiveProviderActivityKind, String)> {
        PiRpcClient::project_live(event)
    }

    fn native_control<'a>(
        close: &'a mut bool,
        interrupt: &'a mut bool,
    ) -> Box<dyn crate::provider_adapter::ProviderNativeControl + 'a> {
        Box::new(crate::provider_adapter::PiNativeControl { close, interrupt })
    }

    fn supports_inject_current_cycle(&self) -> bool {
        true
    }

    fn supports_native_boundary_queue(&self) -> bool {
        true
    }
}

fn ensure_session_has_no_persisted_thinking(path: &Path) -> CliResult<()> {
    let file = std::fs::File::open(path).map_err(|error| {
        CliError::Usage(format!(
            "failed to inspect Pi session {} before resume: {error}",
            path.display()
        ))
    })?;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| {
            CliError::Usage(format!(
                "failed to inspect Pi session {} before resume: {error}",
                path.display()
            ))
        })?;
        let value = serde_json::from_str::<serde_json::Value>(&line).map_err(|error| {
            CliError::Usage(format!(
                "refusing to resume Pi session {} because line {} is not valid JSON: {error}",
                path.display(),
                index + 1
            ))
        })?;
        if value_contains_persisted_thinking(&value) {
            return Err(CliError::Usage(format!(
                "refusing to resume Pi session {} because line {} contains persisted provider thinking; start a fresh Pi ProviderRuntimeProjection with thinking disabled",
                path.display(),
                index + 1
            )));
        }
    }
    Ok(())
}

fn value_contains_persisted_thinking(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(value_contains_persisted_thinking),
        serde_json::Value::Object(object) => {
            object.get("type").and_then(serde_json::Value::as_str) == Some("thinking")
                || object.contains_key("thinkingSignature")
                || object.values().any(value_contains_persisted_thinking)
        }
        _ => false,
    }
}

impl Drop for PiRpcClient {
    fn drop(&mut self) {
        // Kill the process group, then join reader.
        kill_worker_tree(&mut self.child);
        // Give the reader thread a moment to notice EOF and exit.
        if let Some(handle) = self.reader.take() {
            // Don't block indefinitely; the process kill above should make
            // stdout close and unblock the reader.
            let _ = std::thread::Builder::new()
                .name("pi-rpc-reader-waiter".into())
                .spawn(move || {
                    let _ = handle.join();
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_session_has_no_persisted_thinking, value_contains_persisted_thinking, PiRpcClient,
    };

    /// Spawn a minimal fake `pi --mode rpc` shim and exercise the RPC-level
    /// adapter surface: handshake, follow_up acknowledgement, queue
    /// observation, and the --tools permission compilation in the spawn argv.
    #[test]
    fn follow_up_queue_snapshot_and_tools_compilation() {
        let dir = std::env::temp_dir().join(format!(
            "pi-rpc-rpc-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let session_file = dir.join("session.jsonl");
        std::fs::write(&session_file, "{\"type\":\"agent_start\"}\n").unwrap();
        let args_marker = dir.join("argv.json");
        let shim = dir.join("pi");
        let script = format!(
            r##"#!/usr/bin/env python3
import sys, json, os
with open('{args_marker}', 'w') as f:
    json.dump(sys.argv[1:], f)
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        cmd = json.loads(line)
    except json.JSONDecodeError:
        continue
    t = cmd.get('type', '')
    cid = cmd.get('id', '')
    if t == 'get_state':
        resp = {{'id': cid, 'type': 'response', 'command': 'get_state', 'success': True,
                 'data': {{'sessionFile': '{session_file}', 'autoCompactionEnabled': False,
                           'steeringMode': 'one-at-a-time', 'followUpMode': 'one-at-a-time',
                           'pendingMessageCount': 2, 'isStreaming': False}}}}
    elif t == 'follow_up':
        resp = {{'id': cid, 'type': 'response', 'command': 'follow_up', 'success': True}}
    else:
        resp = {{'id': cid, 'type': 'response', 'command': t, 'success': True}}
    print(json.dumps(resp), flush=True)
"##,
            args_marker = args_marker.display(),
            session_file = session_file.display(),
        );
        std::fs::write(&shim, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&shim).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&shim, perms).unwrap();
        }

        let mut client = PiRpcClient::spawn(
            shim.to_str().unwrap(),
            super::PiSpawnOptions {
                cwd: &dir,
                model: None,
                resume_session_file: None,
                session_dir: &dir,
                member_name: "rpc-test",
                collaboration_env: &[],
                tools: Some("read,grep,find,ls"),
            },
        )
        .expect("spawn shim");

        // Permission compilation proof: the allowlist is in the process argv.
        let argv: Vec<String> =
            serde_json::from_str(&std::fs::read_to_string(&args_marker).unwrap()).unwrap();
        let tools_pos = argv.iter().position(|arg| arg == "--tools");
        assert_eq!(
            tools_pos.map(|pos| argv[pos + 1].as_str()),
            Some("read,grep,find,ls"),
            "restricted ceiling must compile to --tools in the spawn argv: {argv:?}"
        );

        let ack = client.follow_up("queued at the native boundary").unwrap();
        assert_eq!(ack.get("success").and_then(|v| v.as_bool()), Some(true));

        let snapshot = client.queue_snapshot().unwrap();
        assert_eq!(
            snapshot["pending_message_count"].as_u64(),
            Some(2),
            "queue observation must surface the native pending count: {snapshot}"
        );
        assert_eq!(snapshot["steering_mode"].as_str(), Some("one-at-a-time"));

        drop(client);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_persisted_thinking_blocks_without_rejecting_level_metadata() {
        assert!(value_contains_persisted_thinking(&serde_json::json!({
            "type": "message",
            "message": {"content": [{"type": "thinking", "thinking": "private"}]}
        })));
        assert!(value_contains_persisted_thinking(&serde_json::json!({
            "type": "message",
            "message": {"content": [{"type": "text", "thinkingSignature": "sig"}]}
        })));
        assert!(!value_contains_persisted_thinking(&serde_json::json!({
            "type": "thinking_level_change",
            "thinkingLevel": "off"
        })));
    }

    #[test]
    fn rejects_a_native_session_that_would_replay_thinking() {
        let dir = std::env::temp_dir().join(format!(
            "harness-pi-rpc-thinking-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("session.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"content\":[{\"type\":\"thinking\",\"thinking\":\"private\"}]}}\n",
        )
        .expect("write session");
        let error = ensure_session_has_no_persisted_thinking(&path).unwrap_err();
        assert!(error.to_string().contains("persisted provider thinking"));
        std::fs::remove_dir_all(dir).expect("remove temp dir");
    }

    #[test]
    fn live_tool_projection_omits_unknown_names_arguments_and_paths() {
        let event = serde_json::json!({
            "type":"tool_execution_start",
            "toolName":"secret-plugin-name",
            "args":{"command":"print-secret", "path":"/private/member/file"}
        });
        let (_, summary) = PiRpcClient::project_live(&event).expect("tool activity");
        assert_eq!(summary, "Tool running");
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("/private"));
    }
}
