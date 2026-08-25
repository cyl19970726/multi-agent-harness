//! DeepSeek Harness native AgentHandle binding for the provider-neutral Team runtime contract.
//!
//! This module owns only the process-local transport to
//! `apps/deepseek-member-runner`. Durable identity, authorization, Work,
//! Messages, and RuntimeCommands remain outside the adapter. One transport is
//! one disposable runtime generation; the provider-native DeepSeek session id
//! is the stable resume point across generations.
//!
//! The exact reviewed composition is the official `@deepseek-ai/dsh-*`
//! plugin set at 0.1.1-rc.2 and upstream revision b150a551. A different
//! package, composition fingerprint, or native session id fails before it can
//! be reported as compatible.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use harness_runtime_host::OwnedProcessGroupRegistration;

use harness_core::agentfirm_api::{
    AgentSession, NativeContinuationActivation, RuntimeEffectCertainty, RuntimePostconditionStatus,
};
use serde_json::{json, Value};

use harness_runtime_contract::{
    AdmissionDecision, CapabilityBinding, CapabilityStatus, ControlIntent, ControlRequest,
    ControlTransportReceipt, CycleControl, CycleRuntimeObservation as CycleObservation,
    EffectInspection, EffectReceipt, ExecutionCycleOutcome, MemberRuntimeCloseReceipt,
    NativeControlPrimitive, ProviderControlAction, ProviderControlPlan, ProviderNativeControl,
    ProviderTerminalFailure, QuiesceReceipt, QuiesceReceiptBuilder, QuiesceStep, ReconcileReceipt,
    ReleaseReceipt, RuntimeAdapter, RuntimeBindingFence, RuntimeContractError, RuntimeDescription,
    SemanticCapability, SteerProviderResult, SteerRequest, TeamRuntimeAdapter,
};

mod capability_transport;
mod composition;
mod error;
mod permission;
mod runner_contract;
pub use capability_transport::*;
use composition::*;
pub use error::{DeepSeekError, DeepSeekResult};
pub use permission::*;

type CliResult<T> = DeepSeekResult<T>;
type CliError = DeepSeekError;

fn now_string() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

pub const REVIEWED_DEEPSEEK_HARNESS_VERSION: &str = "0.1.1-rc.2";
pub const REVIEWED_DEEPSEEK_SOURCE_REVISION: &str = "b150a551b8d465e31e418e1b2eaf5e79bbb7d28e";
pub const REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT: &str =
    "sha256:333c529f67aa2237096dd5191cfd4c46842d14eed786669b9be18b9cc4e2401f";
const DEEPSEEK_BINDING_ID: &str = "deepseek-harness-0.1.1-rc.2+b150a551";
const DEEPSEEK_NATIVE_PROTOCOL: &str = "deepseek-harness-native/v1";
const CONTROL_POLL: Duration = Duration::from_millis(25);
const GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Owned spawn configuration. The caller resolves Workspace/project cwd and
/// supplies only the collaboration environment authorized for this member.
#[derive(Debug)]
pub struct DeepSeekTeamRuntimeConfig {
    pub runner_path: PathBuf,
    pub cwd: PathBuf,
    pub team_run_id: String,
    pub member_run_id: String,
    pub member_name: String,
    pub role_label: String,
    pub owned_paths: Vec<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: String,
    pub allowed_tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub setting_sources: Vec<String>,
    pub resume_session_id: Option<String>,
    pub environment: harness_runtime_contract::CollaborationCapabilityEnvironment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransportState {
    Starting,
    Idle,
    Active,
    Interrupted,
    Closed,
    Disconnected,
}

#[derive(Debug)]
struct RunnerEvent {
    name: String,
    data: Value,
    raw: Value,
}

fn native_cycle_correlation(
    input_id: &str,
    receipt: ControlTransportReceipt,
    terminal_kind: &str,
    native_session_id: &str,
) -> harness_runtime_contract::NativeCycleCorrelation {
    harness_runtime_contract::NativeCycleCorrelation {
        provider_input_id: input_id.to_string(),
        input_acceptance_receipt: receipt,
        terminal_provider_input_id: Some(input_id.to_string()),
        exact_terminal_ref: Some(format!(
            "deepseek_harness.{terminal_kind}:{input_id}:{native_session_id}"
        )),
    }
}

impl RunnerEvent {
    fn parse(line: &str) -> CliResult<Self> {
        let raw: Value = serde_json::from_str(line).map_err(|error| {
            CliError::Usage(format!(
                "DEEPSEEK_HARNESS_PROTOCOL_ERROR: invalid runner JSON: {error}"
            ))
        })?;
        let name = raw
            .get("event")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                CliError::Usage(
                    "DEEPSEEK_HARNESS_PROTOCOL_ERROR: runner event is missing `event`".into(),
                )
            })?
            .to_string();
        let data = raw.get("data").cloned().unwrap_or(Value::Null);
        runner_contract::validate_runner_frame("eventPayloadSchemas", "event", "data", &raw)?;
        Ok(Self { name, data, raw })
    }
}

/// A child guard that owns the entire runner process group. A stale
/// Supervisor generation cannot orphan the DSH runner descendants.
struct DeepSeekRunnerChild {
    child: Child,
    process_group: OwnedProcessGroupRegistration,
    armed: bool,
}

impl DeepSeekRunnerChild {
    fn new(mut child: Child) -> CliResult<Self> {
        let process_group = OwnedProcessGroupRegistration::new(&mut child)?;
        Ok(Self {
            child,
            process_group,
            armed: true,
        })
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let status = self.process_group.try_wait_and_release(&mut self.child)?;
        if status.is_some() {
            self.armed = false;
        }
        Ok(status)
    }

    fn wait_until(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.process_group.try_wait_and_release(&mut self.child)? {
                self.armed = false;
                return Ok(Some(status));
            }
            if started.elapsed() >= timeout {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn terminate_group(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = !matches!(
            self.process_group.kill_and_reap(&mut self.child),
            Ok(Some(_))
        );
    }
}

impl Drop for DeepSeekRunnerChild {
    fn drop(&mut self) {
        self.terminate_group();
    }
}

/// Persistent NDJSON transport to the native DSH Cordis runner.
struct DeepSeekRunnerTransport {
    child: DeepSeekRunnerChild,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<String>>,
    native_session_id: String,
    expected_resume_session_id: Option<String>,
    provider_version: Option<String>,
    state: TransportState,
    next_input_id: u64,
    pending_input_count: u64,
    last_cycle_terminal: bool,
    last_interrupt_resumed_same_session: bool,
    close_reason: Option<String>,
}

impl DeepSeekRunnerTransport {
    fn spawn(config: &DeepSeekTeamRuntimeConfig) -> CliResult<Self> {
        verify_runner_harness_composition(&config.runner_path)?;
        // Validate and freeze the shared Rust/Node protocol before spawning
        // the runner or allowing it to load the DSH plugin composition.
        let start_frame = config.start_frame()?;

        let mut command = Command::new("node");
        command
            .arg(&config.runner_path)
            .current_dir(&config.cwd)
            .envs(
                config
                    .environment
                    .as_pairs()
                    .iter()
                    .map(|(key, value)| (key, value)),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("AGENTFIRM_HTTP_CREDENTIALS_JSON");
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command.spawn().map_err(|error| {
            CliError::Usage(format!(
                "failed to spawn DeepSeek Harness runner {}: {error}",
                config.runner_path.display()
            ))
        })?;
        let mut child = DeepSeekRunnerChild::new(child)?;
        let stdin = child
            .child
            .stdin
            .take()
            .ok_or_else(|| CliError::Usage("DeepSeek runner stdin unavailable".into()))?;
        let stdout = child
            .child
            .stdout
            .take()
            .ok_or_else(|| CliError::Usage("DeepSeek runner stdout unavailable".into()))?;
        let stderr = child
            .child
            .stderr
            .take()
            .ok_or_else(|| CliError::Usage("DeepSeek runner stderr unavailable".into()))?;

        let (line_tx, lines) = mpsc::channel();
        let stdout_reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line_tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            text
        });

        let mut transport = Self {
            child,
            stdin: Some(stdin),
            lines,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            native_session_id: config.resume_session_id.clone().unwrap_or_default(),
            expected_resume_session_id: config.resume_session_id.clone(),
            provider_version: None,
            state: TransportState::Starting,
            next_input_id: 0,
            pending_input_count: 0,
            last_cycle_terminal: false,
            last_interrupt_resumed_same_session: false,
            close_reason: None,
        };
        transport.write_frame(&start_frame)?;
        transport.wait_for_session_bound(Duration::from_secs(30))?;
        Ok(transport)
    }

    fn wait_for_session_bound(&mut self, timeout: Duration) -> CliResult<()> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            let Some(event) = self.receive_event(CONTROL_POLL)? else {
                continue;
            };
            match event.name.as_str() {
                "session_bound" => {
                    self.accept_session_binding(&event)?;
                    return Ok(());
                }
                "runner_error" => {
                    return Err(CliError::Usage(format!(
                        "DEEPSEEK_HARNESS_RUNNER_ERROR: {}",
                        event.data
                    )))
                }
                _ => {}
            }
        }
        Err(CliError::Usage(
            "DEEPSEEK_HARNESS_BIND_TIMEOUT: runner did not bind a native Session".into(),
        ))
    }

    fn write_frame(&mut self, frame: &Value) -> CliResult<()> {
        runner_contract::validate_runner_frame(
            "commandPayloadSchemas",
            "command",
            "payload",
            frame,
        )?;
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            CliError::Usage("DEEPSEEK_HARNESS_TRANSPORT_CLOSED: runner stdin is closed".into())
        })?;
        serde_json::to_writer(&mut *stdin, frame)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    fn ensure_alive(&mut self) -> CliResult<()> {
        if matches!(
            self.state,
            TransportState::Closed | TransportState::Disconnected
        ) {
            return Err(CliError::Usage(format!(
                "DEEPSEEK_HARNESS_TRANSPORT_CLOSED: state={:?}",
                self.state
            )));
        }
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(status)) => {
                self.state = TransportState::Disconnected;
                Err(CliError::Usage(format!(
                    "DEEPSEEK_HARNESS_TRANSPORT_CLOSED: runner exited with {status}"
                )))
            }
            Err(error) => Err(CliError::Usage(format!(
                "failed to inspect DeepSeek Harness runner: {error}"
            ))),
        }
    }

    fn receive_event(&mut self, timeout: Duration) -> CliResult<Option<RunnerEvent>> {
        match self.lines.recv_timeout(timeout) {
            Ok(line) => RunnerEvent::parse(&line).map(Some),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                self.state = TransportState::Disconnected;
                Err(CliError::Usage(format!(
                    "DEEPSEEK_HARNESS_TRANSPORT_CLOSED: runner stdout disconnected{}",
                    self.stderr_snapshot()
                )))
            }
        }
    }

    fn stderr_snapshot(&mut self) -> String {
        if self.child.try_wait().ok().flatten().is_some() {
            if let Some(reader) = self.stderr_reader.take() {
                let text = reader.join().unwrap_or_default();
                if !text.trim().is_empty() {
                    return format!("; stderr: {}", text.trim());
                }
            }
        }
        String::new()
    }

    fn accept_session_binding(&mut self, event: &RunnerEvent) -> CliResult<()> {
        let session_id = event
            .data
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                CliError::Usage(
                    "DEEPSEEK_HARNESS_PROTOCOL_ERROR: session_bound lacked sessionId".into(),
                )
            })?;
        let version = verify_session_bound_provider_identity(&event.data)?;
        if let Some(expected) = self.expected_resume_session_id.as_deref() {
            if expected != session_id {
                return Err(CliError::Usage(format!(
                    "DEEPSEEK_HARNESS_RESUME_MISMATCH: expected native session {expected}, observed {session_id}"
                )));
            }
        }
        if !self.native_session_id.is_empty() && self.native_session_id != session_id {
            return Err(CliError::Usage(format!(
                "DEEPSEEK_HARNESS_SESSION_CHANGED: runtime generation changed native session {} -> {session_id}",
                self.native_session_id
            )));
        }
        self.native_session_id = session_id.to_string();
        self.provider_version = Some(version);
        if matches!(self.state, TransportState::Starting) {
            self.state = TransportState::Idle;
        }
        Ok(())
    }

    fn send_input(&mut self, input: &str) -> CliResult<String> {
        self.ensure_alive()?;
        self.next_input_id += 1;
        let input_id = format!("deepseek-cycle-{}", self.next_input_id);
        self.write_frame(&json!({
            "command": "deliver",
            "payload": {
                "id": input_id,
                "kind": "runtime_cycle",
                "sender_runtime_id": "harness-runtime-adapter",
                "correlation_id": input_id,
                "body": input,
            }
        }))?;
        self.pending_input_count += 1;
        self.last_cycle_terminal = false;
        self.last_interrupt_resumed_same_session = false;
        self.state = TransportState::Active;
        Ok(input_id)
    }

    fn interrupt(&mut self) -> CliResult<()> {
        self.write_frame(&json!({"command": "interrupt", "payload": {}}))
    }

    fn close(&mut self, reason: &str) -> CliResult<()> {
        if matches!(self.state, TransportState::Closed) {
            return Ok(());
        }
        self.close_reason = Some(reason.to_string());
        self.write_frame(&json!({
            "command": "close",
            "payload": {"reason": reason},
        }))
    }

    fn wait_for_member_closed(&mut self, timeout: Duration) -> CliResult<Value> {
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(CliError::Usage(
                    "DEEPSEEK_HARNESS_CLOSE_TIMEOUT: runner did not emit member_closed".into(),
                ));
            }
            let Some(event) = self.receive_event(remaining.min(CONTROL_POLL))? else {
                continue;
            };
            match event.name.as_str() {
                "session_bound" => self.accept_session_binding(&event)?,
                "member_closed" => {
                    let session_id = event.data.get("sessionId").and_then(Value::as_str);
                    let session_matches = if self.native_session_id.is_empty() {
                        session_id.is_none_or(str::is_empty)
                    } else {
                        session_id == Some(self.native_session_id.as_str())
                    };
                    if !session_matches {
                        return Err(CliError::Usage(format!(
                            "DEEPSEEK_HARNESS_CLOSE_SESSION_MISMATCH: retained={} event={session_id:?}",
                            self.native_session_id
                        )));
                    }
                    self.state = TransportState::Closed;
                    self.stdin.take();
                    if self.child.wait_until(GRACEFUL_CLOSE_TIMEOUT)?.is_none() {
                        return Err(CliError::Usage(
                            "DEEPSEEK_HARNESS_CLOSE_TIMEOUT: owned runner process group did not exit"
                                .into(),
                        ));
                    }
                    if let Some(reader) = self.stdout_reader.take() {
                        let _ = reader.join();
                    }
                    if let Some(reader) = self.stderr_reader.take() {
                        let _ = reader.join();
                    }
                    return Ok(event.data);
                }
                "runner_error" => {
                    return Err(CliError::Usage(format!(
                        "DEEPSEEK_HARNESS_RUNNER_ERROR: {}",
                        event.data
                    )));
                }
                // Lifecycle noise emitted while the runner is draining is
                // observable but does not supersede member_closed.
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_cycle(
        &mut self,
        input: &str,
        idle_timeout: Duration,
        on_input_accepted: &mut dyn FnMut(&ControlTransportReceipt) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(&SteerRequest, &SteerProviderResult) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> CycleControl,
    ) -> CliResult<ExecutionCycleOutcome> {
        let input_id = self.send_input(input)?;
        let started = Instant::now();
        let mut final_text = String::new();
        let mut input_acceptance_receipt = None;
        let mut control_receipts = Vec::new();
        let mut tool_call_count = 0u32;
        let mut saw_assistant_message = false;
        let mut interrupt_sent = false;
        let mut interrupt_requested = false;
        let mut interrupted = false;
        let mut close_requested = false;

        loop {
            let control = poll_control();
            if let Some(error) = control.fatal_error {
                return Err(CliError::Usage(error));
            }
            for pending in control.injects {
                on_steer_result(
                    &pending,
                    &SteerProviderResult::NotApplied(
                        "DEEPSEEK_CURRENT_CYCLE_INJECTION_UNSUPPORTED: use an ordinary queued Message"
                            .into(),
                    ),
                )?;
            }
            interrupt_requested |= control.interrupt || control.close;
            close_requested |= control.close;
            // `consumed` is the exact provider-boundary receipt for this
            // StartCycle. Do not race query.interrupt ahead of query creation
            // and then claim a cycle that never crossed that boundary.
            if interrupt_requested && input_acceptance_receipt.is_some() && !interrupt_sent {
                self.interrupt()?;
                interrupt_sent = true;
            }

            let Some(event) = self.receive_event(CONTROL_POLL)? else {
                if started.elapsed() >= idle_timeout {
                    return Err(CliError::Usage(format!(
                        "DeepSeek Harness cycle {input_id} exceeded idle timeout of {}s",
                        idle_timeout.as_secs()
                    )));
                }
                continue;
            };
            on_event(&event.raw);
            match event.name.as_str() {
                "session_bound" => self.accept_session_binding(&event)?,
                "assistant_message" => {
                    saw_assistant_message = true;
                    let (text, tools) = assistant_projection(&event.data);
                    final_text.push_str(&text);
                    tool_call_count = tool_call_count.saturating_add(tools);
                }
                "consumed" => {
                    if event.data.get("id").and_then(Value::as_str) == Some(input_id.as_str()) {
                        self.pending_input_count = self.pending_input_count.saturating_sub(1);
                        let session = event
                            .data
                            .get("sessionId")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or("session-binding-pending");
                        let receipt = ControlTransportReceipt {
                            command: "deliver".into(),
                            response_id: Some(format!("deepseek-sdk-session:{session}:{input_id}")),
                            success: true,
                        };
                        on_input_accepted(&receipt)?;
                        input_acceptance_receipt = Some(receipt);
                    }
                }
                "turn_complete" => {
                    if event.data.get("triggerMessageId").and_then(Value::as_str)
                        != Some(input_id.as_str())
                    {
                        continue;
                    }
                    self.last_cycle_terminal = true;
                    self.state = TransportState::Idle;
                    let receipt = input_acceptance_receipt.clone().ok_or_else(|| {
                        CliError::Usage(format!(
                            "DEEPSEEK_HARNESS_PROTOCOL_ERROR: turn_complete preceded consumed for {input_id}"
                        ))
                    })?;
                    let provider_terminal_failure =
                        if event.data.get("isError").and_then(Value::as_bool) == Some(true) {
                            Some(ProviderTerminalFailure {
                                reason: event
                                    .data
                                    .get("terminalReason")
                                    .and_then(Value::as_str)
                                    .filter(|value| !value.trim().is_empty())
                                    .unwrap_or("unknown_provider_error")
                                    .to_string(),
                                http_status: event
                                    .data
                                    .get("apiErrorStatus")
                                    .and_then(Value::as_i64),
                            })
                        } else if !saw_assistant_message {
                            Some(ProviderTerminalFailure {
                                reason: "empty_final_report".to_string(),
                                http_status: None,
                            })
                        } else {
                            None
                        };
                    return Ok(ExecutionCycleOutcome {
                        final_text,
                        provider_terminal_failure,
                        interrupted: false,
                        close_requested_by_harness: false,
                        tool_call_count,
                        native_correlation: native_cycle_correlation(
                            &input_id,
                            receipt,
                            "turn_complete",
                            &self.native_session_id,
                        ),
                        control_receipts,
                        terminal_observation: self.cycle_observation(true),
                    });
                }
                "interrupted" => {
                    interrupted = true;
                    self.state = TransportState::Interrupted;
                    let still_queued = event.data.get("stillQueued");
                    self.pending_input_count = still_queued
                        .and_then(Value::as_array)
                        .map(|items| items.len() as u64)
                        .unwrap_or(self.pending_input_count);
                    control_receipts.push(ControlTransportReceipt {
                        command: "abort".into(),
                        response_id: Some(format!("deepseek-sdk-interrupt:{input_id}")),
                        success: true,
                    });
                }
                "member_resumed_after_interrupt" if interrupted => {
                    let resumed = event.data.get("sessionId").and_then(Value::as_str);
                    if resumed != Some(self.native_session_id.as_str()) {
                        return Err(CliError::Usage(format!(
                            "DEEPSEEK_HARNESS_INTERRUPT_RESUME_MISMATCH: retained={} resumed={resumed:?}",
                            self.native_session_id
                        )));
                    }
                    self.last_cycle_terminal = true;
                    self.last_interrupt_resumed_same_session = true;
                    self.state = TransportState::Idle;
                    let receipt = input_acceptance_receipt.clone().ok_or_else(|| {
                        CliError::Usage(format!(
                            "DEEPSEEK_HARNESS_PROTOCOL_ERROR: interrupt preceded consumed for {input_id}"
                        ))
                    })?;
                    return Ok(ExecutionCycleOutcome {
                        final_text,
                        provider_terminal_failure: None,
                        interrupted: true,
                        close_requested_by_harness: close_requested,
                        tool_call_count,
                        native_correlation: native_cycle_correlation(
                            &input_id,
                            receipt,
                            "interrupt_resume",
                            &self.native_session_id,
                        ),
                        control_receipts,
                        terminal_observation: self.cycle_observation(true),
                    });
                }
                "runner_error" => {
                    return Err(CliError::Usage(format!(
                        "DEEPSEEK_HARNESS_RUNNER_ERROR: {}",
                        event.data
                    )));
                }
                "member_closed" => {
                    self.state = TransportState::Closed;
                    return Err(CliError::Usage(
                        "DEEPSEEK_HARNESS_UNEXPECTED_CLOSE: member_closed without CloseRuntime"
                            .into(),
                    ));
                }
                _ => {}
            }
        }
    }

    fn cycle_observation(&self, settled: bool) -> CycleObservation {
        let alive = !matches!(
            self.state,
            TransportState::Closed | TransportState::Disconnected
        );
        CycleObservation {
            transport_alive: alive,
            process_alive: alive,
            is_streaming: Some(matches!(self.state, TransportState::Active)),
            pending_message_count: Some(self.pending_input_count),
            steering_mode: Some("unsupported".into()),
            follow_up_mode: Some("harness_safe_boundary".into()),
            settled_boundary_observed: settled,
        }
    }
}

impl Drop for DeepSeekRunnerTransport {
    fn drop(&mut self) {
        // Best-effort graceful close; the process-group guard is the leak-safe
        // fallback when the transport is broken or the Supervisor is stale.
        if !matches!(
            self.state,
            TransportState::Closed | TransportState::Disconnected
        ) {
            let _ = self.close("runtime_handle_dropped");
        }
        self.stdin.take();
    }
}

/// Provider-neutral DeepSeek Harness runtime handle.
pub struct DeepSeekTeamRuntime {
    transport: DeepSeekRunnerTransport,
    description: RuntimeDescription,
    authority_session: Option<AgentSession>,
    canonical_quiesced: bool,
    canonical_released: bool,
}

impl DeepSeekTeamRuntime {
    pub fn spawn(config: DeepSeekTeamRuntimeConfig) -> CliResult<Self> {
        Ok(Self {
            transport: DeepSeekRunnerTransport::spawn(&config)?,
            description: RuntimeDescription {
                binding_id: DEEPSEEK_BINDING_ID.into(),
                native_protocol: DEEPSEEK_NATIVE_PROTOCOL.into(),
                composition_fingerprint: String::new(),
                capability_fingerprint: String::new(),
                capability_bindings: Vec::new(),
            },
            authority_session: None,
            canonical_quiesced: false,
            canonical_released: false,
        })
    }

    fn contract_preflight(
        &self,
        fence: RuntimeBindingFence,
        capability: SemanticCapability,
    ) -> Result<AdmissionDecision, RuntimeContractError> {
        let session =
            self.authority_session
                .as_ref()
                .ok_or_else(|| RuntimeContractError::FenceMismatch {
                    fields: vec!["authority_session".into()],
                })?;
        harness_runtime_contract::preflight_effect(
            &self.description,
            session,
            fence,
            capability,
            &[],
        )
    }

    fn contract_observation(&self) -> harness_runtime_contract::RuntimeObservation {
        let session = self
            .authority_session
            .as_ref()
            .expect("contract observation requires bound session");
        harness_runtime_contract::RuntimeObservation {
            native_session_ref: (!self.transport.native_session_id.is_empty())
                .then(|| self.transport.native_session_id.clone()),
            active_effect_id: None,
            continuation: session.control_state.continuation.clone(),
            observed_at: now_string(),
        }
    }

    /// Team Close is intentionally weaker and differently named than strict
    /// quiesce/release. It closes the owned DSH AgentHandle/process and keeps
    /// the exact native session id available for Reopen. It does not claim
    /// workspace-job drain or durable transcript flush.
    pub fn close_owned_runtime(&mut self, reason: &str) -> CliResult<DeepSeekCloseEvidence> {
        if matches!(self.transport.state, TransportState::Active) {
            return Err(CliError::Usage(
                "DEEPSEEK_CLOSE_REQUIRES_TERMINAL_CYCLE: interrupt the current cycle and observe same-session resume first"
                    .into(),
            ));
        }
        if !self.transport.last_cycle_terminal
            && !matches!(
                self.transport.state,
                TransportState::Starting | TransportState::Idle
            )
        {
            return Err(CliError::Usage(
                "DEEPSEEK_CLOSE_REQUIRES_TERMINAL_CYCLE: no terminal cycle boundary was observed"
                    .into(),
            ));
        }
        let active_cycle_terminal = !matches!(self.transport.state, TransportState::Active);
        let retained = self.transport.native_session_id.clone();
        self.transport.close(reason)?;
        let event = self
            .transport
            .wait_for_member_closed(GRACEFUL_CLOSE_TIMEOUT)?;
        let acknowledged_reason = event
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string);
        if acknowledged_reason.as_deref() != Some(reason) {
            return Err(CliError::Usage(format!(
                "DEEPSEEK_HARNESS_CLOSE_ACK_MISMATCH: requested={reason:?} acknowledged={acknowledged_reason:?}"
            )));
        }
        let undelivered = event
            .get("undelivered")
            .and_then(Value::as_array)
            .map(|items| items.len() as u64);
        Ok(DeepSeekCloseEvidence {
            native_session_id: (!retained.is_empty()).then_some(retained),
            active_cycle_terminal,
            owned_runtime_closed: matches!(self.transport.state, TransportState::Closed),
            native_session_retained: true,
            acknowledged_reason,
            undelivered_input_count: undelivered,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepSeekCloseEvidence {
    pub native_session_id: Option<String>,
    pub active_cycle_terminal: bool,
    pub owned_runtime_closed: bool,
    pub native_session_retained: bool,
    pub acknowledged_reason: Option<String>,
    /// Observation only. Team Close does not overclaim a strong native-queue
    /// drain postcondition from this count.
    pub undelivered_input_count: Option<u64>,
}

struct DeepSeekControlFlags<'a> {
    close: &'a mut bool,
    interrupt: &'a mut bool,
}

impl ProviderNativeControl for DeepSeekControlFlags<'_> {
    fn provider(&self) -> &'static str {
        "deepseek_harness"
    }

    fn dispatch(&mut self, plan: &ProviderControlPlan) -> Result<(), String> {
        if plan.primitive != NativeControlPrimitive::DeepSeekHarnessCancel {
            return Err(format!(
                "PROVIDER_CONTROL_UNPROVEN: DeepSeek adapter received {:?}",
                plan.primitive
            ));
        }
        *self.interrupt = true;
        *self.close = plan.action == ProviderControlAction::CloseSession;
        Ok(())
    }
}

impl TeamRuntimeAdapter for DeepSeekTeamRuntime {
    type Error = CliError;

    fn provider(&self) -> &'static str {
        "deepseek_harness"
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn capability_bindings() -> Vec<CapabilityBinding> {
        vec![
            supported(
                "open_or_resume",
                "DSH ctx.agents.create/resume binds the exact native Session ID on 0.1.1-rc.2",
            ),
            supported(
                "start_cycle",
                "Agent.followup input with matching agent/inbox/spliced receipt; matching turn_complete(triggerMessageId) is the terminal boundary",
            ),
            unsupported(
                "inject_current_cycle",
                "DSH followup is reserved for the next safe cycle; steer is not claimed",
            ),
            unsupported(
                "queue_at_native_boundary",
                "ordinary Messages remain on the Harness queue until the next safe cycle boundary",
            ),
            supported(
                "interrupt_current_cycle",
                "Agent.cancel + whenIdle + flush on the same native DSH Session",
            ),
            supported(
                "observe",
                "owned runner process/stdio liveness and typed runner lifecycle events; no transcript mirroring",
            ),
            unsupported(
                "inspect_effect",
                "the native bridge exposes input receipt and terminal boundaries but no stable effect-inspection API",
            ),
            unsupported(
                "reconcile_effect",
                "no stable provider operation id exists for exact effect reconciliation",
            ),
            unsupported(
                "inspect_continuation",
                "DSH Goal plugins are deliberately absent from the host-driven composition",
            ),
            unsupported(
                "inhibit_continuation",
                "host-driven mode never activates DeepSeek /goal; the adapter cannot control it",
            ),
            unsupported(
                "resume_continuation",
                "host-driven mode never activates DeepSeek /goal; the adapter cannot control it",
            ),
            CapabilityBinding {
                capability: "quiesce",
                status: CapabilityStatus::Degraded,
                evidence: "interrupt + same-session resume can prove a cycle boundary, but the current runner cannot prove FullAccess writable-child drain or durable provider-store flush".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "release",
                status: CapabilityStatus::Degraded,
                evidence: "strict Release depends on a verified Quiesce; use the separately authorized Team CloseRuntime operation to close the owned DSH AgentHandle while retaining the native session".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "close_runtime",
                status: CapabilityStatus::Supported,
                evidence: "terminal cycle acknowledgement followed by runner close, member_closed(sessionId), and owned process-group exit; native session id retained for Reopen".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "permission_enforcement",
                status: CapabilityStatus::Degraded,
                evidence: "read-only/workspace-write/danger-full-access compile into the shared DSH sandbox policy used by bash and filesystem tools".into(),
                security_enforcement_locus: Some("dsh_sandbox_policy_and_sandbox_backends".into()),
            },
        ]
    }

    fn ensure_alive(&mut self) -> CliResult<()> {
        self.transport.ensure_alive()
    }

    fn native_session_locator(&self) -> &str {
        &self.transport.native_session_id
    }

    fn native_locator_kind(&self) -> &'static str {
        "deepseek_harness_session"
    }

    fn bind_authority_session(
        &mut self,
        session: AgentSession,
        profile: &harness_core::ProviderIntegrationProfile,
    ) -> CliResult<()> {
        if session.provider_kind != "deepseek_harness"
            || profile.provider != "deepseek_harness"
            || profile.execution_mode != "deepseek_sdk"
        {
            return Err(CliError::Usage(format!(
                "RUNTIME_ADAPTER_PROVIDER_MISMATCH: DeepSeek adapter cannot bind session={} profile={}:{}",
                session.provider_kind, profile.provider, profile.execution_mode
            )));
        }
        if profile.provider_version.as_deref() != Some(REVIEWED_DEEPSEEK_HARNESS_VERSION) {
            return Err(CliError::Usage(format!(
                "DEEPSEEK_HARNESS_VERSION_UNREVIEWED: profile must bind exact DSH {}, got {:?}",
                REVIEWED_DEEPSEEK_HARNESS_VERSION, profile.provider_version
            )));
        }
        let composition = matching_fingerprint(
            profile.composition_fingerprint.as_deref(),
            session.control_state.composition_fingerprint.as_deref(),
            "composition",
        )?;
        let capabilities = matching_fingerprint(
            profile.capability_fingerprint.as_deref(),
            session.control_state.capability_fingerprint.as_deref(),
            "capability",
        )?;
        if let Some(native) = session.native_session_ref.as_ref() {
            if !self.transport.native_session_id.is_empty()
                && native.native_session_id != self.transport.native_session_id
            {
                return Err(CliError::Usage(format!(
                    "DEEPSEEK_HARNESS_RESUME_MISMATCH: authority={} runtime={}",
                    native.native_session_id, self.transport.native_session_id
                )));
            }
        }
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
        on_input_accepted: &mut dyn FnMut(&ControlTransportReceipt) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(&SteerRequest, &SteerProviderResult) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> CycleControl,
    ) -> CliResult<ExecutionCycleOutcome> {
        self.transport.run_cycle(
            input,
            idle_timeout,
            on_input_accepted,
            on_steer_result,
            on_event,
            poll_control,
        )
    }

    fn project_live(
        event: &Value,
    ) -> Option<(harness_runtime_contract::LiveProviderActivityKind, String)> {
        if event.get("event").and_then(Value::as_str) == Some("provider_activity") {
            use harness_runtime_contract::LiveProviderActivityKind;
            let data = event.get("data")?;
            let kind = match data.get("kind").and_then(Value::as_str)? {
                "thinking" => LiveProviderActivityKind::Thinking,
                "response_streaming" => LiveProviderActivityKind::ResponseStreaming,
                "tool_started" => LiveProviderActivityKind::ToolStarted,
                "tool_completed" => LiveProviderActivityKind::ToolCompleted,
                "tool_failed" => LiveProviderActivityKind::ToolFailed,
                "interaction_waiting" => LiveProviderActivityKind::InteractionWaiting,
                _ => return None,
            };
            let summary = data
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("provider activity")
                .chars()
                .take(240)
                .collect();
            return Some((kind, summary));
        }
        if event.get("event").and_then(Value::as_str) != Some("assistant_message") {
            return None;
        }
        let data = event.get("data")?;
        let (text, tools) = assistant_projection(data);
        if tools > 0 {
            Some((
                harness_runtime_contract::LiveProviderActivityKind::ToolStarted,
                "tool started".into(),
            ))
        } else if !text.is_empty() {
            Some((
                harness_runtime_contract::LiveProviderActivityKind::ResponseStreaming,
                format!(
                    "assistant response streaming · {} chars",
                    text.chars().count()
                ),
            ))
        } else {
            None
        }
    }

    fn native_control<'a>(
        close: &'a mut bool,
        interrupt: &'a mut bool,
    ) -> Box<dyn ProviderNativeControl + 'a> {
        Box::new(DeepSeekControlFlags { close, interrupt })
    }
}

impl RuntimeAdapter for DeepSeekTeamRuntime {
    fn describe(&self) -> &RuntimeDescription {
        &self.description
    }

    fn open_or_resume(
        &mut self,
        fence: RuntimeBindingFence,
        native_session_ref: Option<&str>,
    ) -> Result<harness_runtime_contract::RuntimeObservation, RuntimeContractError> {
        self.contract_preflight(fence, SemanticCapability::OpenOrResume)?;
        self.transport
            .ensure_alive()
            .map_err(contract_bridge_error)?;
        if native_session_ref.is_some_and(|expected| {
            !self.transport.native_session_id.is_empty()
                && expected != self.transport.native_session_id
        }) {
            return Err(RuntimeContractError::FenceMismatch {
                fields: vec!["native_session_ref.native_session_id".into()],
            });
        }
        Ok(self.contract_observation())
    }

    fn execute_control(
        &mut self,
        fence: RuntimeBindingFence,
        request: ControlRequest,
    ) -> Result<EffectReceipt, RuntimeContractError> {
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
        match request.intent {
            ControlIntent::StartCycle { input } => {
                let mut accepted = None;
                let outcome = self
                    .transport
                    .run_cycle(
                        &input,
                        Duration::from_secs(30 * 60),
                        &mut |receipt| {
                            accepted = receipt.response_id.clone();
                            Ok(())
                        },
                        &mut |_pending, _result| Ok(()),
                        &mut |_event| {},
                        &mut CycleControl::default,
                    )
                    .map_err(contract_bridge_error)?;
                let provider_receipt = accepted.ok_or_else(|| {
                    contract_bridge_error("cycle completed without consumed input receipt")
                })?;
                Ok(EffectReceipt {
                    effect_id: request.effect_id,
                    certainty: RuntimeEffectCertainty::Applied,
                    postcondition: RuntimePostconditionStatus::Satisfied,
                    admission: admission.admission,
                    native_evidence: vec![
                        provider_receipt,
                        format!(
                            "deepseek.turn_complete:session={}",
                            self.transport.native_session_id
                        ),
                        format!(
                            "deepseek.terminal:settled={}",
                            outcome.terminal_observation.settled_boundary_observed
                        ),
                    ],
                })
            }
            ControlIntent::Interrupt => {
                self.transport.interrupt().map_err(contract_bridge_error)?;
                Ok(EffectReceipt {
                    effect_id: request.effect_id,
                    certainty: RuntimeEffectCertainty::Applied,
                    // The write is not terminal proof; run_cycle waits for
                    // interrupted + same-session resume before satisfying it.
                    postcondition: RuntimePostconditionStatus::Unknown,
                    admission: admission.admission,
                    native_evidence: vec!["deepseek.query.interrupt dispatched".into()],
                })
            }
            _ => unreachable!("unsupported DeepSeek control must fail canonical preflight"),
        }
    }

    fn observe(
        &mut self,
        fence: RuntimeBindingFence,
    ) -> Result<harness_runtime_contract::RuntimeObservation, RuntimeContractError> {
        self.contract_preflight(fence, SemanticCapability::Observe)?;
        self.transport
            .ensure_alive()
            .map_err(contract_bridge_error)?;
        Ok(self.contract_observation())
    }

    fn close_runtime(
        &mut self,
        fence: RuntimeBindingFence,
    ) -> Result<MemberRuntimeCloseReceipt, RuntimeContractError> {
        self.contract_preflight(fence, SemanticCapability::CloseRuntime)?;
        let evidence = self
            .close_owned_runtime("harness_team_close")
            .map_err(contract_bridge_error)?;
        let satisfied = RuntimePostconditionStatus::Satisfied;
        let receipt = MemberRuntimeCloseReceipt {
            control_acknowledged: if evidence.acknowledged_reason.as_deref()
                == Some("harness_team_close")
            {
                satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            current_cycle_terminal: if evidence.active_cycle_terminal {
                satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            managed_runtime_released: if evidence.owned_runtime_closed {
                satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            live_handle_disposed: if matches!(self.transport.state, TransportState::Closed)
                && self.transport.stdin.is_none()
            {
                satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            native_session_retained: if evidence.native_session_retained
                && evidence.native_session_id.as_deref()
                    == (!self.transport.native_session_id.is_empty())
                        .then_some(self.transport.native_session_id.as_str())
            {
                satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            evidence: vec![
                format!(
                    "deepseek.member_closed:reason={}",
                    evidence.acknowledged_reason.as_deref().unwrap_or("unknown")
                ),
                format!(
                    "deepseek.owned_process_group:pid={}:exited=true",
                    self.transport.child.id()
                ),
                format!(
                    "deepseek.native_session_retained:{}",
                    evidence.native_session_id.as_deref().unwrap_or("none_created")
                ),
                format!(
                    "deepseek.undelivered_input_count={:?} (observation only; not a strict queue-drain claim)",
                    evidence.undelivered_input_count
                ),
            ],
        };
        receipt.verify()?;
        Ok(receipt)
    }

    fn inspect_effect(
        &mut self,
        fence: RuntimeBindingFence,
        _effect_id: &str,
    ) -> Result<EffectInspection, RuntimeContractError> {
        self.contract_preflight(fence, SemanticCapability::InspectEffect)?;
        unreachable!("DeepSeek inspect_effect is not admitted")
    }

    fn reconcile(
        &mut self,
        fence: RuntimeBindingFence,
        _inspection: &EffectInspection,
    ) -> Result<ReconcileReceipt, RuntimeContractError> {
        self.contract_preflight(fence, SemanticCapability::Reconcile)?;
        unreachable!("DeepSeek reconcile is not admitted")
    }

    fn quiesce(
        &mut self,
        fence: RuntimeBindingFence,
    ) -> Result<QuiesceReceipt, RuntimeContractError> {
        // A profile that honestly carries the Degraded binding fails here
        // before a provider effect. Building the receipt below keeps a future
        // accidental Verified claim fail-closed as well.
        self.contract_preflight(fence, SemanticCapability::Quiesce)?;
        let session = self
            .authority_session
            .as_ref()
            .expect("preflight bound session");
        let continuation = if matches!(
            session.control_state.continuation.activation,
            NativeContinuationActivation::Disarmed
        ) {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unknown
        };
        let terminal = if self.transport.last_cycle_terminal
            && !matches!(self.transport.state, TransportState::Active)
        {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unknown
        };
        let queue = if self.transport.pending_input_count == 0 {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unsatisfied
        };
        let flush = RuntimePostconditionStatus::Unknown;
        let mut builder = QuiesceReceiptBuilder::new();
        builder.record(
            QuiesceStep::FenceAdmission,
            RuntimePostconditionStatus::Satisfied,
            "exact RuntimeBindingFence admitted",
        )?;
        builder.record(
            QuiesceStep::InhibitContinuation,
            continuation,
            "DeepSeek /goal is not activated by the host-driven adapter",
        )?;
        builder.record(
            QuiesceStep::SettleActiveCycle,
            terminal,
            "typed turn_complete or interrupt+same-session-resume boundary",
        )?;
        builder.record(
            QuiesceStep::DrainNativeQueue,
            queue,
            format!(
                "runner pending input count={}",
                self.transport.pending_input_count
            ),
        )?;
        builder.record(
            QuiesceStep::DrainWritableChildren,
            RuntimePostconditionStatus::Unknown,
            "danger-full-access DSH exposes no complete writable-child/job inventory",
        )?;
        builder.record(
            QuiesceStep::ObserveIdle,
            terminal,
            format!("runner state={:?}", self.transport.state),
        )?;
        builder.record(
            QuiesceStep::ConfirmFlush,
            flush,
            "DSH persistence flush is acknowledged per cycle, but strict process-independent fsync evidence is not yet exposed by the bridge",
        )?;
        let receipt = builder.finish();
        receipt.verify()?;
        self.canonical_quiesced = true;
        Ok(receipt)
    }

    fn release(
        &mut self,
        fence: RuntimeBindingFence,
    ) -> Result<ReleaseReceipt, RuntimeContractError> {
        if self.canonical_released {
            return Err(RuntimeContractError::AlreadyReleased);
        }
        self.contract_preflight(fence, SemanticCapability::Release)?;
        if !self.canonical_quiesced {
            return Err(RuntimeContractError::CompositionSwapRequiresQuiesce);
        }
        // Strict Release is intentionally unreachable for the current
        // FullAccess composition because strict Quiesce cannot be verified.
        Err(RuntimeContractError::ReleaseIncomplete)
    }
}

fn supported(capability: &'static str, evidence: &'static str) -> CapabilityBinding {
    CapabilityBinding {
        capability,
        status: CapabilityStatus::Supported,
        evidence: evidence.into(),
        security_enforcement_locus: None,
    }
}

fn unsupported(capability: &'static str, evidence: &'static str) -> CapabilityBinding {
    CapabilityBinding {
        capability,
        status: CapabilityStatus::Unsupported,
        evidence: evidence.into(),
        security_enforcement_locus: None,
    }
}

fn matching_fingerprint(
    profile: Option<&str>,
    session: Option<&str>,
    label: &str,
) -> CliResult<String> {
    match (profile, session) {
        (Some(profile), Some(session)) if profile == session => Ok(profile.to_string()),
        _ => Err(CliError::Usage(format!(
            "RUNTIME_ADAPTER_FENCE_INCOMPLETE: persisted profile/session {label} fingerprint mismatch"
        ))),
    }
}

fn contract_bridge_error(error: impl std::fmt::Display) -> RuntimeContractError {
    RuntimeContractError::InvalidCapabilityBindings(format!(
        "DeepSeek Harness native bridge operation failed: {error}"
    ))
}

fn assistant_projection(data: &Value) -> (String, u32) {
    let mut text = String::new();
    let mut tools = 0u32;
    for block in data
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push_str(value);
                }
            }
            Some("tool_use") => tools = tools.saturating_add(1),
            // Never project provider-private thinking into final text or
            // durable Harness records.
            Some("thinking" | "redacted_thinking") => {}
            _ => {}
        }
    }
    (text, tools)
}

#[cfg(test)]
mod tests;
