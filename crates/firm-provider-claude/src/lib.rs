//! Claude Agent SDK transport; durable identity, Work, Messages, and
//! RuntimeCommands remain outside it. Each disposable generation resumes the
//! native session. Only Claude Code 2.1.220 with SDK 0.3.220 is reviewed.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
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
    CycleSettlement, CycleTimeouts, EffectInspection, EffectReceipt, ExecutionCycleOutcome,
    InterruptCause, MemberRuntimeCloseReceipt, NativeControlPrimitive, ProviderControlAction,
    ProviderControlPlan, ProviderNativeControl, ProviderTerminalFailure, QuiesceReceipt,
    QuiesceReceiptBuilder, QuiesceStep, ReconcileReceipt, ReleaseReceipt, RuntimeAdapter,
    RuntimeBindingFence, RuntimeContractError, RuntimeDescription, SemanticCapability,
    SteerProviderResult, SteerRequest, TeamRuntimeAdapter,
};

mod cycle_correlation;
use cycle_correlation::cycle_ref;

mod capability_transport;
mod compatibility;
mod error;
mod host_runtime;
mod permission;
mod resident;
mod runner_contract;
mod transport;
pub use capability_transport::*;
pub use compatibility::*;
pub use error::{ClaudeError, ClaudeResult};
pub use host_runtime::*;
pub use permission::*;
pub use resident::*;
#[cfg(test)]
pub(crate) use transport::{ClaudeRunnerChild, RunnerEvent};
pub(crate) use transport::{ClaudeRunnerTransport, TransportState};

type CliResult<T> = ClaudeResult<T>;
type CliError = ClaudeError;

fn now_string() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

pub const REVIEWED_CLAUDE_CODE_VERSION: &str = "2.1.220";
pub const REVIEWED_CLAUDE_AGENT_SDK_VERSION: &str = "0.3.220";
const CLAUDE_BINDING_ID: &str = "claude-agent-sdk-0.3.220+claude-code-2.1.220";
const CLAUDE_NATIVE_PROTOCOL: &str = "claude-agent-sdk-ndjson-v1";
const CONTROL_POLL: Duration = Duration::from_millis(25);
const GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Owned spawn configuration. The caller resolves Workspace/project cwd and
/// supplies only the collaboration environment authorized for this member.
#[derive(Debug)]
pub struct ClaudeTeamRuntimeConfig {
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

/// Provider-neutral Claude Agent SDK runtime handle.
pub struct ClaudeTeamRuntime {
    transport: ClaudeRunnerTransport,
    description: RuntimeDescription,
    authority_session: Option<AgentSession>,
    canonical_quiesced: bool,
    canonical_released: bool,
}

impl ClaudeTeamRuntime {
    pub fn spawn(config: ClaudeTeamRuntimeConfig) -> CliResult<Self> {
        Ok(Self {
            transport: ClaudeRunnerTransport::spawn(&config)?,
            description: RuntimeDescription {
                binding_id: CLAUDE_BINDING_ID.into(),
                native_protocol: CLAUDE_NATIVE_PROTOCOL.into(),
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
    /// quiesce/release. It closes the owned SDK Query/process handle and keeps
    /// the exact native session id available for Reopen. It does not claim
    /// workspace-job drain or durable transcript flush.
    pub fn close_owned_runtime(&mut self, reason: &str) -> CliResult<ClaudeCloseEvidence> {
        if matches!(self.transport.state, TransportState::Active) {
            return Err(CliError::Usage(
                "CLAUDE_CLOSE_REQUIRES_TERMINAL_CYCLE: interrupt the current cycle and observe same-session resume first"
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
                "CLAUDE_CLOSE_REQUIRES_TERMINAL_CYCLE: no terminal cycle boundary was observed"
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
                "CLAUDE_AGENT_SDK_CLOSE_ACK_MISMATCH: requested={reason:?} acknowledged={acknowledged_reason:?}"
            )));
        }
        let undelivered = event
            .get("undelivered")
            .and_then(Value::as_array)
            .map(|items| items.len() as u64);
        Ok(ClaudeCloseEvidence {
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
pub struct ClaudeCloseEvidence {
    pub native_session_id: Option<String>,
    pub active_cycle_terminal: bool,
    pub owned_runtime_closed: bool,
    pub native_session_retained: bool,
    pub acknowledged_reason: Option<String>,
    /// Observation only. Team Close does not overclaim a strong native-queue
    /// drain postcondition from this count.
    pub undelivered_input_count: Option<u64>,
}

struct ClaudeControlFlags<'a> {
    close: &'a mut bool,
    interrupt: &'a mut bool,
}

impl ProviderNativeControl for ClaudeControlFlags<'_> {
    fn provider(&self) -> &'static str {
        "claude"
    }

    fn dispatch(&mut self, plan: &ProviderControlPlan) -> Result<(), String> {
        if plan.primitive != NativeControlPrimitive::ClaudeAgentSdkInterrupt {
            return Err(format!(
                "PROVIDER_CONTROL_UNPROVEN: Claude adapter received {:?}",
                plan.primitive
            ));
        }
        *self.interrupt = true;
        *self.close = plan.action == ProviderControlAction::CloseSession;
        Ok(())
    }
}

impl TeamRuntimeAdapter for ClaudeTeamRuntime {
    type Error = CliError;

    fn provider(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude"
    }

    fn capability_bindings() -> Vec<CapabilityBinding> {
        vec![
            supported(
                "open_or_resume",
                "Agent SDK system(init).session_id plus query({resume}) on exact Claude Code 2.1.220 / SDK 0.3.220",
            ),
            supported(
                "start_cycle",
                "AsyncIterable streaming input; consumed(id,sessionId) accepts input and matching turn_complete(triggerMessageId) is the terminal boundary",
            ),
            unsupported(
                "inject_current_cycle",
                "Agent SDK mailbox input is not a reviewed same-cycle content-steer primitive",
            ),
            unsupported(
                "queue_at_native_boundary",
                "ordinary Messages remain on the Harness queue until the next safe cycle boundary",
            ),
            supported(
                "interrupt_current_cycle",
                "query.interrupt + query iterator retirement + member_resumed_after_interrupt on the same native session",
            ),
            supported(
                "observe",
                "owned runner process/stdio liveness and typed runner lifecycle events; no transcript mirroring",
            ),
            unsupported(
                "inspect_effect",
                "Agent SDK runner exposes no stable provider operation id for semantic effect inspection",
            ),
            unsupported(
                "reconcile_effect",
                "no stable provider operation id exists for exact effect reconciliation",
            ),
            unsupported(
                "inspect_continuation",
                "Claude /goal exists in Claude Code but is not wired through Agent SDK Team control",
            ),
            unsupported(
                "inhibit_continuation",
                "host-driven mode never activates Claude /goal; the adapter cannot control it",
            ),
            unsupported(
                "resume_continuation",
                "host-driven mode never activates Claude /goal; the adapter cannot control it",
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
                evidence: "strict Release depends on a verified Quiesce; use the separately authorized Team CloseRuntime operation to close the owned SDK handle while retaining the native session".into(),
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
                evidence: "permissionMode/allowedTools compile provider policy; bypassPermissions under trusted-development is explicitly not a sandbox".into(),
                security_enforcement_locus: Some("provider_native_policy_or_none_under_bypass".into()),
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
        "claude_project_session"
    }

    fn bind_authority_session(
        &mut self,
        session: AgentSession,
        profile: &harness_core::ProviderIntegrationProfile,
    ) -> CliResult<()> {
        if session.provider_kind != "claude"
            || profile.provider != "claude"
            || profile.execution_mode != "claude_agent_sdk"
        {
            return Err(CliError::Usage(format!(
                "RUNTIME_ADAPTER_PROVIDER_MISMATCH: Claude adapter cannot bind session={} profile={}:{}",
                session.provider_kind, profile.provider, profile.execution_mode
            )));
        }
        if profile.provider_version.as_deref() != Some(REVIEWED_CLAUDE_CODE_VERSION) {
            return Err(CliError::Usage(format!(
                "CLAUDE_AGENT_SDK_VERSION_UNREVIEWED: profile must bind exact Claude Code {}, got {:?}",
                REVIEWED_CLAUDE_CODE_VERSION, profile.provider_version
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
                    "CLAUDE_AGENT_SDK_RESUME_MISMATCH: authority={} runtime={}",
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
        timeouts: CycleTimeouts,
        on_input_accepted: &mut dyn FnMut(&ControlTransportReceipt) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(&SteerRequest, &SteerProviderResult) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> CycleControl,
    ) -> CliResult<ExecutionCycleOutcome> {
        self.transport.run_cycle(
            input,
            timeouts,
            on_input_accepted,
            on_steer_result,
            on_event,
            poll_control,
        )
    }

    fn native_control<'a>(
        close: &'a mut bool,
        interrupt: &'a mut bool,
    ) -> Box<dyn ProviderNativeControl + 'a> {
        Box::new(ClaudeControlFlags { close, interrupt })
    }
}

impl RuntimeAdapter for ClaudeTeamRuntime {
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
                        CycleTimeouts::with_input_acceptance(Duration::from_secs(30 * 60)),
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
                Ok(EffectReceipt::for_cycle(
                    request.effect_id,
                    admission.admission,
                    CycleSettlement::from_cycle_outcome(&outcome),
                )
                .with_native_evidence([
                    provider_receipt,
                    format!(
                        "claude.turn_complete:session={}",
                        self.transport.native_session_id
                    ),
                    format!(
                        "claude.terminal:settled={}",
                        outcome.terminal_observation.settled_boundary_observed
                    ),
                ]))
            }
            ControlIntent::Interrupt => {
                self.transport.interrupt().map_err(contract_bridge_error)?;
                Ok(EffectReceipt::for_control(
                    request.effect_id,
                    RuntimeEffectCertainty::Applied,
                    // The write is not terminal proof; run_cycle waits for
                    // interrupted + same-session resume before satisfying it.
                    RuntimePostconditionStatus::Unknown,
                    admission.admission,
                    vec!["claude.query.interrupt dispatched".into()],
                ))
            }
            _ => unreachable!("unsupported Claude control must fail canonical preflight"),
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
                    "claude.member_closed:reason={}",
                    evidence.acknowledged_reason.as_deref().unwrap_or("unknown")
                ),
                format!(
                    "claude.owned_process_group:pid={}:exited=true",
                    self.transport.child.id()
                ),
                format!(
                    "claude.native_session_retained:{}",
                    evidence.native_session_id.as_deref().unwrap_or("none_created")
                ),
                format!(
                    "claude.undelivered_input_count={:?} (observation only; not a strict queue-drain claim)",
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
        unreachable!("Claude inspect_effect is not admitted")
    }

    fn reconcile(
        &mut self,
        fence: RuntimeBindingFence,
        _inspection: &EffectInspection,
    ) -> Result<ReconcileReceipt, RuntimeContractError> {
        self.contract_preflight(fence, SemanticCapability::Reconcile)?;
        unreachable!("Claude reconcile is not admitted")
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
        let flush = find_and_sync_claude_session(&self.transport.native_session_id)
            .map(|_| RuntimePostconditionStatus::Satisfied)
            .unwrap_or(RuntimePostconditionStatus::Unknown);
        let mut builder = QuiesceReceiptBuilder::new();
        builder.record(
            QuiesceStep::FenceAdmission,
            RuntimePostconditionStatus::Satisfied,
            "exact RuntimeBindingFence admitted",
        )?;
        builder.record(
            QuiesceStep::InhibitContinuation,
            continuation,
            "Claude /goal is not activated by the host-driven adapter",
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
            "FullAccess Agent SDK exposes no complete writable-child/job inventory",
        )?;
        builder.record(
            QuiesceStep::ObserveIdle,
            terminal,
            format!("runner state={:?}", self.transport.state),
        )?;
        builder.record(
            QuiesceStep::ConfirmFlush,
            flush,
            "exact Claude project-session JSONL file+directory sync when discoverable",
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
        "Claude Agent SDK native bridge operation failed: {error}"
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

fn verify_runner_sdk_version(runner_path: &Path) -> CliResult<()> {
    let package = runner_path
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("package.json"))
        .ok_or_else(|| {
            CliError::Usage(format!(
                "cannot resolve Claude runner package from {}",
                runner_path.display()
            ))
        })?;
    let raw = fs::read_to_string(&package).map_err(|error| {
        CliError::Usage(format!(
            "failed to read Claude runner package {}: {error}",
            package.display()
        ))
    })?;
    let value: Value = serde_json::from_str(&raw)?;
    let observed = value
        .pointer("/dependencies/@anthropic-ai~1claude-agent-sdk")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "Claude runner package {} does not pin @anthropic-ai/claude-agent-sdk",
                package.display()
            ))
        })?;
    if observed != REVIEWED_CLAUDE_AGENT_SDK_VERSION {
        return Err(CliError::Usage(format!(
            "CLAUDE_AGENT_SDK_VERSION_UNREVIEWED: expected SDK {}, package pins {observed}",
            REVIEWED_CLAUDE_AGENT_SDK_VERSION
        )));
    }
    Ok(())
}

/// Best-effort exact provider-store flush evidence for strict Quiesce. Failure
/// is returned to the caller and becomes Unknown; it is never treated as
/// success merely because a JSONL path exists.
fn find_and_sync_claude_session(session_id: &str) -> Result<PathBuf, String> {
    if session_id.trim().is_empty() {
        return Err("native session id is empty".into());
    }
    let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
    find_and_sync_claude_session_under(&PathBuf::from(home).join(".claude/projects"), session_id)
}

fn find_and_sync_claude_session_under(root: &Path, session_id: &str) -> Result<PathBuf, String> {
    let expected = format!("{session_id}.jsonl");
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && entry.file_name().to_string_lossy() == expected {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|error| format!("cannot open {} for sync: {error}", path.display()))?;
                file.sync_all()
                    .map_err(|error| format!("cannot sync {}: {error}", path.display()))?;
                let parent = path.parent().ok_or("session file has no parent")?;
                File::open(parent)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|error| {
                        format!(
                            "cannot sync session directory {}: {error}",
                            parent.display()
                        )
                    })?;
                return Ok(path);
            }
        }
    }
    Err(format!(
        "Claude native session {session_id} was not found beneath {}",
        root.display()
    ))
}

#[cfg(test)]
#[path = "claude_team_runtime_tests.rs"]
mod tests;
