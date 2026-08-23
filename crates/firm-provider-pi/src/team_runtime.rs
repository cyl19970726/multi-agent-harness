//! Provider-neutral Team runtime binding backed by the Pi RPC client.

use std::path::Path;
use std::time::Duration;

use crate::{confirm_pi_session_flush, PiRpcClient, HANDSHAKE_TIMEOUT};
use crate::{PiError as CliError, PiResult as CliResult};

fn now_string() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

struct PiNativeControl<'a> {
    close: &'a mut bool,
    interrupt: &'a mut bool,
}

impl harness_runtime_contract::ProviderNativeControl for PiNativeControl<'_> {
    fn provider(&self) -> &'static str {
        "pi"
    }

    fn dispatch(
        &mut self,
        plan: &harness_runtime_contract::ProviderControlPlan,
    ) -> Result<(), String> {
        use harness_runtime_contract::{NativeControlPrimitive, ProviderControlAction};
        if plan.primitive != NativeControlPrimitive::PiRpcInterrupt {
            return Err(format!(
                "PROVIDER_CONTROL_UNPROVEN: Pi adapter received {:?}",
                plan.primitive
            ));
        }
        *self.interrupt = true;
        *self.close = plan.action == ProviderControlAction::CloseSession;
        Ok(())
    }
}

pub struct PiTeamRuntime {
    client: PiRpcClient,
    description: harness_runtime_contract::RuntimeDescription,
    authority_session: Option<harness_core::agentfirm_api::AgentSession>,
    canonical_quiesced: bool,
    canonical_released: bool,
}

impl PiTeamRuntime {
    pub fn new(client: PiRpcClient) -> Self {
        Self {
            client,
            description: harness_runtime_contract::RuntimeDescription {
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
}

impl harness_runtime_contract::TeamRuntimeAdapter for PiTeamRuntime {
    type Error = CliError;

    fn provider(&self) -> &'static str {
        "pi"
    }

    fn display_name(&self) -> &'static str {
        "Pi"
    }

    fn capability_bindings() -> Vec<harness_runtime_contract::CapabilityBinding> {
        use harness_runtime_contract::{CapabilityBinding, CapabilityStatus};
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
                capability: "close_runtime",
                status: CapabilityStatus::Supported,
                evidence: "get_state proves the current cycle and native input queue idle; the one-shot owned process-group disposer reaps the Pi child while retaining the native JSONL session file. This narrower Team Close does not claim full writable-child drain or durable flush"
                    .into(),
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
            &harness_runtime_contract::ControlTransportReceipt,
        ) -> CliResult<()>,
        on_steer_result: &mut dyn FnMut(
            &harness_runtime_contract::SteerRequest,
            &harness_runtime_contract::SteerProviderResult,
        ) -> CliResult<()>,
        on_event: &mut dyn FnMut(&serde_json::Value),
        poll_control: &mut dyn FnMut() -> harness_runtime_contract::CycleControl,
    ) -> CliResult<harness_runtime_contract::ExecutionCycleOutcome> {
        let outcome = self.client.prompt_dyn(
            input,
            idle_timeout,
            on_input_accepted,
            on_steer_result,
            &mut *on_event,
            &mut *poll_control,
        )?;
        Ok(harness_runtime_contract::ExecutionCycleOutcome {
            final_text: outcome.final_text,
            provider_terminal_failure: None,
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
    ) -> Option<(harness_runtime_contract::LiveProviderActivityKind, String)> {
        PiRpcClient::project_live(event)
    }

    fn native_control<'a>(
        close: &'a mut bool,
        interrupt: &'a mut bool,
    ) -> Box<dyn harness_runtime_contract::ProviderNativeControl + 'a> {
        Box::new(PiNativeControl { close, interrupt })
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
) -> harness_runtime_contract::RuntimeContractError {
    harness_runtime_contract::RuntimeContractError::InvalidCapabilityBindings(format!(
        "Pi native bridge operation failed: {error}"
    ))
}

impl harness_runtime_contract::RuntimeAdapter for PiTeamRuntime {
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
            .map_err(pi_contract_bridge_error)?;
        if native_session_ref.is_some_and(|expected| expected != self.client.session_file()) {
            return Err(
                harness_runtime_contract::RuntimeContractError::FenceMismatch {
                    fields: vec!["native_session_ref.native_session_id".to_string()],
                },
            );
        }
        let session = self
            .authority_session
            .as_ref()
            .expect("preflight bound session");
        Ok(harness_runtime_contract::RuntimeObservation {
            native_session_ref: Some(self.client.session_file().to_string()),
            active_effect_id: None,
            continuation: session.control_state.continuation.clone(),
            observed_at: now_string(),
        })
    }

    fn execute_control(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
        request: harness_runtime_contract::ControlRequest,
    ) -> Result<
        harness_runtime_contract::EffectReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        use harness_core::agentfirm_api::{RuntimeEffectCertainty, RuntimePostconditionStatus};
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
                        harness_runtime_contract::CycleControl::default,
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

        Ok(harness_runtime_contract::EffectReceipt {
            effect_id: request.effect_id,
            certainty,
            postcondition,
            admission: admission.admission,
            native_evidence,
        })
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
            .observe_runtime(false)
            .map_err(pi_contract_bridge_error)?;
        let session = self
            .authority_session
            .as_ref()
            .expect("preflight bound session");
        Ok(harness_runtime_contract::RuntimeObservation {
            native_session_ref: Some(self.client.session_file().to_string()),
            active_effect_id: None,
            continuation: session.control_state.continuation.clone(),
            observed_at: now_string(),
        })
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
        unreachable!("Pi inspect_effect is not admitted")
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
        unreachable!("Pi reconcile is not admitted")
    }

    fn close_runtime(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
    ) -> Result<
        harness_runtime_contract::MemberRuntimeCloseReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        use harness_core::agentfirm_api::RuntimePostconditionStatus;

        if self.canonical_released {
            return Err(harness_runtime_contract::RuntimeContractError::AlreadyReleased);
        }
        self.contract_preflight(
            fence,
            harness_runtime_contract::SemanticCapability::CloseRuntime,
        )?;
        let (writable_children, writable_children_evidence) =
            self.client.writable_children_drain_proof();
        if writable_children != RuntimePostconditionStatus::Satisfied {
            return Err(
                harness_runtime_contract::RuntimeContractError::CapabilityAdmissionDenied {
                    capability: harness_runtime_contract::SemanticCapability::CloseRuntime,
                    admission: harness_core::ProviderBindingAdmission::PendingDependency,
                    reasons: vec![writable_children_evidence],
                },
            );
        }
        let session_file = self.client.session_file().to_string();
        let boundary = self
            .client
            .quiesce_runtime()
            .map_err(pi_contract_bridge_error)?;
        if !boundary.drained {
            return Err(
                harness_runtime_contract::RuntimeContractError::MemberCloseIncomplete {
                    fields: vec!["current_cycle_terminal=Unknown".to_string()],
                },
            );
        }
        let observation = self.client.release().map_err(pi_contract_bridge_error)?;
        let released = !observation.transport_alive && !observation.process_alive;
        let retained = Path::new(&session_file).is_file();
        let receipt = harness_runtime_contract::MemberRuntimeCloseReceipt {
            control_acknowledged: RuntimePostconditionStatus::Satisfied,
            current_cycle_terminal: RuntimePostconditionStatus::Satisfied,
            managed_runtime_released: if released {
                RuntimePostconditionStatus::Satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            live_handle_disposed: if released {
                RuntimePostconditionStatus::Satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            native_session_retained: if retained {
                RuntimePostconditionStatus::Satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            evidence: vec![
                format!("pi.get_state.member_close_boundary:{}", boundary.evidence),
                format!(
                    "pi.owned_process_group_released:transport_alive={};process_alive={}",
                    observation.transport_alive, observation.process_alive
                ),
                writable_children_evidence,
                format!("pi.native_session_retained:{session_file}"),
            ],
        };
        receipt.verify()?;
        self.canonical_released = true;
        Ok(receipt)
    }

    fn quiesce(
        &mut self,
        fence: harness_runtime_contract::RuntimeBindingFence,
    ) -> Result<
        harness_runtime_contract::QuiesceReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        use harness_core::agentfirm_api::{
            NativeContinuationActivation, RuntimePostconditionStatus,
        };
        use harness_runtime_contract::{QuiesceReceiptBuilder, QuiesceStep};

        self.contract_preflight(fence, harness_runtime_contract::SemanticCapability::Quiesce)?;
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
            "exact RuntimeBindingFence admitted",
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
        fence: harness_runtime_contract::RuntimeBindingFence,
    ) -> Result<
        harness_runtime_contract::ReleaseReceipt,
        harness_runtime_contract::RuntimeContractError,
    > {
        use harness_core::agentfirm_api::RuntimePostconditionStatus;

        if self.canonical_released {
            return Err(harness_runtime_contract::RuntimeContractError::AlreadyReleased);
        }
        self.contract_preflight(fence, harness_runtime_contract::SemanticCapability::Release)?;
        if !self.canonical_quiesced {
            return Err(
                harness_runtime_contract::RuntimeContractError::CompositionSwapRequiresQuiesce,
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
        let receipt = harness_runtime_contract::ReleaseReceipt {
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
            return Err(harness_runtime_contract::RuntimeContractError::ReleaseIncomplete);
        }
        self.authority_session = None;
        self.canonical_released = true;
        Ok(receipt)
    }
}
