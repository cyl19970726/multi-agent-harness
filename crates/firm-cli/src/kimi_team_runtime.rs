//! Provider-neutral Team runtime binding for Kimi Code ACP 0.36.1.
//!
//! The durable AgentSession and NativeSessionRef remain the authority. This
//! module owns only the process-local ACP handle and compiles neutral runtime
//! intents into the exact reviewed wire operations:
//!
//! - open/resume is proved by `initialize` plus `session/new|resume` in
//!   [`KimiAcpClient`](crate::kimi_acp::KimiAcpClient);
//! - one ExecutionCycle is one `session/prompt` and its correlated terminal
//!   response;
//! - Interrupt is `session/cancel` followed by terminal
//!   `stopReason=cancelled`;
//! - reversible Team Close is `session/close`, client-stdio close, clean child
//!   exit and reap, while retaining the native session id for Reopen;
//! - strict Quiesce/Release remain fail-closed because ACP exposes no complete
//!   native queue, writable-child, or durable-flush proof.

use std::time::Duration;

use harness_core::agentfirm_api::{
    AgentSession, NativeContinuationActivation, RuntimeEffectCertainty, RuntimePostconditionStatus,
};
use serde_json::Value;

use crate::kimi_acp::{KimiAcpClient, PromptControl};
use crate::{CliError, CliResult};

type ProviderRequestHandler<'a> = Box<dyn FnMut(&Value) -> CliResult<Value> + 'a>;
type ProviderRequestWrittenHandler<'a> = Box<dyn FnMut(&Value) -> CliResult<()> + 'a>;

/// One Kimi ACP child is one process-local RuntimeHandle. Reverse-RPC handlers
/// stay injected by the owning Team loop so canonical permission/interaction
/// authority remains outside the transport adapter.
pub(crate) struct KimiTeamRuntime<'a> {
    client: KimiAcpClient,
    description: crate::runtime_adapter_contract::RuntimeDescription,
    authority_session: Option<AgentSession>,
    on_provider_request: ProviderRequestHandler<'a>,
    on_provider_request_written: ProviderRequestWrittenHandler<'a>,
    last_cycle_terminal: bool,
    last_cycle_cancelled: bool,
}

impl<'a> KimiTeamRuntime<'a> {
    pub(crate) fn new(
        client: KimiAcpClient,
        on_provider_request: impl FnMut(&Value) -> CliResult<Value> + 'a,
        on_provider_request_written: impl FnMut(&Value) -> CliResult<()> + 'a,
    ) -> Self {
        Self {
            client,
            description: crate::runtime_adapter_contract::RuntimeDescription {
                binding_id: "kimi-acp-0.36.1".to_string(),
                native_protocol: "acp-jsonrpc-v1".to_string(),
                composition_fingerprint: String::new(),
                capability_fingerprint: String::new(),
                capability_bindings: Vec::new(),
            },
            authority_session: None,
            on_provider_request: Box::new(on_provider_request),
            on_provider_request_written: Box::new(on_provider_request_written),
            last_cycle_terminal: true,
            last_cycle_cancelled: false,
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

    fn observation(
        &self,
    ) -> Result<
        crate::runtime_adapter_contract::RuntimeObservation,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        let session = self.authority_session.as_ref().ok_or_else(|| {
            crate::runtime_adapter_contract::RuntimeContractError::FenceMismatch {
                fields: vec!["authority_session".to_string()],
            }
        })?;
        Ok(crate::runtime_adapter_contract::RuntimeObservation {
            native_session_ref: self.client.session_id().map(str::to_string),
            active_effect_id: None,
            continuation: session.control_state.continuation.clone(),
            observed_at: crate::now_string(),
        })
    }

    fn unsupported(operation: &str) -> crate::runtime_adapter_contract::RuntimeContractError {
        crate::runtime_adapter_contract::RuntimeContractError::InvalidCapabilityBindings(format!(
            "Kimi ACP 0.36.1 does not expose a reviewed {operation} primitive"
        ))
    }
}

impl crate::runtime_adapter::TeamRuntimeAdapter for KimiTeamRuntime<'_> {
    type Error = CliError;

    fn provider(&self) -> &'static str {
        "kimi"
    }

    fn display_name(&self) -> &'static str {
        "Kimi"
    }

    fn capability_bindings() -> Vec<crate::runtime_adapter::CapabilityBinding> {
        use crate::runtime_adapter::{CapabilityBinding, CapabilityStatus};
        vec![
            CapabilityBinding {
                capability: "open_or_resume",
                status: CapabilityStatus::Supported,
                evidence: "Kimi ACP 0.36.1 initialize + session/new|resume; attach replay drained before the next prompt"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "start_cycle",
                status: CapabilityStatus::Supported,
                evidence: "session/prompt prompt-scoped update or terminal success proves acceptance; correlated response supplies stopReason"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inject_current_cycle",
                status: CapabilityStatus::Unsupported,
                evidence: "ACP 0.36.1 exposes no reviewed content-steer method".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "queue_at_native_boundary",
                status: CapabilityStatus::Unsupported,
                evidence: "ordinary Messages stay in the Harness next-round queue; ACP exposes no native follow-up queue"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "interrupt_current_cycle",
                status: CapabilityStatus::Supported,
                evidence: "session/cancel notification plus correlated prompt stopReason=cancelled"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "close_runtime",
                status: CapabilityStatus::Supported,
                evidence: "Kimi ACP 0.36.1 advertises sessionCapabilities.close; session/close response then client stdin close, clean process exit and child reap retain the native session id"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "observe",
                status: CapabilityStatus::Supported,
                evidence: "owned ACP child/stdout-reader liveness plus correlated prompt boundary; no transcript mirroring"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inspect_effect",
                status: CapabilityStatus::Unsupported,
                evidence: "ACP has no durable provider operation id for post-crash inspection"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "reconcile_effect",
                status: CapabilityStatus::Unsupported,
                evidence: "ACP has no provider-side uncertain-effect reconciliation operation"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inspect_continuation",
                status: CapabilityStatus::Unsupported,
                evidence: "Kimi Goals are not exposed by the reviewed ACP control surface"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "inhibit_continuation",
                status: CapabilityStatus::Unsupported,
                evidence: "ACP exposes no reviewed Goal pause/replace/cancel primitive".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "resume_continuation",
                status: CapabilityStatus::Unsupported,
                evidence: "ACP exposes no reviewed Goal resume primitive".into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "quiesce",
                status: CapabilityStatus::Degraded,
                evidence: "active prompt can settle, but ACP exposes no complete native queue, writable-child, or durable-flush proof; strict quiesce fails closed"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "release",
                status: CapabilityStatus::Degraded,
                evidence: "strict Release requires verified Quiesce; Team Close uses the narrower close_runtime receipt and does not claim flush"
                    .into(),
                security_enforcement_locus: None,
            },
            CapabilityBinding {
                capability: "permission_enforcement",
                status: CapabilityStatus::Degraded,
                evidence: "trusted full_access uses the injected exact permission callback; narrower Kimi ceilings remain inadmissible"
                    .into(),
                security_enforcement_locus: Some(
                    "adapter_auto_approval: trusted full_access only; no narrow ACP sandbox".into(),
                ),
            },
        ]
    }

    fn ensure_alive(&mut self) -> CliResult<()> {
        self.client.ensure_transport_alive()
    }

    fn native_session_locator(&self) -> &str {
        self.client.session_id().unwrap_or("")
    }

    fn native_locator_kind(&self) -> &'static str {
        "kimi_code_session"
    }

    fn bind_authority_session(
        &mut self,
        session: AgentSession,
        profile: &harness_core::ProviderIntegrationProfile,
    ) -> CliResult<()> {
        if session.provider_kind != "kimi"
            || profile.provider != "kimi"
            || profile.execution_mode != "kimi_acp"
        {
            return Err(CliError::Usage(format!(
                "RUNTIME_ADAPTER_PROVIDER_MISMATCH: Kimi adapter cannot bind session={} profile={}/{}",
                session.provider_kind, profile.provider, profile.execution_mode
            )));
        }
        if self.client.provider_version() != Some("0.36.1")
            || profile.provider_version.as_deref() != Some("0.36.1")
        {
            return Err(CliError::Usage(format!(
                "RUNTIME_ADAPTER_VERSION_MISMATCH: Kimi binding requires exact 0.36.1; client={:?} profile={:?}",
                self.client.provider_version(),
                profile.provider_version
            )));
        }
        if session.effective_permission_ceiling
            != harness_core::agentfirm_api::PermissionCeiling::FullAccess
        {
            return Err(CliError::Usage(
                "KIMI_PERMISSION_ADMISSION_FAILED: kimi_acp has no reviewed narrow sandbox; only frozen trusted full_access is admissible"
                    .to_string(),
            ));
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
                    "RUNTIME_ADAPTER_FENCE_INCOMPLETE: persisted Kimi profile/session composition fingerprint mismatch"
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
                    "RUNTIME_ADAPTER_FENCE_INCOMPLETE: persisted Kimi profile/session capability fingerprint mismatch"
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
            &crate::runtime_adapter::SteerRequest,
            &crate::runtime_adapter::SteerProviderResult,
        ) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> crate::runtime_adapter::CycleControl,
    ) -> CliResult<crate::runtime_adapter::ExecutionCycleOutcome> {
        let mut final_text = String::new();
        let mut tool_call_count = 0_u32;
        let mut accepted_receipt = None;
        let mut interrupted = false;
        let mut close_requested = false;
        let mut cancel_requested = false;
        let mut control_error = None;
        let request_handler = &mut self.on_provider_request;
        let request_written_handler = &mut self.on_provider_request_written;
        // ACP delivers Session updates and reverse provider requests through
        // separate callbacks, but both belong to the same owner-private live
        // projection. Share the display-safe projector without retaining or
        // mirroring provider payloads.
        let on_event = std::cell::RefCell::new(on_event);

        self.last_cycle_terminal = false;
        self.last_cycle_cancelled = false;
        let outcome = self.client.prompt(
            input,
            idle_timeout,
            |receipt_id| {
                let receipt = crate::runtime_adapter::ControlTransportReceipt {
                    command: "prompt".to_string(),
                    response_id: Some(receipt_id.to_string()),
                    success: true,
                };
                on_input_accepted(&receipt)?;
                accepted_receipt = Some(receipt);
                Ok(())
            },
            |update| {
                if update.get("sessionUpdate").and_then(Value::as_str)
                    == Some("agent_message_chunk")
                {
                    if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                        final_text.push_str(text);
                    }
                } else if update.get("sessionUpdate").and_then(Value::as_str)
                    == Some("tool_call")
                {
                    tool_call_count = tool_call_count.saturating_add(1);
                }
                (on_event.borrow_mut())(update);
            },
            |request| {
                (on_event.borrow_mut())(request);
                request_handler(request)
            },
            |request| request_written_handler(request),
            || {
                let control = poll_control();
                if let Some(error) = control.fatal_error {
                    control_error = Some(error);
                    return Err(CliError::Usage(
                        control_error.clone().expect("just assigned"),
                    ));
                }
                for pending in &control.injects {
                    on_steer_result(
                        pending,
                        &crate::runtime_adapter::SteerProviderResult::NotApplied(
                            "PROVIDER_CAPABILITY_UNSUPPORTED: kimi_acp has no current-cycle injection"
                                .to_string(),
                        ),
                    )?;
                }
                if control.close || control.interrupt {
                    interrupted = true;
                    close_requested |= control.close;
                    cancel_requested = true;
                    Ok(PromptControl::Cancel)
                } else {
                    Ok(PromptControl::Continue)
                }
            },
        )?;
        if let Some(error) = control_error {
            return Err(CliError::Usage(error));
        }
        if let Some(provider_error) = outcome.provider_error {
            return Err(CliError::Usage(format!(
                "KIMI_CYCLE_PROVIDER_ERROR: {provider_error}"
            )));
        }
        let input_acceptance_receipt = accepted_receipt.ok_or_else(|| {
            CliError::Usage(
                "RUNTIME_COMMAND_RECOVERY_REQUIRED: Kimi cycle had no correlated input-acceptance receipt"
                    .to_string(),
            )
        })?;
        self.last_cycle_terminal = true;
        self.last_cycle_cancelled =
            matches!(outcome.stop_reason.as_str(), "cancelled" | "canceled");
        let control_receipts = if cancel_requested {
            vec![crate::runtime_adapter::ControlTransportReceipt {
                command: "abort".to_string(),
                response_id: Some(format!(
                    "{}:stopReason={}",
                    input_acceptance_receipt
                        .response_id
                        .as_deref()
                        .unwrap_or("kimi-acp-prompt"),
                    outcome.stop_reason
                )),
                success: self.last_cycle_cancelled,
            }]
        } else {
            Vec::new()
        };
        let process = self.client.observe_runtime()?;
        Ok(crate::runtime_adapter::ExecutionCycleOutcome {
            final_text,
            provider_terminal_failure: None,
            interrupted,
            close_requested_by_harness: close_requested,
            tool_call_count,
            input_acceptance_receipt,
            control_receipts,
            terminal_observation: crate::runtime_adapter::RuntimeObservation {
                transport_alive: process.transport_alive,
                process_alive: process.process_alive,
                is_streaming: Some(process.prompt_active),
                pending_message_count: None,
                steering_mode: Some("unsupported".to_string()),
                follow_up_mode: Some("harness_next_round_batched".to_string()),
                settled_boundary_observed: process.settled_boundary_observed,
            },
        })
    }

    fn project_live(
        event: &Value,
    ) -> Option<(crate::provider_event_api::LiveProviderActivityKind, String)> {
        use crate::provider_event_api::LiveProviderActivityKind;
        if event.get("method").and_then(Value::as_str) == Some("session/request_permission") {
            return Some((
                LiveProviderActivityKind::InteractionWaiting,
                "Kimi is waiting for interaction".to_string(),
            ));
        }
        let kind = event.get("sessionUpdate").and_then(Value::as_str)?;
        match kind {
            "agent_thought_chunk" => Some((
                LiveProviderActivityKind::Thinking,
                "Kimi is thinking".to_string(),
            )),
            "agent_message_chunk" => Some((
                LiveProviderActivityKind::ResponseStreaming,
                format!(
                    "assistant response streaming · {} chars",
                    event
                        .pointer("/content/text")
                        .and_then(Value::as_str)
                        .map(str::len)
                        .unwrap_or(0)
                ),
            )),
            "tool_call" => Some((
                LiveProviderActivityKind::ToolStarted,
                "tool started".to_string(),
            )),
            "tool_call_update" => {
                let status = event.get("status").and_then(Value::as_str).unwrap_or("");
                let failed = matches!(status, "failed" | "error" | "cancelled" | "canceled");
                let terminal = failed || matches!(status, "completed" | "success" | "succeeded");
                terminal.then(|| {
                    (
                        if failed {
                            LiveProviderActivityKind::ToolFailed
                        } else {
                            LiveProviderActivityKind::ToolCompleted
                        },
                        if failed {
                            "tool failed".to_string()
                        } else {
                            "tool completed".to_string()
                        },
                    )
                })
            }
            _ => None,
        }
    }

    fn native_control<'b>(
        close: &'b mut bool,
        interrupt: &'b mut bool,
    ) -> Box<dyn crate::provider_adapter::ProviderNativeControl + 'b> {
        Box::new(KimiNeutralNativeControl { close, interrupt })
    }
}

struct KimiNeutralNativeControl<'a> {
    close: &'a mut bool,
    interrupt: &'a mut bool,
}

impl crate::provider_adapter::ProviderNativeControl for KimiNeutralNativeControl<'_> {
    fn provider(&self) -> &'static str {
        "kimi"
    }

    fn dispatch(
        &mut self,
        plan: &crate::provider_adapter::ProviderControlPlan,
    ) -> Result<(), String> {
        use crate::provider_adapter::{NativeControlPrimitive, ProviderControlAction};
        if plan.primitive != NativeControlPrimitive::KimiAcpCancel {
            return Err(format!(
                "PROVIDER_CONTROL_UNPROVEN: Kimi adapter received {:?}",
                plan.primitive
            ));
        }
        *self.interrupt = true;
        *self.close = plan.action == ProviderControlAction::CloseSession;
        Ok(())
    }
}

fn kimi_contract_bridge_error(
    error: impl std::fmt::Display,
) -> crate::runtime_adapter_contract::RuntimeContractError {
    crate::runtime_adapter_contract::RuntimeContractError::InvalidCapabilityBindings(format!(
        "Kimi ACP 0.36.1 native bridge operation failed: {error}"
    ))
}

impl crate::runtime_adapter_contract::RuntimeAdapter for KimiTeamRuntime<'_> {
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
            .map_err(kimi_contract_bridge_error)?;
        if native_session_ref
            .zip(self.client.session_id())
            .is_some_and(|(expected, actual)| expected != actual)
        {
            return Err(
                crate::runtime_adapter_contract::RuntimeContractError::FenceMismatch {
                    fields: vec!["native_session_ref.native_session_id".to_string()],
                },
            );
        }
        self.observation()
    }

    fn execute_control(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
        request: crate::runtime_adapter_contract::ControlRequest,
    ) -> Result<
        crate::runtime_adapter_contract::EffectReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        use crate::runtime_adapter::TeamRuntimeAdapter as _;
        use crate::runtime_adapter_contract::{ControlIntent, SemanticCapability};

        let capability = match &request.intent {
            ControlIntent::StartCycle { .. } => SemanticCapability::StartCycle,
            ControlIntent::InjectCurrentCycle { .. } => SemanticCapability::InjectCurrentCycle,
            ControlIntent::QueueNativeBoundary { .. } => SemanticCapability::QueueNativeBoundary,
            ControlIntent::Interrupt => SemanticCapability::Interrupt,
            ControlIntent::InhibitContinuation { .. } => SemanticCapability::InhibitContinuation,
            ControlIntent::ResumeContinuation { .. } => SemanticCapability::ResumeContinuation,
        };
        let admission = self.contract_preflight(fence, capability)?;
        match request.intent {
            ControlIntent::StartCycle { input } => {
                let mut accepted = None;
                let outcome = self
                    .run_cycle(
                        &input,
                        Duration::from_secs(30 * 60),
                        &mut |receipt| {
                            accepted = receipt.response_id.clone();
                            Ok(())
                        },
                        &mut |_pending, _result| Ok(()),
                        &mut |_event| {},
                        &mut crate::runtime_adapter::CycleControl::default,
                    )
                    .map_err(kimi_contract_bridge_error)?;
                let receipt = accepted.ok_or_else(|| {
                    kimi_contract_bridge_error("prompt completed without acceptance receipt")
                })?;
                Ok(crate::runtime_adapter_contract::EffectReceipt {
                    effect_id: request.effect_id,
                    certainty: RuntimeEffectCertainty::Applied,
                    postcondition: RuntimePostconditionStatus::Satisfied,
                    admission: admission.admission,
                    native_evidence: vec![
                        format!("kimi.session_prompt.accepted:{receipt}"),
                        format!(
                            "kimi.session_prompt.terminal:settled={}",
                            outcome.terminal_observation.settled_boundary_observed
                        ),
                    ],
                })
            }
            ControlIntent::Interrupt => {
                // The live Team loop compiles Interrupt inside `run_cycle`,
                // where it can pair the notification with the exact active
                // prompt and its terminal response. A standalone mutable call
                // here cannot coexist with that borrowed cycle handle, so it
                // must not send an unbound session-scoped cancel.
                let _ = (request.effect_id, admission);
                Err(Self::unsupported(
                    "standalone interrupt outside an active run_cycle control boundary",
                ))
            }
            ControlIntent::InjectCurrentCycle { .. }
            | ControlIntent::QueueNativeBoundary { .. }
            | ControlIntent::InhibitContinuation { .. }
            | ControlIntent::ResumeContinuation { .. } => {
                Err(Self::unsupported(capability.as_str()))
            }
        }
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
            .observe_runtime()
            .map_err(kimi_contract_bridge_error)?;
        self.observation()
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
        Err(Self::unsupported("inspect_effect"))
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
        Err(Self::unsupported("reconcile_effect"))
    }

    fn close_runtime(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
    ) -> Result<
        crate::runtime_adapter_contract::MemberRuntimeCloseReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::CloseRuntime,
        )?;
        let before = self
            .client
            .observe_runtime()
            .map_err(kimi_contract_bridge_error)?;
        if before.prompt_active || !before.settled_boundary_observed || !self.last_cycle_terminal {
            return Err(
                crate::runtime_adapter_contract::RuntimeContractError::MemberCloseIncomplete {
                    fields: vec!["current_cycle_terminal=Unknown".to_string()],
                },
            );
        }
        let retained_session = self.client.session_id().map(str::to_string);
        let close = self
            .client
            .close_session_and_runtime()
            .map_err(kimi_contract_bridge_error)?;
        let released = self
            .client
            .observe_runtime()
            .map_err(kimi_contract_bridge_error)?;
        let process_released = !released.transport_alive
            && !released.process_alive
            && close.shutdown.process_reaped
            && close.shutdown.stdout_reader_joined;
        let native_retained = retained_session.as_deref() == Some(close.session_id.as_str());
        let cycle_evidence = if self.last_cycle_cancelled {
            "kimi.session_cancel.terminal:stopReason=cancelled"
        } else {
            "kimi.current_cycle.already_terminal_before_close"
        };
        let receipt = crate::runtime_adapter_contract::MemberRuntimeCloseReceipt {
            control_acknowledged: RuntimePostconditionStatus::Satisfied,
            current_cycle_terminal: RuntimePostconditionStatus::Satisfied,
            managed_runtime_released: if process_released {
                RuntimePostconditionStatus::Satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            live_handle_disposed: if process_released {
                RuntimePostconditionStatus::Satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            native_session_retained: if native_retained {
                RuntimePostconditionStatus::Satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            evidence: vec![
                cycle_evidence.to_string(),
                format!("kimi.session_close.response:{}", close.response_id),
                format!(
                    "kimi.acp.clean_exit:{};child_reaped={};stdout_reader_joined={}",
                    close.shutdown.exit_status,
                    close.shutdown.process_reaped,
                    close.shutdown.stdout_reader_joined
                ),
                format!("kimi.native_session_retained:{}", close.session_id),
            ],
        };
        receipt.verify()?;
        Ok(receipt)
    }

    fn quiesce(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
    ) -> Result<
        crate::runtime_adapter_contract::QuiesceReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        use crate::runtime_adapter_contract::{QuiesceReceiptBuilder, QuiesceStep};

        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::Quiesce,
        )?;
        let process = self
            .client
            .observe_runtime()
            .map_err(kimi_contract_bridge_error)?;
        let session = self.authority_session.as_ref().ok_or_else(|| {
            crate::runtime_adapter_contract::RuntimeContractError::FenceMismatch {
                fields: vec!["authority_session".to_string()],
            }
        })?;
        let terminal = if !process.prompt_active && process.settled_boundary_observed {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unknown
        };
        let continuation = if matches!(
            session.control_state.continuation.activation,
            NativeContinuationActivation::Disarmed
        ) {
            RuntimePostconditionStatus::Satisfied
        } else {
            RuntimePostconditionStatus::Unknown
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
            "Kimi ACP exposes no native Goal controller; only durably disarmed continuation is satisfiable",
        )?;
        builder.record(
            QuiesceStep::SettleActiveCycle,
            terminal,
            format!(
                "prompt_active={};settled={}",
                process.prompt_active, process.settled_boundary_observed
            ),
        )?;
        builder.record(
            QuiesceStep::DrainNativeQueue,
            RuntimePostconditionStatus::Unknown,
            "Kimi ACP 0.36.1 exposes no complete native pending-input/background queue snapshot",
        )?;
        builder.record(
            QuiesceStep::DrainWritableChildren,
            RuntimePostconditionStatus::Unknown,
            "trusted full_access may create writers outside the owned ACP process group; ACP exposes no complete child/job inventory",
        )?;
        builder.record(
            QuiesceStep::ObserveIdle,
            terminal,
            "correlated session/prompt response is a cycle boundary only",
        )?;
        builder.record(
            QuiesceStep::ConfirmFlush,
            RuntimePostconditionStatus::Unknown,
            "session/close does not acknowledge durable native-session-store flush",
        )?;
        let receipt = builder.finish();
        receipt.verify()?;
        Ok(receipt)
    }

    fn release(
        &mut self,
        fence: crate::runtime_adapter_contract::RuntimeFence<'_>,
    ) -> Result<
        crate::runtime_adapter_contract::ReleaseReceipt,
        crate::runtime_adapter_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            crate::runtime_adapter_contract::SemanticCapability::Release,
        )?;
        Err(crate::runtime_adapter_contract::RuntimeContractError::CompositionSwapRequiresQuiesce)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_adapter::TeamRuntimeAdapter;

    fn bound_adapter() -> (
        KimiTeamRuntime<'static>,
        harness_core::agentfirm_api::RuntimeCommandBinding,
        String,
        u64,
    ) {
        use harness_core::agentfirm_api::{
            AgentSessionControlState, AgentSessionStatus, MemberExecutionDriver,
            NativeSessionAvailability, NativeSessionRef, PermissionCeiling, RuntimeActivity,
            RuntimeDriverRef, RuntimeResidency,
        };

        let mut profile = crate::team_member_provider_profile_for_mode("kimi", Some("kimi_acp"));
        crate::apply_provider_version(&mut profile, Some("0.36.1".to_string()));
        let node_daemon_id = "daemon-kimi-test".to_string();
        let node_daemon_generation = 3;
        let native_session_ref = NativeSessionRef {
            provider: "kimi".to_string(),
            execution_mode: "kimi_acp".to_string(),
            native_session_id: "scripted-session".to_string(),
            native_locator_kind: "kimi_code_session".to_string(),
            provider_version: Some("0.36.1".to_string()),
            adapter_contract_version: "kimi-acp-v1".to_string(),
            availability: NativeSessionAvailability::Available,
            supports_resume: true,
            last_verified_at: None,
            parent_native_session_id: None,
        };
        let session = AgentSession {
            id: "agent-session-kimi-test".to_string(),
            agent_member_id: "identity-kimi-test".to_string(),
            node_id: "node-kimi-test".to_string(),
            execution_space_id: "space-kimi-test".to_string(),
            node_daemon_id: node_daemon_id.clone(),
            node_daemon_generation,
            provider_kind: "kimi".to_string(),
            provider_profile_ref: "profile-kimi-test".to_string(),
            permission_envelope_ref: "permission-kimi-test".to_string(),
            effective_permission_ceiling: PermissionCeiling::FullAccess,
            lifecycle: AgentSessionStatus::Idle,
            runtime_generation: 7,
            control_state: AgentSessionControlState {
                runtime_residency: RuntimeResidency::Attached,
                activity: RuntimeActivity::Idle,
                execution_driver: MemberExecutionDriver::HostDriven,
                driver_generation: 11,
                driver_ref: RuntimeDriverRef::NodeDaemon {
                    node_daemon_id: node_daemon_id.clone(),
                    node_daemon_generation,
                },
                composition_fingerprint: profile.composition_fingerprint.clone(),
                capability_fingerprint: profile.capability_fingerprint.clone(),
                ..Default::default()
            },
            native_session_ref: Some(native_session_ref),
            current_turn_id: None,
            queued_input_count: 0,
            version: 1,
            opened_at: "2026-08-15T00:00:00Z".to_string(),
            last_active_at: "2026-08-15T00:00:00Z".to_string(),
            closed_at: None,
        };
        let binding = harness_core::agentfirm_api::RuntimeCommandBinding {
            target_session_id: Some(session.id.clone()),
            target_runtime_generation: Some(session.runtime_generation),
            target_driver_generation: Some(session.control_state.driver_generation),
            target_driver: session.control_state.driver_ref.clone(),
            native_session_ref: session.native_session_ref.clone(),
            composition_fingerprint: session.control_state.composition_fingerprint.clone(),
            capability_fingerprint: session.control_state.capability_fingerprint.clone(),
            capability_profile_version: Some("agentfirm-runtime-adapter-v1".to_string()),
            permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
        };
        let client = KimiAcpClient::scripted_for_close_contract();
        let mut adapter =
            KimiTeamRuntime::new(client, |_request| Ok(Value::Null), |_request| Ok(()));
        adapter
            .bind_authority_session(session, &profile)
            .expect("bind exact Kimi authority");
        (adapter, binding, node_daemon_id, node_daemon_generation)
    }

    #[test]
    fn capability_surface_keeps_close_separate_from_strict_quiesce_and_release() {
        let bindings = KimiTeamRuntime::capability_bindings();
        let status = |name| {
            bindings
                .iter()
                .find(|binding| binding.capability == name)
                .map(|binding| binding.status)
                .expect("binding exists")
        };
        assert_eq!(
            status("close_runtime"),
            crate::runtime_adapter::CapabilityStatus::Supported
        );
        assert_eq!(
            status("quiesce"),
            crate::runtime_adapter::CapabilityStatus::Degraded
        );
        assert_eq!(
            status("release"),
            crate::runtime_adapter::CapabilityStatus::Degraded
        );
        assert_eq!(
            status("inject_current_cycle"),
            crate::runtime_adapter::CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn live_projection_never_copies_thought_or_tool_payload_content() {
        let thought = serde_json::json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": {"text": "secret chain of thought"}
        });
        let (_, preview) = KimiTeamRuntime::project_live(&thought).expect("thought phase");
        assert_eq!(preview, "Kimi is thinking");
        assert!(!preview.contains("secret"));

        let tool = serde_json::json!({
            "sessionUpdate": "tool_call",
            "title": "cat /private/secret",
            "rawInput": {"token": "credential"}
        });
        let (_, preview) = KimiTeamRuntime::project_live(&tool).expect("tool phase");
        assert_eq!(preview, "tool started");
        assert!(!preview.contains("private"));
        assert!(!preview.contains("credential"));

        let interaction = serde_json::json!({
            "jsonrpc":"2.0",
            "id":7,
            "method":"session/request_permission",
            "params":{"sessionId":"scripted-session"}
        });
        let (kind, preview) =
            KimiTeamRuntime::project_live(&interaction).expect("reverse request waiting phase");
        assert_eq!(
            kind,
            crate::provider_event_api::LiveProviderActivityKind::InteractionWaiting
        );
        assert_eq!(preview, "Kimi is waiting for interaction");
    }

    #[cfg(unix)]
    #[test]
    fn close_runtime_proves_provider_ack_clean_reap_and_native_session_retention_once() {
        use crate::runtime_adapter_contract::RuntimeAdapter;

        let (mut adapter, binding, daemon_id, daemon_generation) = bound_adapter();
        let fence = crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &daemon_id,
            target_node_daemon_generation: daemon_generation,
        };
        let receipt = adapter
            .close_runtime(fence)
            .expect("verified reversible Team Close");
        receipt.verify().expect("complete close receipt");
        assert_eq!(
            receipt.native_session_retained,
            RuntimePostconditionStatus::Satisfied
        );
        assert!(receipt
            .evidence
            .iter()
            .any(|evidence| evidence.contains("session_close.response:2")));
        assert!(receipt
            .evidence
            .iter()
            .any(|evidence| evidence.contains("exit status: 0")));
        assert_eq!(adapter.native_session_locator(), "scripted-session");

        let second = adapter.close_runtime(crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &daemon_id,
            target_node_daemon_generation: daemon_generation,
        });
        assert!(second.is_err(), "live handle disposal must be one-shot");
    }

    #[cfg(unix)]
    #[test]
    fn strict_quiesce_and_release_are_denied_before_any_process_effect() {
        use crate::runtime_adapter_contract::RuntimeAdapter;

        let (mut adapter, binding, daemon_id, daemon_generation) = bound_adapter();
        let quiesce = adapter.quiesce(crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &daemon_id,
            target_node_daemon_generation: daemon_generation,
        });
        assert!(matches!(
            quiesce,
            Err(
                crate::runtime_adapter_contract::RuntimeContractError::CapabilityAdmissionDenied {
                    capability: crate::runtime_adapter_contract::SemanticCapability::Quiesce,
                    ..
                }
            )
        ));
        let release = adapter.release(crate::runtime_adapter_contract::RuntimeFence {
            binding: &binding,
            target_node_daemon_id: &daemon_id,
            target_node_daemon_generation: daemon_generation,
        });
        assert!(matches!(
            release,
            Err(
                crate::runtime_adapter_contract::RuntimeContractError::CapabilityAdmissionDenied {
                    capability: crate::runtime_adapter_contract::SemanticCapability::Release,
                    ..
                }
            )
        ));
        adapter
            .ensure_alive()
            .expect("failed-closed strong operations must not touch provider process");
    }
}
