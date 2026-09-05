//! Provider-neutral Team runtime binding for reviewed Kimi Code ACP versions.
//!
//! The durable AgentSession and NativeSessionRef remain the authority. This
//! module owns only the process-local ACP handle and compiles neutral runtime
//! intents into the exact reviewed wire operations:
//!
//! - open/resume is proved by `initialize` plus `session/new|resume` in
//!   [`KimiAcpClient`](crate::KimiAcpClient);
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
    AgentSession, NativeContinuationActivation, RuntimePostconditionStatus,
};
use serde_json::Value;

use crate::{KimiAcpClient, PromptControl};
use crate::{KimiError as CliError, KimiResult as CliResult};

const REVIEWED_KIMI_ACP_RUNTIME_VERSIONS: &[&str] = &["0.36.1", "0.39.0"];

fn reviewed_runtime_version_pair(client: Option<&str>, profile: Option<&str>) -> bool {
    client == profile
        && client.is_some_and(|version| REVIEWED_KIMI_ACP_RUNTIME_VERSIONS.contains(&version))
}

fn now_string() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

type ProviderRequestHandler<'a> = Box<dyn FnMut(&Value) -> CliResult<Value> + 'a>;
type ProviderRequestWrittenHandler<'a> = Box<dyn FnMut(&Value) -> CliResult<()> + 'a>;

/// One Kimi ACP child is one process-local RuntimeHandle. Reverse-RPC handlers
/// stay injected by the owning Team loop so canonical permission/interaction
/// authority remains outside the transport adapter.
pub struct KimiTeamRuntime<'a> {
    client: KimiAcpClient,
    description: harness_runtime_contract::RuntimeDescription,
    authority_session: Option<AgentSession>,
    on_provider_request: ProviderRequestHandler<'a>,
    on_provider_request_written: ProviderRequestWrittenHandler<'a>,
    last_cycle_terminal: bool,
    last_cycle_cancelled: bool,
}

impl<'a> KimiTeamRuntime<'a> {
    pub fn new(
        client: KimiAcpClient,
        on_provider_request: impl FnMut(&Value) -> CliResult<Value> + 'a,
        on_provider_request_written: impl FnMut(&Value) -> CliResult<()> + 'a,
    ) -> Self {
        let binding_id = format!(
            "kimi-acp-{}",
            client.provider_version().unwrap_or("unverified")
        );
        Self {
            client,
            description: harness_runtime_contract::RuntimeDescription {
                binding_id,
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
        fence: harness_runtime_contract::RuntimeBindingFence,
        capability: harness_runtime_contract::SemanticCapability,
    ) -> Result<
        harness_runtime_contract::AdmissionDecision,
        harness_runtime_contract::RuntimeContractError,
    > {
        let session = self.authority_session.as_ref().ok_or_else(|| {
            harness_runtime_contract::RuntimeContractError::FenceMismatch {
                fields: vec!["authority_session".to_string()],
            }
        })?;
        harness_runtime_contract::preflight_effect(
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
        harness_runtime_contract::RuntimeObservation,
        harness_runtime_contract::RuntimeContractError,
    > {
        let session = self.authority_session.as_ref().ok_or_else(|| {
            harness_runtime_contract::RuntimeContractError::FenceMismatch {
                fields: vec!["authority_session".to_string()],
            }
        })?;
        Ok(harness_runtime_contract::RuntimeObservation {
            native_session_ref: self.client.session_id().map(str::to_string),
            active_effect_id: None,
            continuation: session.control_state.continuation.clone(),
            observed_at: now_string(),
        })
    }

    fn unsupported(operation: &str) -> harness_runtime_contract::RuntimeContractError {
        harness_runtime_contract::RuntimeContractError::InvalidCapabilityBindings(format!(
            "Kimi ACP does not expose a reviewed {operation} primitive"
        ))
    }
}

impl harness_runtime_contract::TeamRuntimeAdapter for KimiTeamRuntime<'_> {
    type Error = CliError;

    fn provider(&self) -> &'static str {
        "kimi"
    }

    fn display_name(&self) -> &'static str {
        "Kimi"
    }

    fn capability_bindings() -> Vec<harness_runtime_contract::CapabilityBinding> {
        use harness_runtime_contract::{CapabilityBinding, CapabilityStatus};
        vec![
            CapabilityBinding {
                capability: "open_or_resume",
                status: CapabilityStatus::Supported,
                evidence: "Reviewed Kimi ACP initialize + session/new|resume; attach replay drained before the next prompt"
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
                evidence: "Reviewed Kimi ACP exposes no content-steer method".into(),
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
                evidence: "Reviewed Kimi ACP advertises sessionCapabilities.close; session/close response then client stdin close, clean process exit and child reap retain the native session id"
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
        let client_version = self.client.provider_version();
        let profile_version = profile.provider_version.as_deref();
        if !reviewed_runtime_version_pair(client_version, profile_version) {
            return Err(CliError::Usage(format!(
                "RUNTIME_ADAPTER_VERSION_MISMATCH: Kimi binding requires an exact client/profile match in reviewed versions {:?}; client={client_version:?} profile={profile_version:?}",
                REVIEWED_KIMI_ACP_RUNTIME_VERSIONS,
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
        timeouts: harness_runtime_contract::CycleTimeouts,
        on_input_accepted: &mut dyn FnMut(
            &harness_runtime_contract::ControlTransportReceipt,
        ) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(
            &harness_runtime_contract::SteerRequest,
            &harness_runtime_contract::SteerProviderResult,
        ) -> CliResult<()>,
        on_event: &mut dyn FnMut(&Value),
        poll_control: &mut dyn FnMut() -> harness_runtime_contract::CycleControl,
    ) -> CliResult<harness_runtime_contract::ExecutionCycleOutcome> {
        let mut final_text = String::new();
        let mut tool_call_count = 0_u32;
        let mut accepted_receipt = None;
        let mut interrupt: Option<harness_runtime_contract::InterruptCause> = None;
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
            timeouts,
            |receipt_id| {
                let receipt = harness_runtime_contract::ControlTransportReceipt {
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
            |request| {
                request_written_handler(request)
            },
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
                        &harness_runtime_contract::SteerProviderResult::NotApplied(
                            "PROVIDER_CAPABILITY_UNSUPPORTED: kimi_acp has no current-cycle injection"
                                .to_string(),
                        ),
                    )
                    ?;
                }
                if control.close || control.interrupt {
                    interrupt = Some(harness_runtime_contract::InterruptCause::HostControl);
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
        // Kimi keeps its pre-S3 recovery semantics (ADR 0041, pinned by the
        // team_run_api recovery tests): a provider failure — before OR after
        // acceptance — stops at RecoveryRequired through this Err, never at
        // a fabricated failed-but-Idle round. The StartCycle receipt path is
        // equally fail-closed: execute_control propagates this Err, so no
        // receipt is produced and none can settle Satisfied/Applied (#709).
        // Unifying failure handling across adapters is a tracked follow-up.
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
            vec![harness_runtime_contract::ControlTransportReceipt {
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
        let provider_input_id = outcome.provider_input_id;
        let exact_terminal_ref = format!(
            "kimi_acp.session_prompt:{provider_input_id}:stop_reason={}",
            outcome.stop_reason
        );
        Ok(harness_runtime_contract::ExecutionCycleOutcome {
            final_text,
            provider_terminal_failure: None,
            interrupt,
            close_requested_by_harness: close_requested,
            tool_call_count,
            native_correlation: harness_runtime_contract::NativeCycleCorrelation {
                provider_input_id: provider_input_id.clone(),
                input_acceptance_receipt,
                terminal_provider_input_id: Some(provider_input_id),
                exact_terminal_ref: Some(exact_terminal_ref),
            },
            control_receipts,
            terminal_observation: harness_runtime_contract::CycleRuntimeObservation {
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

    fn native_control<'b>(
        close: &'b mut bool,
        interrupt: &'b mut bool,
    ) -> Box<dyn harness_runtime_contract::ProviderNativeControl + 'b> {
        Box::new(KimiNeutralNativeControl { close, interrupt })
    }
}

struct KimiNeutralNativeControl<'a> {
    close: &'a mut bool,
    interrupt: &'a mut bool,
}

impl harness_runtime_contract::ProviderNativeControl for KimiNeutralNativeControl<'_> {
    fn provider(&self) -> &'static str {
        "kimi"
    }

    fn dispatch(
        &mut self,
        plan: &harness_runtime_contract::ProviderControlPlan,
    ) -> Result<(), String> {
        use harness_runtime_contract::{NativeControlPrimitive, ProviderControlAction};
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
) -> harness_runtime_contract::RuntimeContractError {
    harness_runtime_contract::RuntimeContractError::InvalidCapabilityBindings(format!(
        "Kimi ACP native bridge operation failed: {error}"
    ))
}

impl harness_runtime_contract::RuntimeAdapter for KimiTeamRuntime<'_> {
    fn describe(&self) -> &harness_runtime_contract::RuntimeDescription {
        &self.description
    }

    fn open_or_resume(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
        native_session_ref: Option<&str>,
    ) -> Result<
        harness_runtime_contract::RuntimeObservation,
        harness_runtime_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            harness_runtime_contract::SemanticCapability::OpenOrResume,
        )?;
        self.client
            .ensure_transport_alive()
            .map_err(kimi_contract_bridge_error)?;
        if native_session_ref
            .zip(self.client.session_id())
            .is_some_and(|(expected, actual)| expected != actual)
        {
            return Err(
                harness_runtime_contract::RuntimeContractError::FenceMismatch {
                    fields: vec!["native_session_ref.native_session_id".to_string()],
                },
            );
        }
        self.observation()
    }

    fn execute_control(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
        request: harness_runtime_contract::ControlRequest,
    ) -> Result<
        harness_runtime_contract::EffectReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        use harness_runtime_contract::TeamRuntimeAdapter as _;
        use harness_runtime_contract::{ControlIntent, SemanticCapability};

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
                        harness_runtime_contract::CycleTimeouts::with_input_acceptance(
                            Duration::from_secs(30 * 60),
                        ),
                        &mut |receipt| {
                            accepted = receipt.response_id.clone();
                            Ok(())
                        },
                        &mut |_pending, _result| Ok(()),
                        &mut |_event| {},
                        &mut harness_runtime_contract::CycleControl::default,
                    )
                    .map_err(kimi_contract_bridge_error)?;
                let receipt = accepted.ok_or_else(|| {
                    kimi_contract_bridge_error("prompt completed without acceptance receipt")
                })?;
                Ok(harness_runtime_contract::EffectReceipt::for_cycle(
                    request.effect_id,
                    admission.admission,
                    harness_runtime_contract::CycleSettlement::from_cycle_outcome(&outcome),
                )
                .with_native_evidence([
                    format!("kimi.session_prompt.accepted:{receipt}"),
                    format!(
                        "kimi.session_prompt.terminal:settled={}",
                        outcome.terminal_observation.settled_boundary_observed
                    ),
                ]))
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
        fence: harness_runtime_contract::RuntimeBindingFence,
    ) -> Result<
        harness_runtime_contract::RuntimeObservation,
        harness_runtime_contract::RuntimeContractError,
    > {
        self.contract_preflight(fence, harness_runtime_contract::SemanticCapability::Observe)?;
        self.client
            .observe_runtime()
            .map_err(kimi_contract_bridge_error)?;
        self.observation()
    }

    fn inspect_effect(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
        _effect_id: &str,
    ) -> Result<
        harness_runtime_contract::EffectInspection,
        harness_runtime_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            harness_runtime_contract::SemanticCapability::InspectEffect,
        )?;
        Err(Self::unsupported("inspect_effect"))
    }

    fn reconcile(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
        _inspection: &harness_runtime_contract::EffectInspection,
    ) -> Result<
        harness_runtime_contract::ReconcileReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            harness_runtime_contract::SemanticCapability::Reconcile,
        )?;
        Err(Self::unsupported("reconcile_effect"))
    }

    fn close_runtime(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
    ) -> Result<
        harness_runtime_contract::MemberRuntimeCloseReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        self.contract_preflight(
            fence,
            harness_runtime_contract::SemanticCapability::CloseRuntime,
        )?;
        let before = self
            .client
            .observe_runtime()
            .map_err(kimi_contract_bridge_error)?;
        if before.prompt_active || !before.settled_boundary_observed || !self.last_cycle_terminal {
            return Err(
                harness_runtime_contract::RuntimeContractError::MemberCloseIncomplete {
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
        let receipt = harness_runtime_contract::MemberRuntimeCloseReceipt {
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
        fence: harness_runtime_contract::RuntimeBindingFence,
    ) -> Result<
        harness_runtime_contract::QuiesceReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        use harness_runtime_contract::{QuiesceReceiptBuilder, QuiesceStep};

        self.contract_preflight(fence, harness_runtime_contract::SemanticCapability::Quiesce)?;
        let process = self
            .client
            .observe_runtime()
            .map_err(kimi_contract_bridge_error)?;
        let session = self.authority_session.as_ref().ok_or_else(|| {
            harness_runtime_contract::RuntimeContractError::FenceMismatch {
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
            "exact RuntimeBindingFence admitted",
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
            "Reviewed Kimi ACP exposes no complete native pending-input/background queue snapshot",
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
        fence: harness_runtime_contract::RuntimeBindingFence,
    ) -> Result<
        harness_runtime_contract::ReleaseReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        self.contract_preflight(fence, harness_runtime_contract::SemanticCapability::Release)?;
        Err(harness_runtime_contract::RuntimeContractError::CompositionSwapRequiresQuiesce)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::{
        AgentRuntimeProvider, ControlTopology, OrdinaryMessageBoundary, ProviderBindingAdmission,
        ProviderCapabilityBinding, ProviderCapabilityEvidence, ProviderCapabilityEvidenceKind,
        ProviderCapabilityStatus, ProviderCompatibilityStatus, ProviderEventFidelity,
        ProviderFeatureMode, ProviderInteractionMode, SecurityEnforcementLocus,
        SecurityEnforcementLocusKind,
    };
    use harness_runtime_contract::TeamRuntimeAdapter;

    #[test]
    fn runtime_version_admission_requires_exact_reviewed_pair() {
        assert!(reviewed_runtime_version_pair(
            Some("0.36.1"),
            Some("0.36.1")
        ));
        assert!(reviewed_runtime_version_pair(
            Some("0.39.0"),
            Some("0.39.0")
        ));
        assert!(!reviewed_runtime_version_pair(
            Some("0.39.0"),
            Some("0.36.1")
        ));
        assert!(!reviewed_runtime_version_pair(
            Some("0.40.0"),
            Some("0.40.0")
        ));
        assert!(!reviewed_runtime_version_pair(None, None));
    }

    fn reviewed_profile() -> harness_core::ProviderIntegrationProfile {
        harness_core::ProviderIntegrationProfile {
            agent_runtime_provider: Some(AgentRuntimeProvider("kimi".to_string())),
            model_route: None,
            provider: "kimi".to_string(),
            execution_mode: "kimi_acp".to_string(),
            execution_driver: harness_core::agentfirm_api::MemberExecutionDriver::HostDriven,
            provider_version: Some("0.36.1".to_string()),
            adapter_contract_version: Some("kimi-acp-v1".to_string()),
            reviewed_provider_versions: vec!["0.36.1".to_string()],
            compatibility_status: ProviderCompatibilityStatus::Current,
            adapter_reviewed_at: None,
            compatibility_note: None,
            interaction_mode: ProviderInteractionMode::PauseAndResume,
            ordinary_message_boundary: OrdinaryMessageBoundary::NextRoundBatched,
            plan_mode: ProviderFeatureMode::Native,
            goal_mode: ProviderFeatureMode::Emulated,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Summary,
            supports_cancel: true,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
            control_topology: ControlTopology::ExternalProtocol,
            composition_fingerprint: Some("composition-kimi-test".to_string()),
            capability_fingerprint: Some("capabilities-kimi-test".to_string()),
            capability_bindings: vec![ProviderCapabilityBinding {
                capability: harness_runtime_contract::SemanticCapability::CloseRuntime
                    .as_str()
                    .to_string(),
                status: ProviderCapabilityStatus::Verified,
                admission: ProviderBindingAdmission::Active,
                provider_version: Some("0.36.1".to_string()),
                adapter_revision: Some("kimi-acp-v1".to_string()),
                feature_fingerprint: Some("feature-close".to_string()),
                required_dependencies: Vec::new(),
                evidence: vec![
                    ProviderCapabilityEvidence {
                        kind: ProviderCapabilityEvidenceKind::DeterministicAcceptance,
                        evidence_ref: "test:kimi_close_runtime".to_string(),
                        observed_at: None,
                        note: None,
                    },
                    ProviderCapabilityEvidence {
                        kind: ProviderCapabilityEvidenceKind::LiveCanary,
                        evidence_ref: "live:DEV-26:kimi_acp@0.36.1:close_runtime".to_string(),
                        observed_at: None,
                        note: None,
                    },
                ],
            }],
            binding_admission: ProviderBindingAdmission::Active,
            adapter_bridge_revision: Some("kimi-acp-v1".to_string()),
            security_enforcement_locus: SecurityEnforcementLocus {
                kind: SecurityEnforcementLocusKind::AdapterAutoApproval,
                note: None,
            },
        }
    }

    fn bound_adapter() -> (
        KimiTeamRuntime<'static>,
        harness_runtime_contract::RuntimeBindingFence,
    ) {
        use harness_core::agentfirm_api::{
            AgentSessionControlState, AgentSessionStatus, MemberExecutionDriver,
            NativeSessionAvailability, NativeSessionRef, PermissionCeiling, RuntimeActivity,
            RuntimeDriverRef, RuntimeResidency,
        };

        let profile = reviewed_profile();
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
            workspace_cwd: Some("/tmp".to_string()),
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
            target_member_run_id: Some("member-run-kimi-test".to_string()),
            target_member_run_generation: Some(session.runtime_generation),
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
        let member = harness_core::agentfirm_api::MemberRun {
            id: "member-run-kimi-test".to_string(),
            agent_member_id: session.agent_member_id.clone(),
            team_run_id: "team-run-kimi-test".to_string(),
            role_snapshot: "member".to_string(),
            provider_profile_snapshot: None,
            requested_controls: serde_json::json!({}),
            effective_controls: serde_json::json!({}),
            coordination_status: harness_core::agentfirm_api::MemberCoordinationStatus::Active,
            runtime_status: harness_core::agentfirm_api::MemberRuntimeStatus::Idle,
            runtime_generation: session.runtime_generation,
            workspace_binding_id: None,
            native_session: session.native_session_ref.clone(),
            version: 1,
            started_at: "t0".to_string(),
            last_event_at: None,
            finished_at: None,
        };
        let daemon = harness_core::NodeDaemonLease {
            node_id: session.node_id.clone(),
            daemon_id: session.node_daemon_id.clone(),
            generation: session.node_daemon_generation,
            instance_id: "instance-kimi-test".to_string(),
            status: harness_core::NodeDaemonLeaseStatus::Active,
            acquired_unix_ms: 1,
            renewed_unix_ms: 1,
            expires_unix_ms: 100,
            released_unix_ms: None,
        };
        let command = harness_core::agentfirm_api::RuntimeCommandRecord {
            id: "command-kimi-test".to_string(),
            execution_space_id: session.execution_space_id.clone(),
            target_node_id: session.node_id.clone(),
            target_node_daemon_id: daemon.daemon_id.clone(),
            target_node_daemon_generation: daemon.generation,
            authenticated_actor: harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::Service,
                id: daemon.daemon_id.clone(),
            },
            command: harness_core::agentfirm_api::RuntimeCommandKind::StartCycle,
            required_capability: "cycle.start".to_string(),
            idempotency_key: "command-kimi-test".to_string(),
            request_fingerprint: "fingerprint-kimi-test".to_string(),
            status: harness_core::agentfirm_api::RuntimeCommandStatus::Accepted,
            phase: harness_core::agentfirm_api::RuntimeCommandPhase::Prepared,
            effect_certainty: harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown,
            postcondition_status: harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown,
            binding,
            precondition: Default::default(),
            postcondition: Default::default(),
            target_session_id: Some(session.id.clone()),
            target_session_generation: Some(session.runtime_generation),
            source_record_id: None,
            provider_attempt: None,
            result: None,
            cycle_correlation: None,
            failure_code: None,
            version: 1,
            created_at: "t0".to_string(),
            updated_at: "t0".to_string(),
        };
        let fence = harness_runtime_contract::RuntimeBindingFence::from_admitted_command(
            &command, &session, &member, &daemon, None, 2,
        )
        .expect("exact admitted runtime binding");
        let client = KimiAcpClient::scripted_for_close_contract();
        let mut adapter =
            KimiTeamRuntime::new(client, |_request| Ok(Value::Null), |_request| Ok(()));
        adapter
            .bind_authority_session(session, &profile)
            .expect("bind exact Kimi authority");
        (adapter, fence)
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
            harness_runtime_contract::CapabilityStatus::Supported
        );
        assert_eq!(
            status("quiesce"),
            harness_runtime_contract::CapabilityStatus::Degraded
        );
        assert_eq!(
            status("release"),
            harness_runtime_contract::CapabilityStatus::Degraded
        );
        assert_eq!(
            status("inject_current_cycle"),
            harness_runtime_contract::CapabilityStatus::Unsupported
        );
    }

    #[cfg(unix)]
    #[test]
    fn close_runtime_proves_provider_ack_clean_reap_and_native_session_retention_once() {
        use harness_runtime_contract::RuntimeAdapter;

        let (mut adapter, fence) = bound_adapter();
        let receipt = adapter
            .close_runtime(fence.clone())
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

        let second = adapter.close_runtime(fence);
        assert!(second.is_err(), "live handle disposal must be one-shot");
    }

    #[cfg(unix)]
    #[test]
    fn strict_quiesce_and_release_are_denied_before_any_process_effect() {
        use harness_runtime_contract::RuntimeAdapter;

        let (mut adapter, fence) = bound_adapter();
        let quiesce = adapter.quiesce(fence.clone());
        assert!(matches!(
            quiesce,
            Err(
                harness_runtime_contract::RuntimeContractError::CapabilityAdmissionDenied {
                    capability: harness_runtime_contract::SemanticCapability::Quiesce,
                    ..
                }
            )
        ));
        let release = adapter.release(fence);
        assert!(matches!(
            release,
            Err(
                harness_runtime_contract::RuntimeContractError::CapabilityAdmissionDenied {
                    capability: harness_runtime_contract::SemanticCapability::Release,
                    ..
                }
            )
        ));
        adapter
            .ensure_alive()
            .expect("failed-closed strong operations must not touch provider process");
    }
}
