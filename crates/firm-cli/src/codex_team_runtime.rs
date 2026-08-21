//! Provider-neutral Codex app-server Team runtime binding.
//!
//! The binding owns only one process-local app-server handle. Durable
//! authority remains the exact `AgentSession` + `RuntimeCommand` fence, while
//! the provider-native thread remains the sole turn/tool/transcript truth.

use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use harness_core::agentfirm_api::{
    AgentSession, MemberExecutionDriver, NativeContinuationActivation, NativeContinuationBudget,
    NativeContinuationDefinition, NativeContinuationPhase, NativeContinuationProjection,
    RuntimeEffectCertainty, RuntimePostconditionStatus,
};
use harness_core::ProviderIntegrationProfile;
use serde_json::Value;

use crate::codex_app_server::{CodexAppServerClient, CodexAppServerShutdownReceipt};
use crate::provider_adapter::{
    NativeControlPrimitive, ProviderControlAction, ProviderControlPlan, ProviderNativeControl,
};
use crate::runtime_adapter::{
    CapabilityBinding, CapabilityStatus, ControlTransportReceipt, CycleControl,
    ExecutionCycleOutcome, RuntimeObservation as CycleRuntimeObservation, SteerProviderResult,
    SteerRequest, TeamRuntimeAdapter,
};
use crate::runtime_adapter_contract::{
    AdmissionDecision, ControlIntent, ControlRequest, EffectInspection, EffectReceipt,
    MemberRuntimeCloseReceipt, QuiesceReceipt, QuiesceReceiptBuilder, QuiesceStep,
    ReconcileReceipt, ReleaseReceipt, RuntimeContractError, RuntimeDescription, RuntimeFence,
    SemanticCapability,
};
use crate::{CliError, CliResult, ProviderTerminalFailure};

const INTERRUPT_SETTLE_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const REVIEWED_CODEX_APP_SERVER_VERSION: &str = "0.148.0-alpha.9";

/// Narrow bridge used by deterministic tests. It is process-local and never a
/// second persistence interface.
pub(crate) trait CodexAppServerBridge {
    fn ensure_transport_alive(&mut self) -> CliResult<()>;
    fn thread_id(&self) -> &str;
    fn start_turn(&mut self, text: &str) -> CliResult<String>;
    fn steer(&mut self, turn_id: &str, text: &str) -> CliResult<String>;
    fn interrupt(&mut self, turn_id: &str) -> CliResult<()>;
    fn recv(&self, timeout: Duration) -> Result<Value, RecvTimeoutError>;
    fn read_thread(&mut self, include_turns: bool) -> CliResult<Value>;
    fn read_thread_goal(&mut self) -> CliResult<Option<Value>>;
    fn set_thread_goal_status(&mut self, status: &str) -> CliResult<Value>;
    fn shutdown_with_receipt(&mut self) -> CliResult<CodexAppServerShutdownReceipt>;
}

impl CodexAppServerBridge for CodexAppServerClient {
    fn ensure_transport_alive(&mut self) -> CliResult<()> {
        Ok(CodexAppServerClient::ensure_transport_alive(self)?)
    }

    fn thread_id(&self) -> &str {
        CodexAppServerClient::thread_id(self)
    }

    fn start_turn(&mut self, text: &str) -> CliResult<String> {
        Ok(CodexAppServerClient::start_turn(self, text)?)
    }

    fn steer(&mut self, turn_id: &str, text: &str) -> CliResult<String> {
        Ok(CodexAppServerClient::steer(self, turn_id, text)?)
    }

    fn interrupt(&mut self, turn_id: &str) -> CliResult<()> {
        Ok(CodexAppServerClient::interrupt(self, turn_id)?)
    }

    fn recv(&self, timeout: Duration) -> Result<Value, RecvTimeoutError> {
        CodexAppServerClient::recv(self, timeout)
    }

    fn read_thread(&mut self, include_turns: bool) -> CliResult<Value> {
        Ok(CodexAppServerClient::read_thread(self, include_turns)?)
    }

    fn read_thread_goal(&mut self) -> CliResult<Option<Value>> {
        Ok(CodexAppServerClient::read_thread_goal(self)?)
    }

    fn set_thread_goal_status(&mut self, status: &str) -> CliResult<Value> {
        Ok(CodexAppServerClient::set_thread_goal_status(self, status)?)
    }

    fn shutdown_with_receipt(&mut self) -> CliResult<CodexAppServerShutdownReceipt> {
        Ok(CodexAppServerClient::shutdown_with_receipt(self)?)
    }
}

/// Server-initiated requests need the existing correlated provider-interaction
/// path. The caller injects that handler; absent a handler, the adapter fails
/// closed rather than fabricating a permission or user answer.
type ProviderRequestHandler<'a, B> = Box<dyn FnMut(&mut B, &Value) -> CliResult<()> + 'a>;

#[derive(Debug, Clone, PartialEq)]
struct CodexTerminalTurn {
    status: String,
    error: Option<Value>,
}

fn provider_terminal_failure(error: Option<&Value>) -> ProviderTerminalFailure {
    let info = error.and_then(|error| error.get("codexErrorInfo"));
    let (reason, http_status) = match info {
        Some(Value::String(reason)) if !reason.trim().is_empty() => (reason.clone(), None),
        Some(Value::Object(fields)) => {
            let variant = fields
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "other".to_string());
            let http_status = fields
                .get(&variant)
                .and_then(|details| details.get("httpStatusCode"))
                .and_then(Value::as_i64);
            (variant, http_status)
        }
        _ => (
            error
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .filter(|message| !message.trim().is_empty())
                .unwrap_or("turn_failed")
                .to_string(),
            None,
        ),
    };
    ProviderTerminalFailure {
        reason,
        http_status,
    }
}

pub(crate) struct CodexTeamRuntime<'a, B = CodexAppServerClient> {
    bridge: B,
    description: RuntimeDescription,
    authority_session: Option<AgentSession>,
    provider_request_handler: Option<ProviderRequestHandler<'a, B>>,
    active_turn_id: Option<String>,
    last_cycle_terminal: bool,
    last_control_acknowledged: bool,
    canonical_quiesced: bool,
    runtime_closed: bool,
}

impl<'a, B: CodexAppServerBridge> CodexTeamRuntime<'a, B> {
    pub(crate) fn new(bridge: B) -> Self {
        Self {
            bridge,
            description: RuntimeDescription {
                binding_id: format!("codex-app-server-{REVIEWED_CODEX_APP_SERVER_VERSION}"),
                native_protocol: "codex-app-server-json-rpc-v2".to_string(),
                composition_fingerprint: String::new(),
                capability_fingerprint: String::new(),
                capability_bindings: Vec::new(),
            },
            authority_session: None,
            provider_request_handler: None,
            active_turn_id: None,
            last_cycle_terminal: true,
            last_control_acknowledged: false,
            canonical_quiesced: false,
            runtime_closed: false,
        }
    }

    pub(crate) fn with_provider_request_handler(
        mut self,
        handler: impl FnMut(&mut B, &Value) -> CliResult<()> + 'a,
    ) -> Self {
        self.provider_request_handler = Some(Box::new(handler));
        self
    }

    #[cfg(test)]
    fn into_inner(self) -> B {
        self.bridge
    }

    fn authority(&self) -> Result<&AgentSession, RuntimeContractError> {
        self.authority_session
            .as_ref()
            .ok_or_else(|| RuntimeContractError::FenceMismatch {
                fields: vec!["authority_session".to_string()],
            })
    }

    fn preflight(
        &self,
        fence: RuntimeFence<'_>,
        capability: SemanticCapability,
    ) -> Result<AdmissionDecision, RuntimeContractError> {
        crate::runtime_adapter_contract::preflight_effect(
            &self.description,
            self.authority()?,
            fence,
            capability,
            &[],
        )
    }

    fn handle_provider_request(&mut self, frame: &Value) -> CliResult<()> {
        if frame.get("method").and_then(Value::as_str) == Some("item/tool/requestUserInput") {
            let params = frame.get("params").ok_or_else(|| {
                CliError::Usage(
                    "CODEX_PROVIDER_REQUEST_UNSAFE: requestUserInput omitted params; denied fail-closed"
                        .to_string(),
                )
            })?;
            if params.get("isBlocking").and_then(Value::as_bool) != Some(true) {
                return Err(CliError::Usage(
                    "CODEX_PROVIDER_REQUEST_UNSUPPORTED: non-blocking requestUserInput cannot use the durable blocking Message path; denied fail-closed"
                        .to_string(),
                ));
            }
            let questions = params
                .get("questions")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    CliError::Usage(
                        "CODEX_PROVIDER_REQUEST_UNSAFE: requestUserInput omitted questions; denied fail-closed"
                            .to_string(),
                    )
                })?;
            if questions
                .iter()
                .any(|question| question.get("isSecret").and_then(Value::as_bool) != Some(false))
            {
                return Err(CliError::Usage(
                    "CODEX_PROVIDER_REQUEST_UNSAFE: secret or unclassified requestUserInput cannot be persisted as a Team Message; denied fail-closed"
                        .to_string(),
                ));
            }
        }
        match self.provider_request_handler.as_mut() {
            Some(handler) => handler(&mut self.bridge, frame),
            None => Err(CliError::Usage(format!(
                "CODEX_PROVIDER_REQUEST_UNHANDLED: {} denied fail-closed",
                frame
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ))),
        }
    }

    fn exact_thread_is_idle(&mut self, include_turns: bool) -> CliResult<Value> {
        let thread = self.bridge.read_thread(include_turns)?;
        let status = thread.pointer("/status/type").and_then(Value::as_str);
        if status != Some("idle") {
            return Err(CliError::Usage(format!(
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: thread {} status is {}, not idle",
                self.bridge.thread_id(),
                status.unwrap_or("<missing>")
            )));
        }
        Ok(thread)
    }

    /// Resolve the one currently running native turn without relying on an
    /// adapter-local `turn/start`. Provider-driven Goal continuation can start
    /// a turn by itself, so Close/Interrupt must be able to fence that exact
    /// turn from `thread/read(includeTurns=true)`.
    fn active_native_turn(&mut self) -> CliResult<Option<String>> {
        let thread = self.bridge.read_thread(true)?;
        let status = thread.pointer("/status/type").and_then(Value::as_str);
        let turns = thread
            .get("turns")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                CliError::Usage(
                    "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: thread/read(includeTurns=true) omitted turns"
                        .to_string(),
                )
            })?;
        let active = turns
            .iter()
            .filter(|turn| turn.get("status").and_then(Value::as_str) == Some("inProgress"))
            .map(|turn| {
                turn.get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        CliError::Usage(
                            "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: in-progress turn omitted id"
                                .to_string(),
                        )
                    })
            })
            .collect::<CliResult<Vec<_>>>()?;
        match (status, active.as_slice()) {
            (Some("idle"), []) => Ok(None),
            (Some("active"), [turn_id]) => Ok(Some(turn_id.clone())),
            (Some("idle"), _) => Err(CliError::Usage(
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: thread is idle but retains an in-progress turn"
                    .to_string(),
            )),
            (Some("active"), []) => Err(CliError::Usage(
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: active thread has no in-progress turn"
                    .to_string(),
            )),
            (Some("active"), _) => Err(CliError::Usage(
                "CODEX_ONE_DRIVER_VIOLATION: thread/read reported multiple in-progress turns"
                    .to_string(),
            )),
            (Some(other), _) => Err(CliError::Usage(format!(
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: thread status is {other}"
            ))),
            (None, _) => Err(CliError::Usage(
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: thread/read omitted status".to_string(),
            )),
        }
    }

    fn inhibit_active_goal_for_terminal_control(&mut self) -> CliResult<()> {
        let Some(goal) = self.bridge.read_thread_goal()? else {
            return Ok(());
        };
        match goal.get("status").and_then(Value::as_str) {
            Some("active") => {
                self.bridge.set_thread_goal_status("paused")?;
                self.last_control_acknowledged = true;
                Ok(())
            }
            Some("paused" | "blocked" | "usageLimited" | "budgetLimited" | "complete") => Ok(()),
            Some(status) => Err(CliError::Usage(format!(
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: cannot inhibit native Goal status {status}"
            ))),
            None => Err(CliError::Usage(
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: native Goal omitted status".to_string(),
            )),
        }
    }

    fn observe_continuation(&mut self) -> CliResult<NativeContinuationProjection> {
        let durable = self
            .authority_session
            .as_ref()
            .map(|session| session.control_state.continuation.clone())
            .unwrap_or_default();
        let Some(goal) = self.bridge.read_thread_goal()? else {
            return Ok(NativeContinuationProjection {
                definition: NativeContinuationDefinition {
                    phase: NativeContinuationPhase::Inactive,
                    ..Default::default()
                },
                activation: NativeContinuationActivation::Disarmed,
                observed_at: Some(crate::now_string()),
            });
        };
        let phase = match goal.get("status").and_then(Value::as_str) {
            Some("active") => NativeContinuationPhase::Active,
            Some("paused") => NativeContinuationPhase::Paused,
            Some("blocked" | "usageLimited" | "budgetLimited") => NativeContinuationPhase::Blocked,
            Some("complete") => NativeContinuationPhase::Satisfied,
            _ => NativeContinuationPhase::Unknown,
        };
        let token_budget = goal.get("tokenBudget").and_then(Value::as_u64);
        let tokens_used = goal.get("tokensUsed").and_then(Value::as_u64);
        Ok(NativeContinuationProjection {
            definition: NativeContinuationDefinition {
                continuation_ref: Some(format!("codex-goal:{}", self.bridge.thread_id())),
                revision: goal.get("updatedAt").and_then(Value::as_u64),
                phase,
                budget: Some(NativeContinuationBudget {
                    remaining_cycles: None,
                    remaining_tokens: token_budget
                        .zip(tokens_used)
                        .map(|(budget, used)| budget.saturating_sub(used)),
                    deadline: None,
                    provider_budget_ref: Some(format!(
                        "codex-goal-budget:{}",
                        self.bridge.thread_id()
                    )),
                }),
            },
            // Observation never grants execution authority. Only a durable
            // driver handoff may arm the provider continuation.
            activation: durable.activation,
            observed_at: Some(crate::now_string()),
        })
    }

    fn require_exact_frame_thread(&self, frame: &Value, method: &str) -> CliResult<()> {
        let observed_thread = frame
            .pointer("/params/threadId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: {method} omitted threadId"
                ))
            })?;
        if observed_thread != self.bridge.thread_id() {
            return Err(CliError::Usage(format!(
                "CODEX_ONE_DRIVER_VIOLATION: {method} for thread {observed_thread} arrived on owned thread {}",
                self.bridge.thread_id()
            )));
        }
        Ok(())
    }

    fn terminal_frame_for_active_turn(
        &self,
        frame: &Value,
    ) -> CliResult<Option<CodexTerminalTurn>> {
        if frame.get("method").and_then(Value::as_str) != Some("turn/completed") {
            return Ok(None);
        }
        self.require_exact_frame_thread(frame, "turn/completed")?;
        let params = frame.get("params").unwrap_or(frame);
        let observed_turn = params
            .pointer("/turn/id")
            .or_else(|| params.get("turnId"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CliError::Usage(
                    "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: turn/completed omitted turn id"
                        .to_string(),
                )
            })?;
        let expected = self.active_turn_id.as_deref().ok_or_else(|| {
            CliError::Usage(
                "CODEX_ONE_DRIVER_VIOLATION: terminal frame arrived without an admitted active turn"
                    .to_string(),
            )
        })?;
        if observed_turn != expected {
            return Err(CliError::Usage(format!(
                "CODEX_ONE_DRIVER_VIOLATION: active turn {expected} received terminal frame for {observed_turn}"
            )));
        }
        let status = params
            .pointer("/turn/status")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CliError::Usage(
                    "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: turn/completed omitted status"
                        .to_string(),
                )
            })?;
        Ok(Some(CodexTerminalTurn {
            status: status.to_string(),
            error: params
                .pointer("/turn/error")
                .filter(|error| !error.is_null())
                .cloned(),
        }))
    }

    fn settle_active_turn_for_close(&mut self) -> CliResult<()> {
        // Pause continuation before inspecting or interrupting the turn. If a
        // Goal stays active it can race the terminal observation by starting a
        // successor turn, violating both Close and strong handoff semantics.
        self.inhibit_active_goal_for_terminal_control()?;
        let turn_id = match self.active_turn_id.clone() {
            Some(turn_id) => Some(turn_id),
            None => self.active_native_turn()?,
        };
        let Some(turn_id) = turn_id else {
            self.last_cycle_terminal = true;
            self.last_control_acknowledged = true;
            return Ok(());
        };
        self.active_turn_id = Some(turn_id.clone());
        self.bridge.interrupt(&turn_id)?;
        self.last_control_acknowledged = true;
        let deadline = Instant::now() + INTERRUPT_SETTLE_TIMEOUT;
        while Instant::now() < deadline {
            match self.bridge.recv(Duration::from_millis(50)) {
                Ok(frame) => {
                    if frame.get("id").is_some() && frame.get("method").is_some() {
                        self.handle_provider_request(&frame)?;
                        continue;
                    }
                    if let Some(terminal) = self.terminal_frame_for_active_turn(&frame)? {
                        if !matches!(
                            terminal.status.as_str(),
                            "interrupted" | "completed" | "failed"
                        ) {
                            return Err(CliError::Usage(format!(
                                "CODEX_RUNTIME_CLOSE_UNKNOWN: active turn ended as {}",
                                terminal.status
                            )));
                        }
                        self.active_turn_id = None;
                        self.exact_thread_is_idle(false)?;
                        self.last_cycle_terminal = true;
                        return Ok(());
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(CliError::Usage(
                        "CODEX_RUNTIME_CLOSE_UNKNOWN: transport disconnected before turn/completed"
                            .to_string(),
                    ))
                }
            }
        }
        Err(CliError::Usage(
            "CODEX_RUNTIME_CLOSE_UNKNOWN: interrupt was acknowledged but terminal turn evidence timed out"
                .to_string(),
        ))
    }
}

pub(crate) fn capability_bindings() -> Vec<CapabilityBinding> {
    use CapabilityStatus::{Degraded, Experimental, Supported, Unsupported};
    vec![
        CapabilityBinding {
            capability: "open_or_resume",
            status: Supported,
            evidence: "Codex 0.148.0-alpha.9 app-server initialize + thread/start|resume; exact returned thread id and effective permission controls are retained as native truth".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "start_cycle",
            status: Supported,
            evidence: "turn/start returns the exact turn id; turn/completed plus thread/read status=idle proves the later cycle boundary".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "inject_current_cycle",
            status: Experimental,
            evidence: "0.148.0-alpha.9 schema review and deterministic tests cover turn/steer with expectedTurnId, but the DEV-26 live upgrade canary did not exercise steer".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "queue_at_native_boundary",
            status: Unsupported,
            evidence: "Codex app-server exposes current-turn steer, not a provider-native ordinary-message queue; Harness retains next-round mail".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "interrupt_current_cycle",
            status: Supported,
            evidence: "turn/interrupt response plus matching turn/completed and thread/read status=idle".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "observe",
            status: Supported,
            evidence: "non-invasive thread/read and thread/goal/get plus owned transport liveness".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "inspect_continuation",
            status: Experimental,
            evidence: "Codex 0.148.0-alpha.9 schema review and deterministic tests cover thread/goal/get; live ProviderDriven supervision remains unproven".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "inhibit_continuation",
            status: Experimental,
            evidence: "thread/goal/set status=paused has an exact deterministic receipt, but the DEV-26 live canary did not activate a native Goal".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "resume_continuation",
            status: Experimental,
            evidence: "thread/goal/set status=active is fenced by ProviderDriven authority in deterministic tests; live autonomous continuation supervision remains unproven".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "close_runtime",
            status: Supported,
            evidence: "terminal active-turn observation followed by one-shot owned process-group release/reap; thread id is retained for Reopen".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "quiesce",
            status: Degraded,
            evidence: "thread/goal and thread/read prove continuation/cycle idle, but FullAccess detached writable children and durable rollout flush are not fully observable".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "release",
            status: Degraded,
            evidence: "strong release remains gated by verified quiesce; Team Close uses the narrower close_runtime receipt".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "inspect_effect",
            status: Unsupported,
            evidence: "Codex exposes native turns but no durable RuntimeCommand effect-id lookup".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "reconcile_effect",
            status: Unsupported,
            evidence: "no provider operation id can reconcile an unknown Harness effect after transport loss".into(),
            security_enforcement_locus: None,
        },
        CapabilityBinding {
            capability: "permission_enforcement",
            status: Supported,
            evidence: "thread/start|resume sandbox and approvalPolicy are compiled from the frozen AgentSession ceiling".into(),
            security_enforcement_locus: Some("provider_native_policy: Codex sandbox + approvalPolicy".into()),
        },
    ]
}

struct CodexDeferredNativeControl<'a> {
    close: &'a mut bool,
    interrupt: &'a mut bool,
}

impl ProviderNativeControl for CodexDeferredNativeControl<'_> {
    fn provider(&self) -> &'static str {
        "codex"
    }

    fn dispatch(&mut self, plan: &ProviderControlPlan) -> Result<(), String> {
        if plan.primitive != NativeControlPrimitive::CodexTurnInterrupt {
            return Err(format!(
                "PROVIDER_CONTROL_UNPROVEN: Codex adapter received {:?}",
                plan.primitive
            ));
        }
        *self.interrupt = true;
        *self.close = plan.action == ProviderControlAction::CloseSession;
        Ok(())
    }
}

impl<'a, B: CodexAppServerBridge> TeamRuntimeAdapter for CodexTeamRuntime<'a, B> {
    fn provider(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn capability_bindings() -> Vec<CapabilityBinding> {
        capability_bindings()
    }

    fn ensure_alive(&mut self) -> CliResult<()> {
        if self.runtime_closed {
            return Err(CliError::Usage(
                "codex app-server runtime was explicitly closed".to_string(),
            ));
        }
        self.bridge.ensure_transport_alive()
    }

    fn native_session_locator(&self) -> &str {
        self.bridge.thread_id()
    }

    fn native_locator_kind(&self) -> &'static str {
        "codex_rollout"
    }

    fn bind_authority_session(
        &mut self,
        session: AgentSession,
        profile: &ProviderIntegrationProfile,
    ) -> CliResult<()> {
        if session.provider_kind != "codex"
            || profile.provider != "codex"
            || profile.execution_mode != "codex_app_server"
        {
            return Err(CliError::Usage(format!(
                "RUNTIME_ADAPTER_PROVIDER_MISMATCH: Codex adapter cannot bind session={} profile={}/{}",
                session.provider_kind, profile.provider, profile.execution_mode
            )));
        }
        if let Some(native) = session.native_session_ref.as_ref() {
            if native.native_session_id != self.bridge.thread_id() {
                return Err(CliError::Usage(format!(
                    "RUNTIME_ADAPTER_FENCE_INCOMPLETE: Codex thread {} does not match AgentSession {}",
                    self.bridge.thread_id(), native.native_session_id
                )));
            }
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
                    "RUNTIME_ADAPTER_FENCE_INCOMPLETE: Codex profile/session composition fingerprint mismatch"
                        .to_string(),
                )
            })?;
        let capabilities = profile
            .capability_fingerprint
            .clone()
            .filter(|value| {
                session.control_state.capability_fingerprint.as_deref()
                    == Some(value.as_str())
            })
            .ok_or_else(|| {
                CliError::Usage(
                    "RUNTIME_ADAPTER_FENCE_INCOMPLETE: Codex profile/session capability fingerprint mismatch"
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
        on_input_accepted: &mut dyn FnMut(&ControlTransportReceipt) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(&SteerRequest, &SteerProviderResult) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> CycleControl,
    ) -> CliResult<ExecutionCycleOutcome> {
        if self.runtime_closed {
            return Err(CliError::Usage(
                "codex app-server runtime was explicitly closed".to_string(),
            ));
        }
        if self.active_turn_id.is_some() {
            return Err(CliError::Usage(
                "CODEX_ONE_DRIVER_VIOLATION: start_cycle called while another turn is active"
                    .to_string(),
            ));
        }
        if let Some(session) = self.authority_session.as_ref() {
            if session.control_state.execution_driver != MemberExecutionDriver::HostDriven {
                return Err(CliError::Usage(
                    "CODEX_ONE_DRIVER_VIOLATION: Harness start_cycle requires HostDriven authority"
                        .to_string(),
                ));
            }
            if matches!(
                session.control_state.continuation.activation,
                NativeContinuationActivation::Armed { .. }
            ) {
                return Err(CliError::Usage(
                    "CODEX_ONE_DRIVER_VIOLATION: HostDriven start rejected while native Goal continuation is armed"
                        .to_string(),
                ));
            }
            if let Some(goal) = self.bridge.read_thread_goal()? {
                match goal.get("status").and_then(Value::as_str) {
                    Some("active") => {
                        return Err(CliError::Usage(
                            "CODEX_ONE_DRIVER_VIOLATION: native Goal is active while Harness owns HostDriven scheduling"
                                .to_string(),
                        ))
                    }
                    Some(
                        "paused"
                        | "blocked"
                        | "usageLimited"
                        | "budgetLimited"
                        | "complete",
                    ) => {}
                    Some(status) => {
                        return Err(CliError::Usage(format!(
                            "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: HostDriven start cannot classify native Goal status {status}"
                        )))
                    }
                    None => {
                        return Err(CliError::Usage(
                            "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: native Goal omitted status before HostDriven start"
                                .to_string(),
                        ))
                    }
                }
            }
        }
        let turn_id = self.bridge.start_turn(input)?;
        self.active_turn_id = Some(turn_id.clone());
        self.last_cycle_terminal = false;
        self.last_control_acknowledged = false;
        self.canonical_quiesced = false;
        let input_receipt = ControlTransportReceipt {
            command: "turn/start".to_string(),
            response_id: Some(turn_id.clone()),
            success: true,
        };
        on_input_accepted(&input_receipt)?;

        let mut final_text = String::new();
        let mut tool_call_count = 0u32;
        let mut interrupted = false;
        let mut close_requested = false;
        let mut interrupt_sent = false;
        let mut control_receipts = Vec::new();
        let mut last_activity = Instant::now();
        let mut interrupt_deadline = None;

        loop {
            let control = poll_control();
            if let Some(error) = control.fatal_error {
                return Err(CliError::Usage(error));
            }
            for pending in control.injects {
                match self.bridge.steer(&turn_id, &pending.content) {
                    Ok(active_turn) if active_turn == turn_id => {
                        let receipt = ControlTransportReceipt {
                            command: "turn/steer".to_string(),
                            response_id: Some(active_turn),
                            success: true,
                        };
                        on_steer_result(
                            &pending,
                            &SteerProviderResult::Acknowledged(receipt.clone()),
                        )?;
                        control_receipts.push(receipt);
                    }
                    Ok(other_turn) => {
                        let detail = format!(
                            "CODEX_ONE_DRIVER_VIOLATION: turn/steer rebound {turn_id} to {other_turn}"
                        );
                        on_steer_result(&pending, &SteerProviderResult::Unknown(detail.clone()))?;
                        return Err(CliError::Usage(detail));
                    }
                    Err(error) => {
                        let detail = format!(
                            "Codex turn/steer outcome is unknown after RPC failure: {error}"
                        );
                        on_steer_result(&pending, &SteerProviderResult::Unknown(detail.clone()))?;
                        return Err(CliError::Usage(detail));
                    }
                }
            }
            if (control.interrupt || control.close) && !interrupt_sent {
                self.bridge.interrupt(&turn_id)?;
                interrupt_sent = true;
                interrupted = true;
                close_requested = control.close;
                self.last_control_acknowledged = true;
                interrupt_deadline = Some(Instant::now() + INTERRUPT_SETTLE_TIMEOUT);
                control_receipts.push(ControlTransportReceipt {
                    // The shared loop currently recognizes this provider-neutral
                    // terminal-control label. Native evidence still names
                    // turn/interrupt in capability and durable receipts.
                    command: "abort".to_string(),
                    response_id: Some(turn_id.clone()),
                    success: true,
                });
            } else if control.close && interrupt_sent {
                // Close may be requested after an earlier Interrupt already
                // crossed the native boundary. Reuse that exact terminal
                // cycle acknowledgement, then let close_runtime dispose the
                // handle; never send a second turn/interrupt.
                interrupted = true;
                close_requested = true;
            }

            match self.bridge.recv(Duration::from_millis(50)) {
                Ok(frame) => {
                    last_activity = Instant::now();
                    if frame.get("id").is_some() && frame.get("method").is_some() {
                        self.handle_provider_request(&frame)?;
                        continue;
                    }
                    let method = frame.get("method").and_then(Value::as_str);
                    let params = frame.get("params").unwrap_or(&frame);
                    let frame_turn_id = params
                        .get("turnId")
                        .or_else(|| params.pointer("/turn/id"))
                        .and_then(Value::as_str);
                    if method == Some("turn/started") {
                        self.require_exact_frame_thread(&frame, "turn/started")?;
                        let observed = frame_turn_id.ok_or_else(|| {
                            CliError::Usage(
                                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: turn/started omitted turn id"
                                    .to_string(),
                            )
                        })?;
                        if observed != turn_id {
                            return Err(CliError::Usage(format!(
                                "CODEX_ONE_DRIVER_VIOLATION: admitted turn {turn_id} observed concurrent turn {observed}"
                            )));
                        }
                    }
                    if method != Some("turn/completed")
                        && frame_turn_id.is_some_and(|observed| observed != turn_id)
                    {
                        // Delayed item activity from an earlier interrupted
                        // turn is not evidence for this cycle.
                        continue;
                    }
                    match method {
                        Some("item/agentMessage/delta") => {
                            if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                                final_text.push_str(delta);
                            }
                            on_event(&frame);
                        }
                        Some("item/started") => {
                            let item_type = params
                                .pointer("/item/type")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            if !matches!(
                                item_type,
                                "" | "agentMessage" | "agent_message" | "reasoning" | "plan"
                            ) {
                                tool_call_count = tool_call_count.saturating_add(1);
                            }
                            on_event(&frame);
                        }
                        Some("item/completed" | "item/reasoning/summaryTextDelta") => {
                            on_event(&frame)
                        }
                        Some("turn/completed") => {
                            let terminal = self
                                .terminal_frame_for_active_turn(&frame)?
                                .expect("matched terminal method");
                            if final_text.trim().is_empty() {
                                final_text = params
                                    .pointer("/turn/items")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter(|item| {
                                        matches!(
                                            item.get("type").and_then(Value::as_str),
                                            Some("agentMessage" | "agent_message")
                                        )
                                    })
                                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                            }
                            if !matches!(
                                terminal.status.as_str(),
                                "completed" | "interrupted" | "failed"
                            ) {
                                return Err(CliError::Usage(format!(
                                    "codex app-server turn {turn_id} ended as {}",
                                    terminal.status
                                )));
                            }
                            interrupted |= terminal.status == "interrupted";
                            self.active_turn_id = None;
                            self.exact_thread_is_idle(false)?;
                            self.last_cycle_terminal = true;
                            let provider_terminal_failure = (terminal.status == "failed")
                                .then(|| provider_terminal_failure(terminal.error.as_ref()));
                            return Ok(ExecutionCycleOutcome {
                                final_text,
                                provider_terminal_failure,
                                interrupted,
                                close_requested_by_harness: close_requested,
                                tool_call_count,
                                input_acceptance_receipt: input_receipt,
                                control_receipts,
                                terminal_observation: CycleRuntimeObservation {
                                    transport_alive: true,
                                    process_alive: true,
                                    is_streaming: Some(false),
                                    pending_message_count: Some(0),
                                    steering_mode: Some("turn/steer".to_string()),
                                    follow_up_mode: Some("harness_next_round".to_string()),
                                    settled_boundary_observed: true,
                                },
                            });
                        }
                        _ => {}
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(deadline) = interrupt_deadline {
                        if Instant::now() >= deadline {
                            return Err(CliError::Usage(
                                "CODEX_RUNTIME_CONTROL_UNKNOWN: turn/interrupt was acknowledged but turn/completed was not observed"
                                    .to_string(),
                            ));
                        }
                    } else if last_activity.elapsed() >= idle_timeout {
                        self.bridge.interrupt(&turn_id)?;
                        interrupt_sent = true;
                        interrupted = true;
                        self.last_control_acknowledged = true;
                        interrupt_deadline = Some(Instant::now() + INTERRUPT_SETTLE_TIMEOUT);
                        control_receipts.push(ControlTransportReceipt {
                            command: "abort".to_string(),
                            response_id: Some(turn_id.clone()),
                            success: true,
                        });
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(CliError::Usage(
                        "codex app-server transport disconnected before turn/completed".to_string(),
                    ))
                }
            }
        }
    }

    fn project_live(
        event: &Value,
    ) -> Option<(crate::provider_event_api::LiveProviderActivityKind, String)> {
        use crate::provider_event_api::LiveProviderActivityKind;
        let method = event.get("method").and_then(Value::as_str)?;
        let params = event.get("params").unwrap_or(event);
        match method {
            "item/agentMessage/delta" => Some((
                LiveProviderActivityKind::ResponseStreaming,
                "assistant response streaming".to_string(),
            )),
            "item/reasoning/summaryTextDelta" => params
                .get("delta")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .map(|text| {
                    (
                        LiveProviderActivityKind::Thinking,
                        text.chars().take(240).collect(),
                    )
                }),
            "item/started" => Some((
                LiveProviderActivityKind::ToolStarted,
                "Codex tool started".to_string(),
            )),
            "item/completed" => Some((
                LiveProviderActivityKind::ToolCompleted,
                "Codex tool completed".to_string(),
            )),
            _ => None,
        }
    }

    fn native_control<'b>(
        close: &'b mut bool,
        interrupt: &'b mut bool,
    ) -> Box<dyn ProviderNativeControl + 'b> {
        Box::new(CodexDeferredNativeControl { close, interrupt })
    }

    fn supports_inject_current_cycle(&self) -> bool {
        true
    }
}

fn bridge_error(error: impl std::fmt::Display) -> RuntimeContractError {
    RuntimeContractError::InvalidCapabilityBindings(format!(
        "Codex native bridge operation failed: {error}"
    ))
}

impl<'a, B: CodexAppServerBridge> crate::runtime_adapter_contract::RuntimeAdapter
    for CodexTeamRuntime<'a, B>
{
    fn describe(&self) -> &RuntimeDescription {
        &self.description
    }

    fn open_or_resume(
        &mut self,
        fence: RuntimeFence<'_>,
        native_session_ref: Option<&str>,
    ) -> Result<crate::runtime_adapter_contract::RuntimeObservation, RuntimeContractError> {
        self.preflight(fence, SemanticCapability::OpenOrResume)?;
        self.bridge.ensure_transport_alive().map_err(bridge_error)?;
        if native_session_ref.is_some_and(|expected| expected != self.bridge.thread_id()) {
            return Err(RuntimeContractError::FenceMismatch {
                fields: vec!["native_session_ref.native_session_id".to_string()],
            });
        }
        self.exact_thread_is_idle(false).map_err(bridge_error)?;
        let continuation = self.observe_continuation().map_err(bridge_error)?;
        Ok(crate::runtime_adapter_contract::RuntimeObservation {
            native_session_ref: Some(self.bridge.thread_id().to_string()),
            active_effect_id: None,
            continuation,
            observed_at: crate::now_string(),
        })
    }

    fn execute_control(
        &mut self,
        fence: RuntimeFence<'_>,
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
        let admission = self.preflight(fence, capability)?;
        self.canonical_quiesced = false;
        let (certainty, postcondition, evidence) = match request.intent {
            ControlIntent::StartCycle { input } => {
                let mut accepted = None;
                let outcome = TeamRuntimeAdapter::run_cycle(
                    self,
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
                .map_err(bridge_error)?;
                let turn_id = accepted
                    .ok_or_else(|| bridge_error("turn/start succeeded without an exact turn id"))?;
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Satisfied,
                    vec![
                        format!("codex.turn/start:{turn_id}"),
                        format!(
                            "codex.turn/completed:settled={}",
                            outcome.terminal_observation.settled_boundary_observed
                        ),
                    ],
                )
            }
            ControlIntent::InjectCurrentCycle { input } => {
                let turn_id = match self.active_turn_id.clone() {
                    Some(turn_id) => Some(turn_id),
                    None => self.active_native_turn().map_err(bridge_error)?,
                }
                .ok_or_else(|| bridge_error("turn/steer requires an exact active turn"))?;
                self.active_turn_id = Some(turn_id.clone());
                let observed = self.bridge.steer(&turn_id, &input).map_err(bridge_error)?;
                if observed != turn_id {
                    return Err(RuntimeContractError::StaleContinuation {
                        fields: vec!["active_turn_id".to_string()],
                    });
                }
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Satisfied,
                    vec![format!("codex.turn/steer:{turn_id}")],
                )
            }
            ControlIntent::Interrupt => {
                let turn_id = match self.active_turn_id.clone() {
                    Some(turn_id) => Some(turn_id),
                    None => self.active_native_turn().map_err(bridge_error)?,
                }
                .ok_or_else(|| bridge_error("turn/interrupt requires an exact active turn"))?;
                self.active_turn_id = Some(turn_id.clone());
                self.bridge.interrupt(&turn_id).map_err(bridge_error)?;
                self.last_control_acknowledged = true;
                (
                    RuntimeEffectCertainty::Applied,
                    // The RPC response proves transport acceptance only.
                    RuntimePostconditionStatus::Unknown,
                    vec![format!("codex.turn/interrupt:{turn_id}")],
                )
            }
            ControlIntent::InhibitContinuation { expected } => {
                if self.authority()?.control_state.continuation != expected {
                    return Err(RuntimeContractError::StaleContinuation {
                        fields: vec!["continuation".to_string()],
                    });
                }
                let goal = self
                    .bridge
                    .set_thread_goal_status("paused")
                    .map_err(bridge_error)?;
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Satisfied,
                    vec![format!(
                        "codex.thread/goal/set:{}:paused:revision={}",
                        self.bridge.thread_id(),
                        goal.get("updatedAt")
                            .and_then(Value::as_u64)
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    )],
                )
            }
            ControlIntent::ResumeContinuation { expected } => {
                let session = self.authority()?;
                if session.control_state.continuation != expected
                    || session.control_state.execution_driver
                        != MemberExecutionDriver::ProviderDriven
                    || !matches!(
                        session.control_state.continuation.activation,
                        NativeContinuationActivation::Armed {
                            runtime_generation,
                            driver_generation,
                        } if runtime_generation == session.runtime_generation
                            && driver_generation == session.control_state.driver_generation
                    )
                {
                    return Err(RuntimeContractError::StaleContinuation {
                        fields: vec!["provider_driven_continuation_authority".to_string()],
                    });
                }
                let goal = self
                    .bridge
                    .set_thread_goal_status("active")
                    .map_err(bridge_error)?;
                (
                    RuntimeEffectCertainty::Applied,
                    RuntimePostconditionStatus::Satisfied,
                    vec![format!(
                        "codex.thread/goal/set:{}:active:revision={}",
                        self.bridge.thread_id(),
                        goal.get("updatedAt")
                            .and_then(Value::as_u64)
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    )],
                )
            }
            ControlIntent::QueueNativeBoundary { .. } => {
                unreachable!("unsupported Codex queue must fail canonical preflight")
            }
        };
        Ok(EffectReceipt {
            effect_id: request.effect_id,
            certainty,
            postcondition,
            admission: admission.admission,
            native_evidence: evidence,
        })
    }

    fn observe(
        &mut self,
        fence: RuntimeFence<'_>,
    ) -> Result<crate::runtime_adapter_contract::RuntimeObservation, RuntimeContractError> {
        self.preflight(fence, SemanticCapability::Observe)?;
        self.bridge.ensure_transport_alive().map_err(bridge_error)?;
        let active_effect_id = self.active_native_turn().map_err(bridge_error)?;
        self.active_turn_id = active_effect_id.clone();
        let continuation = self.observe_continuation().map_err(bridge_error)?;
        Ok(crate::runtime_adapter_contract::RuntimeObservation {
            native_session_ref: Some(self.bridge.thread_id().to_string()),
            active_effect_id,
            continuation,
            observed_at: crate::now_string(),
        })
    }

    fn inspect_effect(
        &mut self,
        fence: RuntimeFence<'_>,
        _effect_id: &str,
    ) -> Result<EffectInspection, RuntimeContractError> {
        self.preflight(fence, SemanticCapability::InspectEffect)?;
        unreachable!("unsupported Codex effect inspection must fail canonical preflight")
    }

    fn reconcile(
        &mut self,
        fence: RuntimeFence<'_>,
        _inspection: &EffectInspection,
    ) -> Result<ReconcileReceipt, RuntimeContractError> {
        self.preflight(fence, SemanticCapability::Reconcile)?;
        unreachable!("unsupported Codex reconciliation must fail canonical preflight")
    }

    fn close_runtime(
        &mut self,
        fence: RuntimeFence<'_>,
    ) -> Result<MemberRuntimeCloseReceipt, RuntimeContractError> {
        if self.runtime_closed {
            return Err(RuntimeContractError::AlreadyReleased);
        }
        self.preflight(fence, SemanticCapability::CloseRuntime)?;
        self.settle_active_turn_for_close().map_err(bridge_error)?;
        let retained_thread = self.bridge.thread_id().to_string();
        if retained_thread.trim().is_empty() {
            return Err(RuntimeContractError::MemberCloseIncomplete {
                fields: vec!["native_session_retained=Unknown".to_string()],
            });
        }
        let native = self.bridge.shutdown_with_receipt().map_err(bridge_error)?;
        let satisfied = RuntimePostconditionStatus::Satisfied;
        let unknown = RuntimePostconditionStatus::Unknown;
        let receipt = MemberRuntimeCloseReceipt {
            control_acknowledged: if self.last_control_acknowledged {
                satisfied
            } else {
                unknown
            },
            current_cycle_terminal: if self.last_cycle_terminal {
                satisfied
            } else {
                unknown
            },
            managed_runtime_released: if native.process_reaped {
                satisfied
            } else {
                unknown
            },
            live_handle_disposed: if native.process_reaped && native.stdout_reader_joined {
                satisfied
            } else {
                unknown
            },
            native_session_retained: if native.thread_id_retained
                && retained_thread == self.bridge.thread_id()
            {
                satisfied
            } else {
                unknown
            },
            evidence: vec![
                format!("codex.thread:{retained_thread}:idle"),
                format!(
                    "codex.app-server:process_was_running={}:reaped={}:reader_joined={}:exit={}",
                    native.process_was_running,
                    native.process_reaped,
                    native.stdout_reader_joined,
                    native.exit_status
                ),
            ],
        };
        receipt.verify()?;
        self.runtime_closed = true;
        self.authority_session = None;
        Ok(receipt)
    }

    fn quiesce(&mut self, fence: RuntimeFence<'_>) -> Result<QuiesceReceipt, RuntimeContractError> {
        self.preflight(fence, SemanticCapability::Quiesce)?;
        self.settle_active_turn_for_close().map_err(bridge_error)?;
        let goal = self.bridge.read_thread_goal().map_err(bridge_error)?;
        let continuation = match goal
            .as_ref()
            .and_then(|goal| goal.get("status"))
            .and_then(Value::as_str)
        {
            None | Some("paused" | "blocked" | "usageLimited" | "budgetLimited" | "complete") => {
                RuntimePostconditionStatus::Satisfied
            }
            Some("active") => {
                self.bridge
                    .set_thread_goal_status("paused")
                    .map_err(bridge_error)?;
                RuntimePostconditionStatus::Satisfied
            }
            _ => RuntimePostconditionStatus::Unknown,
        };
        self.exact_thread_is_idle(true).map_err(bridge_error)?;
        let mut builder = QuiesceReceiptBuilder::new();
        builder.record(
            QuiesceStep::FenceAdmission,
            RuntimePostconditionStatus::Satisfied,
            "exact RuntimeFence admitted",
        )?;
        builder.record(
            QuiesceStep::InhibitContinuation,
            continuation,
            "thread/goal/get|set native receipt",
        )?;
        builder.record(
            QuiesceStep::SettleActiveCycle,
            RuntimePostconditionStatus::Satisfied,
            "matching turn/completed and thread/read status=idle",
        )?;
        builder.record(QuiesceStep::DrainNativeQueue, RuntimePostconditionStatus::Satisfied, "app-server has no provider-native ordinary-message queue; turn/steer is active-turn only")?;
        builder.record(QuiesceStep::DrainWritableChildren, RuntimePostconditionStatus::Unknown, "Codex FullAccess may leave detached writable descendants; app-server exposes no complete job inventory")?;
        builder.record(
            QuiesceStep::ObserveIdle,
            RuntimePostconditionStatus::Satisfied,
            "thread/read status=idle",
        )?;
        builder.record(
            QuiesceStep::ConfirmFlush,
            RuntimePostconditionStatus::Unknown,
            "thread/read proves readable native state but is not a durable rollout flush receipt",
        )?;
        let receipt = builder.finish();
        receipt.verify()?;
        self.canonical_quiesced = true;
        Ok(receipt)
    }

    fn release(&mut self, fence: RuntimeFence<'_>) -> Result<ReleaseReceipt, RuntimeContractError> {
        if self.runtime_closed {
            return Err(RuntimeContractError::AlreadyReleased);
        }
        self.preflight(fence, SemanticCapability::Release)?;
        if !self.canonical_quiesced {
            return Err(RuntimeContractError::CompositionSwapRequiresQuiesce);
        }
        let native = self.bridge.shutdown_with_receipt().map_err(bridge_error)?;
        let satisfied = RuntimePostconditionStatus::Satisfied;
        let unknown = RuntimePostconditionStatus::Unknown;
        let receipt = ReleaseReceipt {
            native_runtime_released: if native.process_reaped {
                satisfied
            } else {
                unknown
            },
            live_handle_disposed: if native.process_reaped && native.stdout_reader_joined {
                satisfied
            } else {
                unknown
            },
            authority_detached: satisfied,
            flush_confirmed: satisfied,
            evidence: vec![format!(
                "codex strong release after verified quiesce: exit={}",
                native.exit_status
            )],
        };
        receipt.verify()?;
        self.runtime_closed = true;
        self.authority_session = None;
        Ok(receipt)
    }
}

#[cfg(test)]
#[path = "codex_team_runtime_tests.rs"]
mod tests;
