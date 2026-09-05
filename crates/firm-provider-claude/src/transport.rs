//! Persistent NDJSON transport to the Agent SDK runner: process ownership,
//! frame parsing, and the cycle driver for `ClaudeTeamRuntime`.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportState {
    Starting,
    Idle,
    Active,
    Interrupted,
    Closed,
    Disconnected,
}

#[derive(Debug)]
pub(crate) struct RunnerEvent {
    pub(crate) name: String,
    pub(crate) data: Value,
    raw: Value,
}

impl RunnerEvent {
    pub(crate) fn parse(line: &str) -> CliResult<Self> {
        let raw: Value = serde_json::from_str(line).map_err(|error| {
            CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: invalid runner JSON: {error}"
            ))
        })?;
        let name = raw
            .get("event")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                CliError::Usage(
                    "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: runner event is missing `event`".into(),
                )
            })?
            .to_string();
        let data = raw.get("data").cloned().unwrap_or(Value::Null);
        runner_contract::validate_runner_frame("eventPayloadSchemas", "event", "data", &raw)?;
        Ok(Self { name, data, raw })
    }
}

/// Owns the runner group so stale Supervisors cannot orphan descendants.
pub(crate) struct ClaudeRunnerChild {
    child: Child,
    process_group: OwnedProcessGroupRegistration,
    armed: bool,
}

impl ClaudeRunnerChild {
    pub(crate) fn new(mut child: Child) -> CliResult<Self> {
        let process_group = OwnedProcessGroupRegistration::new(&mut child)?;
        Ok(Self {
            child,
            process_group,
            armed: true,
        })
    }

    pub(crate) fn id(&self) -> u32 {
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

impl Drop for ClaudeRunnerChild {
    fn drop(&mut self) {
        self.terminate_group();
    }
}

/// Persistent NDJSON transport to the Agent SDK runner.
pub(crate) struct ClaudeRunnerTransport {
    pub(crate) child: ClaudeRunnerChild,
    pub(crate) stdin: Option<ChildStdin>,
    pub(crate) lines: Receiver<String>,
    pub(crate) stdout_reader: Option<JoinHandle<()>>,
    pub(crate) stderr_reader: Option<JoinHandle<String>>,
    pub(crate) native_session_id: String,
    pub(crate) expected_resume_session_id: Option<String>,
    pub(crate) provider_version: Option<String>,
    pub(crate) state: TransportState,
    pub(crate) next_input_id: u64,
    pub(crate) pending_input_count: u64,
    pub(crate) last_cycle_terminal: bool,
    pub(crate) last_interrupt_resumed_same_session: bool,
    pub(crate) close_reason: Option<String>,
}

impl ClaudeRunnerTransport {
    pub(crate) fn spawn(config: &ClaudeTeamRuntimeConfig) -> CliResult<Self> {
        verify_runner_sdk_version(&config.runner_path)?;
        // Validate and freeze the shared Rust/Node protocol before spawning
        // the runner or allowing it to load the provider SDK.
        let start_frame = config.start_frame()?;

        let mut command = Command::new("node");
        apply_collaboration_environment(&mut command, &config.environment);
        command
            .arg(&config.runner_path)
            .current_dir(&config.cwd)
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
                "failed to spawn Claude Agent SDK runner {}: {error}",
                config.runner_path.display()
            ))
        })?;
        let mut child = ClaudeRunnerChild::new(child)?;
        let stdin = child
            .child
            .stdin
            .take()
            .ok_or_else(|| CliError::Usage("Claude runner stdin unavailable".into()))?;
        let stdout = child
            .child
            .stdout
            .take()
            .ok_or_else(|| CliError::Usage("Claude runner stdout unavailable".into()))?;
        let stderr = child
            .child
            .stderr
            .take()
            .ok_or_else(|| CliError::Usage("Claude runner stderr unavailable".into()))?;

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
        Ok(transport)
    }

    fn write_frame(&mut self, frame: &Value) -> CliResult<()> {
        runner_contract::validate_runner_frame(
            "commandPayloadSchemas",
            "command",
            "payload",
            frame,
        )?;
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            CliError::Usage("CLAUDE_AGENT_SDK_TRANSPORT_CLOSED: runner stdin is closed".into())
        })?;
        serde_json::to_writer(&mut *stdin, frame)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    pub(crate) fn ensure_alive(&mut self) -> CliResult<()> {
        if matches!(
            self.state,
            TransportState::Closed | TransportState::Disconnected
        ) {
            return Err(CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_TRANSPORT_CLOSED: state={:?}",
                self.state
            )));
        }
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(status)) => {
                self.state = TransportState::Disconnected;
                Err(CliError::Usage(format!(
                    "CLAUDE_AGENT_SDK_TRANSPORT_CLOSED: runner exited with {status}"
                )))
            }
            Err(error) => Err(CliError::Usage(format!(
                "failed to inspect Claude Agent SDK runner: {error}"
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
                    "CLAUDE_AGENT_SDK_TRANSPORT_CLOSED: runner stdout disconnected{}",
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
                    "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: session_bound lacked sessionId".into(),
                )
            })?;
        let version = event
            .data
            .get("providerVersion")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                CliError::Usage(
                    "CLAUDE_AGENT_SDK_VERSION_UNVERIFIED: session_bound lacked providerVersion"
                        .into(),
                )
            })?;
        if version != REVIEWED_CLAUDE_CODE_VERSION {
            return Err(CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_VERSION_UNREVIEWED: expected Claude Code {}, observed {version}",
                REVIEWED_CLAUDE_CODE_VERSION
            )));
        }
        if let Some(expected) = self.expected_resume_session_id.as_deref() {
            if expected != session_id {
                return Err(CliError::Usage(format!(
                    "CLAUDE_AGENT_SDK_RESUME_MISMATCH: expected native session {expected}, observed {session_id}"
                )));
            }
        }
        if !self.native_session_id.is_empty() && self.native_session_id != session_id {
            return Err(CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_SESSION_CHANGED: runtime generation changed native session {} -> {session_id}",
                self.native_session_id
            )));
        }
        self.native_session_id = session_id.to_string();
        self.provider_version = Some(version.to_string());
        if matches!(self.state, TransportState::Starting) {
            self.state = TransportState::Idle;
        }
        Ok(())
    }

    fn send_input(&mut self, input: &str) -> CliResult<String> {
        self.ensure_alive()?;
        self.next_input_id += 1;
        let input_id = format!("claude-cycle-{}", self.next_input_id);
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

    pub(crate) fn interrupt(&mut self) -> CliResult<()> {
        self.write_frame(&json!({"command": "interrupt", "payload": {}}))
    }

    pub(crate) fn close(&mut self, reason: &str) -> CliResult<()> {
        if matches!(self.state, TransportState::Closed) {
            return Ok(());
        }
        self.close_reason = Some(reason.to_string());
        self.write_frame(&json!({
            "command": "close",
            "payload": {"reason": reason},
        }))
    }

    pub(crate) fn wait_for_member_closed(&mut self, timeout: Duration) -> CliResult<Value> {
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(CliError::Usage(
                    "CLAUDE_AGENT_SDK_CLOSE_TIMEOUT: runner did not emit member_closed".into(),
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
                            "CLAUDE_AGENT_SDK_CLOSE_SESSION_MISMATCH: retained={} event={session_id:?}",
                            self.native_session_id
                        )));
                    }
                    self.state = TransportState::Closed;
                    self.stdin.take();
                    if self.child.wait_until(GRACEFUL_CLOSE_TIMEOUT)?.is_none() {
                        return Err(CliError::Usage(
                            "CLAUDE_AGENT_SDK_CLOSE_TIMEOUT: owned runner process group did not exit"
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
                        "CLAUDE_AGENT_SDK_RUNNER_ERROR: {}",
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
    pub(crate) fn run_cycle(
        &mut self,
        input: &str,
        timeouts: CycleTimeouts,
        on_input_accepted: &mut dyn FnMut(&ControlTransportReceipt) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(&SteerRequest, &SteerProviderResult) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> CycleControl,
    ) -> CliResult<ExecutionCycleOutcome> {
        let input_id = self.send_input(input)?;
        let input_sent_at = Instant::now();
        let mut final_text = String::new();
        let mut input_acceptance_receipt = None;
        let mut control_receipts = Vec::new();
        let mut tool_call_count = 0u32;
        let mut saw_assistant_message = false;
        let mut interrupt_sent = false;
        let mut interrupt_sent_at: Option<Instant> = None;
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
                        "CLAUDE_CURRENT_CYCLE_INJECTION_UNSUPPORTED: use an ordinary queued Message"
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
                interrupt_sent_at = Some(Instant::now());
            }

            let Some(event) = self.receive_event(CONTROL_POLL)? else {
                // A healthy Claude turn may legitimately run for hours, and a
                // provider tool may be silent while it does real work. The
                // caller's timeout therefore fences only the unacknowledged
                // delivery boundary; it is not a hidden wall-clock limit on
                // an accepted cycle. `transport_liveness` (Spec D2) is proven
                // by `ensure_alive()` and by child-exit/stdout-disconnect
                // failing closed — never by a silence verdict.
                self.ensure_alive()?;
                // A5/D3: an issued Interrupt that the provider never
                // acknowledges expires after control_settle — Unknown, never
                // a cycle failure and never a silent hang.
                if let Some(sent_at) = interrupt_sent_at {
                    if sent_at.elapsed() >= timeouts.control_settle {
                        return Err(CliError::Usage(format!(
                            "CLAUDE_AGENT_SDK_CONTROL_SETTLE_TIMEOUT: interrupt was not acknowledged within {}s",
                            timeouts.control_settle.as_secs()
                        )));
                    }
                }
                if input_acceptance_receipt.is_none()
                    && input_sent_at.elapsed() >= timeouts.input_acceptance
                {
                    return Err(CliError::Usage(format!(
                        "CLAUDE_AGENT_SDK_INPUT_ACCEPTANCE_TIMEOUT: cycle {input_id} was not consumed within {}s",
                        timeouts.input_acceptance.as_secs()
                    )));
                }
                continue;
            };
            // `session_bound` is the provider's exact native-session proof.
            // Verify and freeze it before exposing the raw event to the
            // application callback so a later timeout cannot erase a valid
            // binding or make the callback persist unverified provider data.
            if event.name == "session_bound" {
                self.accept_session_binding(&event)?;
            }
            on_event(&event.raw);
            match event.name.as_str() {
                "session_bound" => {}
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
                            response_id: Some(format!("claude-sdk-session:{session}:{input_id}")),
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
                            "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: turn_complete preceded consumed for {input_id}"
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
                        interrupt: None,
                        close_requested_by_harness: false,
                        tool_call_count,
                        native_correlation: cycle_ref(&input_id, receipt, "turn_complete"),
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
                        response_id: Some(format!("claude-sdk-interrupt:{input_id}")),
                        success: true,
                    });
                }
                "member_resumed_after_interrupt" if interrupted => {
                    let resumed = event.data.get("sessionId").and_then(Value::as_str);
                    if resumed != Some(self.native_session_id.as_str()) {
                        return Err(CliError::Usage(format!(
                            "CLAUDE_AGENT_SDK_INTERRUPT_RESUME_MISMATCH: retained={} resumed={resumed:?}",
                            self.native_session_id
                        )));
                    }
                    self.last_cycle_terminal = true;
                    self.last_interrupt_resumed_same_session = true;
                    self.state = TransportState::Idle;
                    let receipt = input_acceptance_receipt.clone().ok_or_else(|| {
                        CliError::Usage(format!(
                            "CLAUDE_AGENT_SDK_PROTOCOL_ERROR: interrupt preceded consumed for {input_id}"
                        ))
                    })?;
                    return Ok(ExecutionCycleOutcome {
                        final_text,
                        provider_terminal_failure: None,
                        interrupt: Some(InterruptCause::HostControl),
                        close_requested_by_harness: close_requested,
                        tool_call_count,
                        native_correlation: cycle_ref(&input_id, receipt, "interrupt_resume"),
                        control_receipts,
                        terminal_observation: self.cycle_observation(true),
                    });
                }
                "runner_error" => {
                    return Err(CliError::Usage(format!(
                        "CLAUDE_AGENT_SDK_RUNNER_ERROR: {}",
                        event.data
                    )));
                }
                "member_closed" => {
                    self.state = TransportState::Closed;
                    return Err(CliError::Usage(
                        "CLAUDE_AGENT_SDK_UNEXPECTED_CLOSE: member_closed without CloseRuntime"
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

impl Drop for ClaudeRunnerTransport {
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
