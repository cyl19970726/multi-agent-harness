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

use harness_core::agentfirm_api::PermissionCeiling;
use harness_runtime_host::OwnedProcessGroupRegistration;

mod capability_transport;
pub use capability_transport::*;

pub type PiResult<T> = Result<T, PiError>;

#[derive(Debug, thiserror::Error)]
pub enum PiError {
    #[error(transparent)]
    ProcessGroupAdmissionClosed(#[from] harness_runtime_host::ProcessGroupRegistrationError),
    #[error("{0}")]
    Usage(String),
    #[error("application callback failed: {detail}")]
    Callback {
        detail: String,
        supervisor_lease_lost: bool,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

type CliResult<T> = PiResult<T>;
type CliError = PiError;

pub fn tools_allowlist_for_ceiling(ceiling: PermissionCeiling) -> PiResult<Option<&'static str>> {
    match ceiling {
        PermissionCeiling::ReadOnly => Ok(Some("read,grep,find,ls")),
        PermissionCeiling::WorkspaceWrite => Err(PiError::Usage(
            "PI_PERMISSION_ADMISSION_FAILED: workspace_write requires verified filesystem containment; Pi --tools only limits tool kinds"
                .to_string(),
        )),
        PermissionCeiling::FullAccess => Ok(None),
    }
}

pub fn compile_rpc_permission(
    ceiling: PermissionCeiling,
) -> PiResult<(&'static str, &'static str)> {
    match ceiling {
        PermissionCeiling::ReadOnly => Ok(("tool-allowlist-read-only", "none")),
        PermissionCeiling::WorkspaceWrite => Err(PiError::Usage(
            "Pi cannot contain workspace_write without an OS sandbox or controlled tool bridge"
                .to_string(),
        )),
        PermissionCeiling::FullAccess => Ok(("unrestricted", "none")),
    }
}

pub fn admit_permission_ceiling(
    ceiling: PermissionCeiling,
    compiled_tools: Option<&str>,
) -> PiResult<()> {
    let expected = tools_allowlist_for_ceiling(ceiling)?;
    if compiled_tools != expected {
        return Err(PiError::Usage(format!(
            "PI_PERMISSION_ADMISSION_FAILED: {ceiling:?} expected tools {expected:?}, got {compiled_tools:?}"
        )));
    }
    Ok(())
}

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

pub struct PiRpcClient {
    child: Child,
    owned_process_group: OwnedProcessGroupRegistration,
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
    last_observation: Option<harness_runtime_contract::CycleRuntimeObservation>,
    released: bool,
}

pub struct PiSpawnOptions<'a> {
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

pub struct PiTurnOutcome {
    pub final_text: String,
    pub interrupted: bool,
    pub close_requested_by_harness: bool,
    pub tool_call_count: u32,
    pub native_correlation: harness_runtime_contract::NativeCycleCorrelation,
    pub control_receipts: Vec<harness_runtime_contract::ControlTransportReceipt>,
    pub terminal_observation: harness_runtime_contract::CycleRuntimeObservation,
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
pub fn confirm_pi_session_flush(path: &Path) -> Result<String, String> {
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
    pub fn spawn(pi_bin: &str, options: PiSpawnOptions<'_>) -> CliResult<Self> {
        admit_permission_ceiling(options.permission_ceiling, options.tools)?;
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
            .stderr(Stdio::piped())
            .env_remove("AGENTFIRM_HTTP_CREDENTIALS_JSON");

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
        let owned_process_group = OwnedProcessGroupRegistration::new(&mut child)?;

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
            owned_process_group,
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
    ) -> harness_runtime_contract::CycleRuntimeObservation {
        harness_runtime_contract::CycleRuntimeObservation {
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

    pub fn session_file(&self) -> &str {
        &self.session_file
    }

    pub fn ensure_transport_alive(&mut self) -> CliResult<()> {
        let reader_ended = self.reader.as_ref().is_some_and(JoinHandle::is_finished);
        let child_ended = self
            .owned_process_group
            .try_wait_and_release(&mut self.child)
            .map_err(|error| {
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
        control: &mut harness_runtime_contract::CycleControl,
        on_steer_result: &mut dyn FnMut(
            &harness_runtime_contract::SteerRequest,
            &harness_runtime_contract::SteerProviderResult,
        ) -> CliResult<()>,
    ) -> CliResult<Vec<harness_runtime_contract::ControlTransportReceipt>> {
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
                    let receipt = harness_runtime_contract::ControlTransportReceipt {
                        command: "steer".to_string(),
                        response_id: Some(response_id.clone()),
                        success: true,
                    };
                    // Durable settlement happens before the API caller sees
                    // success. A provider transport receipt without a settled
                    // RuntimeCommand must never escape as `steer_accepted`.
                    on_steer_result(
                        &pending,
                        &harness_runtime_contract::SteerProviderResult::Acknowledged(
                            receipt.clone(),
                        ),
                    )?;
                    receipts.push(receipt);
                }
                Err(error) => {
                    let detail = format!(
                        "PI_STEER_RECEIPT_UNKNOWN: provider did not acknowledge steer: {error}"
                    );
                    on_steer_result(
                        &pending,
                        &harness_runtime_contract::SteerProviderResult::Unknown(detail.clone()),
                    )?;
                    for undispatched in injects {
                        let not_applied = format!(
                            "PI_STEER_NOT_DISPATCHED: an earlier steer failed before this command: {detail}"
                        );
                        on_steer_result(
                            &undispatched,
                            &harness_runtime_contract::SteerProviderResult::NotApplied(
                                not_applied.clone(),
                            ),
                        )?;
                    }
                    return Err(CliError::Usage(detail));
                }
            }
        }
        if control.close || control.interrupt {
            let response =
                self.request_blocking("abort", serde_json::json!({}), HANDSHAKE_TIMEOUT)?;
            receipts.push(harness_runtime_contract::ControlTransportReceipt {
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
    pub fn follow_up(&mut self, text: &str) -> CliResult<serde_json::Value> {
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
    pub fn queue_snapshot(&mut self) -> CliResult<serde_json::Value> {
        let observation = self.observe_runtime(false)?;
        Ok(serde_json::json!({
            "steering_mode": observation.steering_mode,
            "follow_up_mode": observation.follow_up_mode,
            "pending_message_count": observation.pending_message_count,
            "is_streaming": observation.is_streaming,
        }))
    }

    pub fn observe_runtime(
        &mut self,
        settled_boundary_observed: bool,
    ) -> CliResult<harness_runtime_contract::CycleRuntimeObservation> {
        self.ensure_transport_alive()?;
        let state = self.request_blocking("get_state", serde_json::json!({}), HANDSHAKE_TIMEOUT)?;
        let data = state.get("data").ok_or_else(|| {
            CliError::Usage("pi get_state response missing data during observation".to_string())
        })?;
        let observation = Self::observation_from_state(data, true, settled_boundary_observed);
        self.last_observation = Some(observation.clone());
        Ok(observation)
    }

    pub fn quiesce_runtime(&mut self) -> CliResult<harness_runtime_contract::QuiesceOutcome> {
        let observation = self.observe_runtime(true)?;
        let drained =
            observation.is_streaming == Some(false) && observation.pending_message_count == Some(0);
        Ok(harness_runtime_contract::QuiesceOutcome {
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

    pub fn writable_children_drain_proof(
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

    pub fn release(&mut self) -> CliResult<harness_runtime_contract::CycleRuntimeObservation> {
        if !self.released {
            self.released = true;
            let _ = self.owned_process_group.kill_and_reap(&mut self.child);
            if self
                .owned_process_group
                .try_wait_and_release(&mut self.child)
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
        let mut observation = self.last_observation.clone().unwrap_or(
            harness_runtime_contract::CycleRuntimeObservation {
                transport_alive: false,
                process_alive: false,
                is_streaming: Some(false),
                pending_message_count: None,
                steering_mode: None,
                follow_up_mode: None,
                settled_boundary_observed: false,
            },
        );
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
    /// [`CycleControl`](harness_runtime_contract::CycleControl): explicit Steer
    /// bodies are compiled into `steer` frames at this boundary, and
    /// close/interrupt sends `abort` while the loop continues reading until
    /// `agent_settled`.
    ///
    /// Returns `PiTurnOutcome` with `final_text` extracted from the last
    /// `turn_end.message` content blocks.
    pub fn prompt<A, S, F, C>(
        &mut self,
        text: &str,
        idle_timeout: Duration,
        mut on_input_accepted: A,
        mut on_steer_result: S,
        mut on_event: F,
        mut poll_control: C,
    ) -> CliResult<PiTurnOutcome>
    where
        A: FnMut(&harness_runtime_contract::ControlTransportReceipt) -> CliResult<()>,
        S: FnMut(
            &harness_runtime_contract::SteerRequest,
            &harness_runtime_contract::SteerProviderResult,
        ) -> CliResult<()>,
        F: FnMut(&serde_json::Value),
        C: FnMut() -> harness_runtime_contract::CycleControl,
    {
        self.prompt_dyn(
            text,
            idle_timeout,
            &mut on_input_accepted,
            &mut on_steer_result,
            &mut on_event,
            &mut poll_control,
        )
    }

    pub fn prompt_dyn(
        &mut self,
        text: &str,
        idle_timeout: Duration,
        on_input_accepted: &mut dyn FnMut(
            &harness_runtime_contract::ControlTransportReceipt,
        ) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(
            &harness_runtime_contract::SteerRequest,
            &harness_runtime_contract::SteerProviderResult,
        ) -> CliResult<()>,
        on_event: &mut dyn FnMut(&serde_json::Value),
        poll_control: &mut dyn FnMut() -> harness_runtime_contract::CycleControl,
    ) -> CliResult<PiTurnOutcome> {
        // Pi's `agent_settled` event has no native cycle id. Under the strict
        // one-driver contract the previous cycle has already proved idle, so
        // discard every queued pre-dispatch event before assigning the next
        // prompt response id. A stale idle event must not terminate a fresh
        // follow-up.
        while self.incoming.try_recv().is_ok() {}
        let prompt_response = self.request_blocking(
            "prompt",
            serde_json::json!({"message": text}),
            HANDSHAKE_TIMEOUT,
        )?;
        let input_acceptance_receipt = harness_runtime_contract::ControlTransportReceipt {
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
                        .extend(self.apply_cycle_control(&mut control, on_steer_result)?);

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
                        .extend(self.apply_cycle_control(&mut control, on_steer_result)?);
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
                        let _ = self.owned_process_group.kill_and_reap(&mut self.child);
                        return Err(CliError::Usage(format!(
                            "pi rpc prompt timed out after {}s idle{}",
                            idle_timeout.as_secs(),
                            stderr_suffix(&self.stderr_tail)
                        )));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Reader thread exited — transport dead.
                    let status = self
                        .owned_process_group
                        .try_wait_and_release(&mut self.child)
                        .ok()
                        .flatten();
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
        let provider_input_id = input_acceptance_receipt
            .response_id
            .clone()
            .expect("validated Pi prompt response id");
        Ok(PiTurnOutcome {
            final_text,
            interrupted,
            close_requested_by_harness: close_requested,
            tool_call_count,
            native_correlation: harness_runtime_contract::NativeCycleCorrelation {
                provider_input_id: provider_input_id.clone(),
                input_acceptance_receipt,
                // Pi RPC emits an unkeyed `agent_settled` event. The one-driver
                // prompt response plus the ordered terminal boundary closes
                // the local cycle; there is no stronger native terminal id.
                terminal_provider_input_id: Some(provider_input_id.clone()),
                exact_terminal_ref: Some(format!("pi.agent_settled:{provider_input_id}")),
            },
            control_receipts,
            terminal_observation,
        })
    }

    /// Extract text from the last `turn_end.message` content blocks.
    pub fn extract_turn_end_text(frame: &serde_json::Value) -> String {
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

    pub fn request_blocking(
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

/// The Pi binding of [`TeamRuntimeAdapter`](harness_runtime_contract::TeamRuntimeAdapter):
/// a live `pi --mode rpc` child as the process-local RuntimeHandle. Durable
/// identity stays with AgentSession/NativeSessionRef; this handle is
/// disposable and never an authority.
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

mod team_runtime;
pub use team_runtime::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
