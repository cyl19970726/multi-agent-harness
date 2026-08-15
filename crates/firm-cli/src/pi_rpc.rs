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
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::{kill_worker_tree, CliError, CliResult};
use harness_core::agentfirm_api::PermissionCeiling;

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
    /// Actual spawn policy, retained as process-local evidence for conditional
    /// quiescence claims. Durable permission intent alone cannot prove argv.
    permission_ceiling: PermissionCeiling,
    tools_allowlist: Option<String>,
    last_observation: Option<crate::runtime_adapter::RuntimeObservation>,
    released: bool,
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
    /// Canonical requested ceiling. Spawn admission validates this against
    /// the actual argv; a restricted ceiling can never degrade to an
    /// unrestricted Pi launch.
    pub permission_ceiling: PermissionCeiling,
}

pub(crate) struct PiTurnOutcome {
    pub final_text: String,
    pub interrupted: bool,
    pub close_requested_by_harness: bool,
    pub tool_call_count: u32,
    pub input_acceptance_receipt: crate::runtime_adapter::ControlTransportReceipt,
    pub control_receipts: Vec<crate::runtime_adapter::ControlTransportReceipt>,
    pub terminal_observation: crate::runtime_adapter::RuntimeObservation,
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

/// Pi 0.84.2 persists each native session entry with synchronous
/// `appendFileSync` / `writeFileSync`, but those calls do not establish a
/// durable-storage acknowledgement.  After the provider has reported
/// `agent_settled` and a subsequent `get_state` reports idle, explicitly sync
/// both the JSONL inode and its parent directory.  Merely finding the path is
/// never accepted as flush evidence.
fn confirm_pi_session_flush(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err(format!(
            "Pi native session path is not absolute: {}",
            path.display()
        ));
    }
    let link_metadata = std::fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Pi native session metadata is unavailable at {}: {error}",
            path.display()
        )
    })?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(format!(
            "Pi native session flush requires a regular non-symlink file: {}",
            path.display()
        ));
    }
    if link_metadata.len() == 0 {
        return Err(format!(
            "Pi native session JSONL is empty at {}",
            path.display()
        ));
    }

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|error| format!("failed to open Pi native session for sync: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("failed to sync Pi native session bytes: {error}"))?;
    file.seek(SeekFrom::End(-1))
        .map_err(|error| format!("failed to inspect Pi native session line boundary: {error}"))?;
    let mut final_byte = [0_u8; 1];
    file.read_exact(&mut final_byte)
        .map_err(|error| format!("failed to read Pi native session line boundary: {error}"))?;
    if final_byte[0] != b'\n' {
        return Err(format!(
            "Pi native session has an incomplete final JSONL record at {}",
            path.display()
        ));
    }

    let parent = path.parent().ok_or_else(|| {
        format!(
            "Pi native session has no parent directory: {}",
            path.display()
        )
    })?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("failed to sync Pi native session directory: {error}"))?;
    let after = std::fs::symlink_metadata(path)
        .map_err(|error| format!("failed to restat Pi native session after sync: {error}"))?;
    if after.len() != link_metadata.len() || !after.is_file() {
        return Err(format!(
            "Pi native session changed while flush was being confirmed: {}",
            path.display()
        ));
    }
    Ok(format!(
        "Pi 0.84.2 synchronous JSONL boundary observed; file+directory sync_all confirmed {} bytes at {}",
        after.len(),
        path.display()
    ))
}

impl PiRpcClient {
    pub(crate) fn spawn(pi_bin: &str, options: PiSpawnOptions<'_>) -> CliResult<Self> {
        crate::runtime_adapter::admit_pi_permission_ceiling(
            options.permission_ceiling,
            options.tools,
        )?;
        if let Some(session_file) = options.resume_session_file {
            if !Path::new(session_file).is_file() {
                return Err(CliError::Usage(format!(
                    "PI_NATIVE_SESSION_MISSING: refusing to replace missing resume session {session_file} with a fresh session"
                )));
            }
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
            permission_ceiling: options.permission_ceiling,
            tools_allowlist: options.tools.map(str::to_string),
            last_observation: None,
            released: false,
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
        client.last_observation = Some(Self::observation_from_state(data, true, false));

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

    fn observation_from_state(
        data: &serde_json::Value,
        process_alive: bool,
        settled_boundary_observed: bool,
    ) -> crate::runtime_adapter::RuntimeObservation {
        crate::runtime_adapter::RuntimeObservation {
            transport_alive: process_alive,
            process_alive,
            is_streaming: data.get("isStreaming").and_then(|value| value.as_bool()),
            pending_message_count: data
                .get("pendingMessageCount")
                .and_then(|value| value.as_u64()),
            steering_mode: data
                .get("steeringMode")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            follow_up_mode: data
                .get("followUpMode")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            settled_boundary_observed,
        }
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

    /// Compile one cycle's control intents into Pi request/response commands.
    /// Pi 0.84 steer and abort responses are transport receipts only. A
    /// Steer caller is answered after its matching response succeeds; abort
    /// still needs `agent_settled` plus a post-abort state observation before
    /// the durable RuntimeCommand can settle.
    fn apply_cycle_control(
        &mut self,
        control: &mut crate::runtime_adapter::CycleControl,
        on_steer_result: &mut dyn FnMut(
            &crate::runtime_adapter::PendingSteer,
            &crate::runtime_adapter::SteerProviderResult,
        ) -> CliResult<()>,
    ) -> CliResult<Vec<crate::runtime_adapter::ControlTransportReceipt>> {
        if let Some(error) = control.fatal_error.take() {
            return Err(CliError::Usage(error));
        }
        let mut receipts = Vec::new();
        let mut injects = std::mem::take(&mut control.injects).into_iter();
        while let Some(pending) = injects.next() {
            match self.request_blocking(
                "steer",
                serde_json::json!({"message": pending.content.clone()}),
                HANDSHAKE_TIMEOUT,
            ) {
                Ok(response) => {
                    let response_id = response
                        .get("id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string);
                    let response_id = response_id.ok_or_else(|| {
                        CliError::Usage(
                            "PI_STEER_RECEIPT_UNKNOWN: successful steer response had no id"
                                .to_string(),
                        )
                    })?;
                    let receipt = crate::runtime_adapter::ControlTransportReceipt {
                        command: "steer".to_string(),
                        response_id: Some(response_id.clone()),
                        success: true,
                    };
                    // Durable settlement happens before the API caller sees
                    // success. A provider transport receipt without a settled
                    // RuntimeCommand must never escape as `steer_accepted`.
                    on_steer_result(
                        &pending,
                        &crate::runtime_adapter::SteerProviderResult::Acknowledged(receipt.clone()),
                    )?;
                    let mut reply = pending.success_reply;
                    reply["provider_response_id"] = response_id.into();
                    let _ = pending.reply.send(Ok(reply));
                    receipts.push(receipt);
                }
                Err(error) => {
                    let detail = format!(
                        "PI_STEER_RECEIPT_UNKNOWN: provider did not acknowledge steer: {error}"
                    );
                    on_steer_result(
                        &pending,
                        &crate::runtime_adapter::SteerProviderResult::Unknown(detail.clone()),
                    )?;
                    let _ = pending.reply.send(Err(CliError::Usage(detail.clone())));
                    for undispatched in injects {
                        let not_applied = format!(
                            "PI_STEER_NOT_DISPATCHED: an earlier steer failed before this command: {detail}"
                        );
                        on_steer_result(
                            &undispatched,
                            &crate::runtime_adapter::SteerProviderResult::NotApplied(
                                not_applied.clone(),
                            ),
                        )?;
                        let _ = undispatched.reply.send(Err(CliError::Usage(not_applied)));
                    }
                    return Err(CliError::Usage(detail));
                }
            }
        }
        if control.close || control.interrupt {
            let response =
                self.request_blocking("abort", serde_json::json!({}), HANDSHAKE_TIMEOUT)?;
            receipts.push(crate::runtime_adapter::ControlTransportReceipt {
                command: "abort".to_string(),
                response_id: response
                    .get("id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                success: true,
            });
        }
        Ok(receipts)
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
        let observation = self.observe_runtime(false)?;
        Ok(serde_json::json!({
            "steering_mode": observation.steering_mode,
            "follow_up_mode": observation.follow_up_mode,
            "pending_message_count": observation.pending_message_count,
            "is_streaming": observation.is_streaming,
        }))
    }

    fn observe_runtime(
        &mut self,
        settled_boundary_observed: bool,
    ) -> CliResult<crate::runtime_adapter::RuntimeObservation> {
        self.ensure_transport_alive()?;
        let state = self.request_blocking("get_state", serde_json::json!({}), HANDSHAKE_TIMEOUT)?;
        let data = state.get("data").ok_or_else(|| {
            CliError::Usage("pi get_state response missing data during observation".to_string())
        })?;
        let observation = Self::observation_from_state(data, true, settled_boundary_observed);
        self.last_observation = Some(observation.clone());
        Ok(observation)
    }

    fn quiesce_runtime(&mut self) -> CliResult<crate::runtime_adapter::QuiesceOutcome> {
        let observation = self.observe_runtime(true)?;
        let drained =
            observation.is_streaming == Some(false) && observation.pending_message_count == Some(0);
        Ok(crate::runtime_adapter::QuiesceOutcome {
            drained,
            evidence: if drained {
                "Pi get_state observed isStreaming=false and pendingMessageCount=0".to_string()
            } else {
                format!(
                    "Pi get_state did not prove a drained runtime: isStreaming={:?}, pendingMessageCount={:?}",
                    observation.is_streaming, observation.pending_message_count
                )
            },
            observation,
        })
    }

    fn writable_children_drain_proof(
        &self,
    ) -> (
        harness_core::agentfirm_api::RuntimePostconditionStatus,
        String,
    ) {
        use harness_core::agentfirm_api::RuntimePostconditionStatus;

        match (self.permission_ceiling, self.tools_allowlist.as_deref()) {
            (PermissionCeiling::ReadOnly, Some("read,grep,find,ls")) => (
                RuntimePostconditionStatus::Satisfied,
                "actual Pi spawn argv is the reviewed read/grep/find/ls-only allowlist with extensions disabled; no shell/write/edit tool can create a writable child"
                    .to_string(),
            ),
            (PermissionCeiling::FullAccess, _) => (
                RuntimePostconditionStatus::Unknown,
                "Pi FullAccess exposes shell/process creation, while Pi RPC has no background-job inventory and a child may escape the owned process group; writable-child drain is unprovable"
                    .to_string(),
            ),
            (PermissionCeiling::WorkspaceWrite, _) => (
                RuntimePostconditionStatus::Unknown,
                "Pi WorkspaceWrite should have failed spawn admission because no filesystem/process containment proves writable-child drain"
                    .to_string(),
            ),
            (PermissionCeiling::ReadOnly, actual) => (
                RuntimePostconditionStatus::Unknown,
                format!(
                    "Pi ReadOnly runtime argv did not retain the exact reviewed allowlist: {actual:?}"
                ),
            ),
        }
    }

    fn release(&mut self) -> CliResult<crate::runtime_adapter::RuntimeObservation> {
        if !self.released {
            self.released = true;
            kill_worker_tree(&mut self.child);
            if self
                .child
                .try_wait()
                .map_err(|error| {
                    CliError::Usage(format!("failed to verify pi process release: {error}"))
                })?
                .is_none()
            {
                return Err(CliError::Usage(
                    "PI_RUNTIME_RELEASE_UNKNOWN: disposer returned while process was alive"
                        .to_string(),
                ));
            }
        }
        let mut observation =
            self.last_observation
                .clone()
                .unwrap_or(crate::runtime_adapter::RuntimeObservation {
                    transport_alive: false,
                    process_alive: false,
                    is_streaming: Some(false),
                    pending_message_count: None,
                    steering_mode: None,
                    follow_up_mode: None,
                    settled_boundary_observed: false,
                });
        observation.transport_alive = false;
        observation.process_alive = false;
        // A successful quiesce is the prerequisite for a successful Close;
        // release itself only proves process death and does not invent queue
        // drain evidence.
        self.last_observation = Some(observation.clone());
        Ok(observation)
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
    pub(crate) fn prompt<A, S, F, C>(
        &mut self,
        text: &str,
        idle_timeout: Duration,
        mut on_input_accepted: A,
        mut on_steer_result: S,
        mut on_event: F,
        mut poll_control: C,
    ) -> CliResult<PiTurnOutcome>
    where
        A: FnMut(&crate::runtime_adapter::ControlTransportReceipt) -> CliResult<()>,
        S: FnMut(
            &crate::runtime_adapter::PendingSteer,
            &crate::runtime_adapter::SteerProviderResult,
        ) -> CliResult<()>,
        F: FnMut(&serde_json::Value),
        C: FnMut() -> crate::runtime_adapter::CycleControl,
    {
        let prompt_response = self.request_blocking(
            "prompt",
            serde_json::json!({"message": text}),
            HANDSHAKE_TIMEOUT,
        )?;
        let input_acceptance_receipt = crate::runtime_adapter::ControlTransportReceipt {
            command: "prompt".to_string(),
            response_id: prompt_response
                .get("id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            success: true,
        };
        if input_acceptance_receipt.response_id.is_none() {
            return Err(CliError::Usage(
                "PI_PROMPT_RECEIPT_UNKNOWN: successful prompt response had no id".to_string(),
            ));
        }
        on_input_accepted(&input_acceptance_receipt)?;

        let mut last_idle = Instant::now();
        let mut interrupted = false;
        let mut close_requested = false;
        let mut tool_call_count: u32 = 0;
        let mut final_text = String::new();
        let mut control_receipts = Vec::new();

        loop {
            match self.incoming.recv_timeout(Duration::from_millis(500)) {
                Ok(frame) => {
                    last_idle = Instant::now();
                    let event_type = frame.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    // A control observed after agent_settled would target no
                    // current cycle. Leave it queued for the idle/next-cycle
                    // control path instead of fabricating a late abort/steer.
                    if event_type == "agent_settled" {
                        break;
                    }

                    // Check for control intents (close/interrupt/steer).
                    let mut control = poll_control();
                    if control.close || control.interrupt {
                        interrupted = true;
                        close_requested = control.close;
                    }
                    control_receipts
                        .extend(self.apply_cycle_control(&mut control, &mut on_steer_result)?);

                    match event_type {
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
                    let mut control = poll_control();
                    control_receipts
                        .extend(self.apply_cycle_control(&mut control, &mut on_steer_result)?);
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
        let terminal_observation = self.observe_runtime(true)?;
        if terminal_observation.is_streaming != Some(false) {
            return Err(CliError::Usage(format!(
                "PI_CYCLE_SETTLEMENT_UNKNOWN: agent_settled was not confirmed by get_state isStreaming=false: {terminal_observation:?}"
            )));
        }
        Ok(PiTurnOutcome {
            final_text,
            interrupted,
            close_requested_by_harness: close_requested,
            tool_call_count,
            input_acceptance_receipt,
            control_receipts,
            terminal_observation,
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
    description: crate::runtime_adapter_contract::RuntimeDescription,
    authority_session: Option<harness_core::agentfirm_api::AgentSession>,
    canonical_quiesced: bool,
    canonical_released: bool,
}

impl PiTeamRuntime {
    pub(crate) fn new(client: PiRpcClient) -> Self {
        Self {
            client,
            description: crate::runtime_adapter_contract::RuntimeDescription {
                binding_id: "pi-rpc-0.84.2".to_string(),
                native_protocol: "pi-jsonl-rpc".to_string(),
                composition_fingerprint: String::new(),
                capability_fingerprint: String::new(),
                capability_bindings: Vec::new(),
            },
            authority_session: None,
            canonical_quiesced: false,
            canonical_released: false,
        }
    }

    fn contract_preflight(
        &self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
        capability: crate::runtime_adapter_contract::SemanticCapability,
    ) -> Result<
        crate::runtime_adapter_contract::AdmissionDecision,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        let session = self.authority_session.as_ref().ok_or_else(|| {
            crate::runtime_adapter_contract::RuntimeContractError::FenceMismatch {
                fields: vec!["authority_session".to_string()],
            }
        })?;
        crate::runtime_adapter_contract::preflight_effect(
            &self.description,
            session,
            fence,
            capability,
            &[],
        )
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
                capability: "open_or_resume",
                status: CapabilityStatus::Supported,
                evidence: "pi --mode rpc --session <file>; tests/pi_team_member.rs".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "start_cycle",
                status: CapabilityStatus::Supported,
                evidence: "correlated prompt response proves input acceptance; agent_settled + get_state prove the later cycle boundary; tests/pi_team_member.rs journey".into(),
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
                capability: "observe",
                status: CapabilityStatus::Supported,
                evidence: "non-invasive get_state plus owned-process liveness".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "quiesce",
                status: CapabilityStatus::Supported,
                evidence: "get_state proves only cycle/queue idle; reviewed ReadOnly argv proves writable-child non-creation; Pi 0.84.2 synchronous JSONL plus file/directory sync proves flush. FullAccess returns Unknown and fails closed"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "release",
                status: CapabilityStatus::Supported,
                evidence: "owned process-group disposer waits for process exit and preserves session JSONL"
                    .into(),
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
                status: CapabilityStatus::Degraded,
                evidence: "read_only is enforced by --tools; workspace_write is refused without filesystem containment; trusted full_access is explicitly none_verified".into(),
                security_enforcement_locus: None,
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

    fn bind_authority_session(
        &mut self,
        session: harness_core::agentfirm_api::AgentSession,
        profile: &harness_core::ProviderIntegrationProfile,
    ) -> CliResult<()> {
        if session.provider_kind != "pi" || profile.provider != "pi" {
            return Err(CliError::Usage(format!(
                "RUNTIME_ADAPTER_PROVIDER_MISMATCH: Pi adapter cannot bind session={} profile={}",
                session.provider_kind, profile.provider
            )));
        }
        let composition = profile
            .composition_fingerprint
            .clone()
            .filter(|value| {
                session.control_state.composition_fingerprint.as_deref()
                    == Some(value.as_str())
            })
            .ok_or_else(|| {
                CliError::Usage(
                    "RUNTIME_ADAPTER_FENCE_INCOMPLETE: persisted profile/session composition fingerprint mismatch"
                        .to_string(),
                )
            })?;
        let capabilities = profile
            .capability_fingerprint
            .clone()
            .filter(|value| {
                session.control_state.capability_fingerprint.as_deref() == Some(value.as_str())
            })
            .ok_or_else(|| {
                CliError::Usage(
                    "RUNTIME_ADAPTER_FENCE_INCOMPLETE: persisted profile/session capability fingerprint mismatch"
                        .to_string(),
                )
            })?;
        self.description.composition_fingerprint = composition;
        self.description.capability_fingerprint = capabilities;
        self.description.capability_bindings = profile.capability_bindings.clone();
        self.authority_session = Some(session);
        Ok(())
    }

    fn run_cycle(
        &mut self,
        input: &str,
        idle_timeout: Duration,
        on_input_accepted: &mut dyn FnMut(
            &crate::runtime_adapter::ControlTransportReceipt,
        ) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(
            &crate::runtime_adapter::PendingSteer,
            &crate::runtime_adapter::SteerProviderResult,
        ) -> CliResult<()>,
        on_event: &mut dyn FnMut(&serde_json::Value),
        poll_control: &mut dyn FnMut() -> crate::runtime_adapter::CycleControl,
    ) -> CliResult<crate::runtime_adapter::ExecutionCycleOutcome> {
        let outcome = self.client.prompt(
            input,
            idle_timeout,
            &mut *on_input_accepted,
            &mut *on_steer_result,
            &mut *on_event,
            &mut *poll_control,
        )?;
        Ok(crate::runtime_adapter::ExecutionCycleOutcome {
            final_text: outcome.final_text,
            interrupted: outcome.interrupted,
            close_requested_by_harness: outcome.close_requested_by_harness,
            tool_call_count: outcome.tool_call_count,
            input_acceptance_receipt: outcome.input_acceptance_receipt,
            control_receipts: outcome.control_receipts,
            terminal_observation: outcome.terminal_observation,
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

fn pi_contract_bridge_error(
    error: impl std::fmt::Display,
) -> crate::runtime_adapter_contract::RuntimeContractError {
    crate::runtime_adapter_contract::RuntimeContractError::InvalidCapabilityBindings(format!(
        "Pi native bridge operation failed: {error}"
    ))
}

impl crate::runtime_adapter_contract::RuntimeAdapter for PiTeamRuntime {
    fn describe(&self) -> &crate::runtime_adapter_contract::RuntimeDescription {
        &self.description
    }

    fn open_or_resume(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
        native_session_ref: Option<&str>,
    ) -> Result<
        crate::runtime_adapter_contract::RuntimeObservation,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::OpenOrResume,
        )?;
        self.client
            .ensure_transport_alive()
            .map_err(pi_contract_bridge_error)?;
        if native_session_ref.is_some_and(|expected| expected != self.client.session_file()) {
            return Err(
                crate::runtime_adapter_contract::RuntimeContractError::FenceMismatch {
                    fields: vec!["native_session_ref.native_session_id".to_string()],
                },
            );
        }
        let session = self
            .authority_session
            .as_ref()
            .expect("preflight bound session");
        Ok(crate::runtime_adapter_contract::RuntimeObservation {
            native_session_ref: Some(self.client.session_file().to_string()),
            active_effect_id: None,
            continuation: session.control_state.continuation.clone(),
            observed_at: crate::now_string(),
        })
    }

    fn execute_control(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
        request: crate::runtime_adapter_contract::ControlRequest,
    ) -> Result<
        crate::runtime_adapter_contract::EffectReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        use crate::runtime_adapter_contract::{ControlIntent, SemanticCapability};
        use harness_core::agentfirm_api::{RuntimeEffectCertainty, RuntimePostconditionStatus};

        let capability = match &request.intent {
            ControlIntent::StartCycle { .. } => SemanticCapability::StartCycle,
            ControlIntent::InjectCurrentCycle { .. } => SemanticCapability::InjectCurrentCycle,
            ControlIntent::QueueNativeBoundary { .. } => SemanticCapability::QueueNativeBoundary,
            ControlIntent::Interrupt => SemanticCapability::Interrupt,
            ControlIntent::InhibitContinuation { .. } => SemanticCapability::InhibitContinuation,
            ControlIntent::ResumeContinuation { .. } => SemanticCapability::ResumeContinuation,
        };
        let admission = self.contract_preflight(fence, capability)?;
        self.canonical_quiesced = false;

        let (certainty, postcondition, native_evidence) = match request.intent {
            ControlIntent::StartCycle { input } => {
                let mut input_receipt = None;
                let outcome = self
                    .client
                    .prompt(
                        &input,
                        Duration::from_secs(30 * 60),
                        |receipt| {
                            input_receipt = receipt.response_id.clone();
                            Ok(())
                        },
                        |_pending, _result| Ok(()),
                        |_event| {},
                        crate::runtime_adapter::CycleControl::default,
                    )
                    .map_err(pi_contract_bridge_error)?;
                let receipt = input_receipt.ok_or_else(|| {
                    pi_contract_bridge_error("prompt success lacked a provider response id")
                })?;
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Satisfied,
                    vec![
                        format!("pi.prompt.response:{receipt}"),
                        format!(
                            "pi.agent_settled:is_streaming={:?}:pending={:?}",
                            outcome.terminal_observation.is_streaming,
                            outcome.terminal_observation.pending_message_count
                        ),
                    ],
                )
            }
            ControlIntent::InjectCurrentCycle { input } => {
                let response = self
                    .client
                    .request_blocking(
                        "steer",
                        serde_json::json!({"message": input}),
                        HANDSHAKE_TIMEOUT,
                    )
                    .map_err(pi_contract_bridge_error)?;
                let id = response
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| pi_contract_bridge_error("steer response lacked id"))?;
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Satisfied,
                    vec![format!("pi.steer.response:{id}")],
                )
            }
            ControlIntent::QueueNativeBoundary { input } => {
                let response = self
                    .client
                    .follow_up(&input)
                    .map_err(pi_contract_bridge_error)?;
                let id = response
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| pi_contract_bridge_error("follow_up response lacked id"))?;
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Satisfied,
                    vec![format!("pi.follow_up.response:{id}")],
                )
            }
            ControlIntent::Interrupt => {
                let response = self
                    .client
                    .request_blocking("abort", serde_json::json!({}), HANDSHAKE_TIMEOUT)
                    .map_err(pi_contract_bridge_error)?;
                let id = response
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| pi_contract_bridge_error("abort response lacked id"))?;
                // Abort success is only a transport receipt. The caller must
                // observe agent_settled plus get_state before the terminal
                // postcondition can become Satisfied.
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Unknown,
                    vec![format!("pi.abort.response:{id}")],
                )
            }
            ControlIntent::InhibitContinuation { .. }
            | ControlIntent::ResumeContinuation { .. } => {
                unreachable!("unsupported Pi continuation operation must fail canonical preflight")
            }
        };

        Ok(crate::runtime_adapter_contract::EffectReceipt {
            effect_id: request.effect_id,
            certainty,
            postcondition,
            admission: admission.admission,
            native_evidence,
        })
    }

    fn observe(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
    ) -> Result<
        crate::runtime_adapter_contract::RuntimeObservation,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::Observe,
        )?;
        self.client
            .observe_runtime(false)
            .map_err(pi_contract_bridge_error)?;
        let session = self
            .authority_session
            .as_ref()
            .expect("preflight bound session");
        Ok(crate::runtime_adapter_contract::RuntimeObservation {
            native_session_ref: Some(self.client.session_file().to_string()),
            active_effect_id: None,
            continuation: session.control_state.continuation.clone(),
            observed_at: crate::now_string(),
        })
    }

    fn inspect_effect(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
        _effect_id: &str,
    ) -> Result<
        crate::runtime_adapter_contract::EffectInspection,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::InspectEffect,
        )?;
        unreachable!("Pi inspect_effect is not admitted")
    }

    fn reconcile(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
        _inspection: &crate::runtime_adapter_contract::EffectInspection,
    ) -> Result<
        crate::runtime_adapter_contract::ReconcileReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::Reconcile,
        )?;
        unreachable!("Pi reconcile is not admitted")
    }

    fn quiesce(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
    ) -> Result<
        crate::runtime_adapter_contract::QuiesceReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        use crate::runtime_adapter_contract::{QuiesceReceiptBuilder, QuiesceStep};
        use harness_core::agentfirm_api::{
            NativeContinuationActivation, RuntimePostconditionStatus,
        };

        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::Quiesce,
        )?;
        let outcome = self
            .client
            .quiesce_runtime()
            .map_err(pi_contract_bridge_error)?;
        let session = self
            .authority_session
            .as_ref()
            .expect("preflight bound session");
        let drained = if outcome.drained {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unsatisfied
        };
        let continuation = if matches!(
            session.control_state.continuation.activation,
            NativeContinuationActivation::Disarmed
        ) {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unknown
        };
        let (writable_children, writable_children_evidence) =
            self.client.writable_children_drain_proof();
        let (flush, flush_evidence) =
            match confirm_pi_session_flush(Path::new(self.client.session_file())) {
                Ok(evidence) => (RuntimePostconditionStatus::Satisfied, evidence),
                Err(evidence) => (RuntimePostconditionStatus::Unknown, evidence),
            };
        let mut builder = QuiesceReceiptBuilder::new();
        builder.record(
            QuiesceStep::FenceAdmission,
            RuntimePostconditionStatus::Satisfied,
            "exact RuntimeFence admitted",
        )?;
        builder.record(
            QuiesceStep::InhibitContinuation,
            continuation,
            "Pi has no native continuation and activation is durably disarmed",
        )?;
        builder.record(
            QuiesceStep::SettleActiveCycle,
            drained,
            format!("isStreaming={:?}", outcome.observation.is_streaming),
        )?;
        builder.record(
            QuiesceStep::DrainNativeQueue,
            drained,
            format!(
                "pendingMessageCount={:?}",
                outcome.observation.pending_message_count
            ),
        )?;
        builder.record(
            QuiesceStep::DrainWritableChildren,
            writable_children,
            writable_children_evidence,
        )?;
        builder.record(QuiesceStep::ObserveIdle, drained, outcome.evidence)?;
        builder.record(QuiesceStep::ConfirmFlush, flush, flush_evidence)?;
        let receipt = builder.finish();
        receipt.verify()?;
        self.canonical_quiesced = true;
        Ok(receipt)
    }

    fn release(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
    ) -> Result<
        crate::runtime_adapter_contract::ReleaseReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        use harness_core::agentfirm_api::RuntimePostconditionStatus;

        if self.canonical_released {
            return Err(crate::runtime_adapter_contract::RuntimeContractError::AlreadyReleased);
        }
        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::Release,
        )?;
        if !self.canonical_quiesced {
            return Err(
                crate::runtime_adapter_contract::RuntimeContractError::CompositionSwapRequiresQuiesce,
            );
        }
        let session_file = self.client.session_file().to_string();
        let observation = self.client.release().map_err(pi_contract_bridge_error)?;
        let released = if !observation.transport_alive && !observation.process_alive {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unknown
        };
        let (flush, flush_evidence) = match confirm_pi_session_flush(Path::new(&session_file)) {
            Ok(evidence) => (RuntimePostconditionStatus::Satisfied, evidence),
            Err(evidence) => (RuntimePostconditionStatus::Unknown, evidence),
        };
        let receipt = crate::runtime_adapter_contract::ReleaseReceipt {
            native_runtime_released: released,
            live_handle_disposed: released,
            authority_detached: RuntimePostconditionStatus::Satisfied,
            flush_confirmed: flush,
            evidence: vec![
                format!("process_alive={}", observation.process_alive),
                flush_evidence,
            ],
        };
        if released != RuntimePostconditionStatus::Satisfied
            || flush != RuntimePostconditionStatus::Satisfied
        {
            return Err(crate::runtime_adapter_contract::RuntimeContractError::ReleaseIncomplete);
        }
        self.authority_session = None;
        self.canonical_released = true;
        Ok(receipt)
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
        // The explicit disposer and Drop share one idempotent release path.
        // This makes Close observable while retaining a leak-safe fallback.
        let _ = self.release();
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
        confirm_pi_session_flush, ensure_session_has_no_persisted_thinking,
        value_contains_persisted_thinking, PermissionCeiling, PiRpcClient,
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
                permission_ceiling: PermissionCeiling::ReadOnly,
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

        let (children, children_evidence) = client.writable_children_drain_proof();
        assert_eq!(
            children,
            harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
            "reviewed ReadOnly argv proves writable-child non-creation: {children_evidence}"
        );
        let flush = confirm_pi_session_flush(&session_file)
            .expect("a complete JSONL line must receive file+directory sync evidence");
        assert!(flush.contains("sync_all confirmed"), "{flush}");

        drop(client);

        let full_access = PiRpcClient::spawn(
            shim.to_str().unwrap(),
            super::PiSpawnOptions {
                cwd: &dir,
                model: None,
                resume_session_file: None,
                session_dir: &dir,
                member_name: "rpc-full-access-test",
                collaboration_env: &[],
                tools: None,
                permission_ceiling: PermissionCeiling::FullAccess,
            },
        )
        .expect("spawn FullAccess shim");
        let (children, children_evidence) = full_access.writable_children_drain_proof();
        assert_eq!(
            children,
            harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown,
            "FullAccess cannot claim child drain without a native job inventory: {children_evidence}"
        );
        assert!(children_evidence.contains("may escape the owned process group"));
        drop(full_access);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flush_evidence_requires_a_complete_regular_jsonl_file() {
        let dir = std::env::temp_dir().join(format!(
            "pi-rpc-flush-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let session_file = dir.join("session.jsonl");
        std::fs::write(&session_file, "{\"type\":\"session\"}").expect("write incomplete session");
        let error = confirm_pi_session_flush(&session_file)
            .expect_err("path existence without a complete record is not flush proof");
        assert!(error.contains("incomplete final JSONL record"));

        std::fs::write(&session_file, "{\"type\":\"session\"}\n").expect("complete session");
        confirm_pi_session_flush(&session_file).expect("complete file can be durably synced");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let linked = dir.join("linked-session.jsonl");
            symlink(&session_file, &linked).expect("create symlink fixture");
            let error = confirm_pi_session_flush(&linked)
                .expect_err("a symlink must not be promoted to native flush evidence");
            assert!(error.contains("regular non-symlink"));
        }
        std::fs::remove_dir_all(dir).unwrap();
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
