use super::*;

fn prepared_command_recovery(command_id: &str, error: impl std::fmt::Display) -> CliError {
    CliError::RuntimeRecoveryRequired(format!(
        "command {command_id} was durably prepared; reconcile before provider effect: {error}"
    ))
}

pub(crate) fn current_node_daemon_lease_after_admission_at(
    store: &harness_store::HarnessStore,
    admitted_lease: &harness_core::NodeDaemonLease,
    now_unix_ms: u64,
    command_id: &str,
) -> CliResult<harness_core::NodeDaemonLease> {
    store
        .latest_node_daemon_lease(&admitted_lease.node_id)
        .map_err(|error| prepared_command_recovery(command_id, error))?
        .filter(|lease| {
            lease.node_id == admitted_lease.node_id
                && lease.daemon_id == admitted_lease.daemon_id
                && lease.instance_id == admitted_lease.instance_id
                && lease.generation == admitted_lease.generation
                && lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > now_unix_ms
        })
        .ok_or_else(|| {
            prepared_command_recovery(
                command_id,
                "NODE_DAEMON_CURRENT_LEASE_FENCED_AFTER_ADMISSION",
            )
        })
}

fn current_node_daemon_lease_after_admission(
    store: &harness_store::HarnessStore,
    admitted_lease: &harness_core::NodeDaemonLease,
    command_id: &str,
) -> CliResult<harness_core::NodeDaemonLease> {
    current_node_daemon_lease_after_admission_at(
        store,
        admitted_lease,
        current_unix_ms_u64(),
        command_id,
    )
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderEffectAdmission {
    pub(crate) command_id: String,
    pub(crate) settle_context: harness_core::agentfirm_api::MutationContext,
    pub(crate) control_plan: Option<crate::provider_adapter::ProviderControlPlan>,
    pub(crate) target_session: harness_core::agentfirm_api::AgentSession,
    pub(crate) fence: crate::runtime_adapter_contract::RuntimeBindingFence,
}

pub(crate) fn runtime_command_postcondition_for(
    kind: harness_core::agentfirm_api::RuntimeCommandKind,
) -> harness_core::agentfirm_api::RuntimeCommandPostcondition {
    use harness_core::agentfirm_api::{
        RuntimeAcknowledgementLevel, RuntimeCommandKind, RuntimeDesiredPostcondition,
    };
    let desired_postcondition = match kind {
        RuntimeCommandKind::StartSession
        | RuntimeCommandKind::ResumeSession
        | RuntimeCommandKind::OpenRuntime
        | RuntimeCommandKind::ResumeNativeSession
        | RuntimeCommandKind::ReattachLiveRuntime
        | RuntimeCommandKind::ReopenMember => RuntimeDesiredPostcondition::RuntimeAttached,
        RuntimeCommandKind::DispatchProvider | RuntimeCommandKind::StartCycle => {
            RuntimeDesiredPostcondition::CycleStarted
        }
        RuntimeCommandKind::CancelProviderTurn | RuntimeCommandKind::InterruptCurrentCycle => {
            RuntimeDesiredPostcondition::CurrentCycleTerminal
        }
        RuntimeCommandKind::ReleaseRuntime | RuntimeCommandKind::CloseMember => {
            RuntimeDesiredPostcondition::RuntimeReleased
        }
        RuntimeCommandKind::QuiesceExecutionLane | RuntimeCommandKind::DrainRuntime => {
            RuntimeDesiredPostcondition::ExecutionLaneQuiesced
        }
        RuntimeCommandKind::ActivateContinuation | RuntimeCommandKind::ResumeContinuation => {
            RuntimeDesiredPostcondition::ContinuationActivated
        }
        RuntimeCommandKind::InhibitContinuation | RuntimeCommandKind::ClearContinuation => {
            RuntimeDesiredPostcondition::ContinuationInhibited
        }
        RuntimeCommandKind::TransferExecutionDriver => {
            RuntimeDesiredPostcondition::DriverTransferred
        }
        RuntimeCommandKind::InspectCommandEffect
        | RuntimeCommandKind::ReconcileUnknownEffect
        | RuntimeCommandKind::AbortIfNotApplied => RuntimeDesiredPostcondition::StateReconciled,
        RuntimeCommandKind::CancelPendingInput => {
            RuntimeDesiredPostcondition::PendingInputCancelled
        }
        RuntimeCommandKind::AuthorMessage
        | RuntimeCommandKind::StopSession
        | RuntimeCommandKind::RetireMember
        | RuntimeCommandKind::DeleteNativeSession
        | RuntimeCommandKind::InjectCurrentCycle
        | RuntimeCommandKind::QueueAtNativeBoundary
        | RuntimeCommandKind::InspectContinuation
        | RuntimeCommandKind::ReplaceContinuationCondition
        | RuntimeCommandKind::StopBackgroundTask => {
            RuntimeDesiredPostcondition::ProviderAcknowledged
        }
    };
    harness_core::agentfirm_api::RuntimeCommandPostcondition {
        desired_ack_level: RuntimeAcknowledgementLevel::ProviderReceipt,
        desired_postcondition,
        ..Default::default()
    }
}

/// Durably admit one real provider turn before crossing the provider boundary.
///
/// The Team runtime is only a collaboration overlay here. The exact current
/// machine-local AgentSession and NodeDaemon generation are resolved from the
/// canonical store, and a replayed or ambiguous command never re-enters the
/// provider transport.
pub(crate) fn prepare_provider_effect_kind(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    source_record_id: &str,
    content: &str,
    command_kind: harness_core::agentfirm_api::RuntimeCommandKind,
    required_capability: &str,
    provider_attempt: Option<u64>,
) -> CliResult<ProviderEffectAdmission> {
    use harness_core::agentfirm_api::{
        ActorKind, ActorRef, AgentSessionStatus, ControlCommandEnvelope, RuntimeCommandStatus,
        RuntimeEffectCertainty,
    };

    let (execution_space_id, canonical_member, session) =
        provider_runtime_subject_for_member(ledger, member)
            .map_err(|error| CliError::ProviderAdmissionRejected(error.to_string()))?;
    let lifecycle_is_eligible = match command_kind {
        harness_core::agentfirm_api::RuntimeCommandKind::DispatchProvider
        | harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
        | harness_core::agentfirm_api::RuntimeCommandKind::InjectCurrentCycle
        | harness_core::agentfirm_api::RuntimeCommandKind::QueueAtNativeBoundary
        | harness_core::agentfirm_api::RuntimeCommandKind::CancelProviderTurn
        | harness_core::agentfirm_api::RuntimeCommandKind::InterruptCurrentCycle
        | harness_core::agentfirm_api::RuntimeCommandKind::CancelPendingInput => {
            session.lifecycle == AgentSessionStatus::Active
        }
        harness_core::agentfirm_api::RuntimeCommandKind::QuiesceExecutionLane
        | harness_core::agentfirm_api::RuntimeCommandKind::ReleaseRuntime
        | harness_core::agentfirm_api::RuntimeCommandKind::CloseMember => matches!(
            session.lifecycle,
            AgentSessionStatus::Cold
                | AgentSessionStatus::Active
                | AgentSessionStatus::Idle
                | AgentSessionStatus::Waiting
                | AgentSessionStatus::Interrupted
        ),
        harness_core::agentfirm_api::RuntimeCommandKind::StopSession => matches!(
            session.lifecycle,
            AgentSessionStatus::Cold
                | AgentSessionStatus::Active
                | AgentSessionStatus::Idle
                | AgentSessionStatus::Waiting
                | AgentSessionStatus::Interrupted
        ),
        _ => false,
    };
    if !lifecycle_is_eligible {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_CONTROL_NOT_ELIGIBLE: {:?} cannot target session {} in {:?}",
            command_kind, session.id, session.lifecycle
        )));
    }
    let lease = ledger
        .store
        .latest_node_daemon_lease(&session.node_id)
        .map_err(|error| CliError::ProviderAdmissionRejected(error.to_string()))?
        .filter(|lease| {
            lease.daemon_id == session.node_daemon_id
                && lease.generation == session.node_daemon_generation
                && lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| {
            CliError::ProviderAdmissionRejected("NODE_DAEMON_GENERATION_FENCED".into())
        })?;
    crate::provider_adapter::map_permission(
        &session.provider_kind,
        session.effective_permission_ceiling,
    )
    .map_err(CliError::ProviderAdmissionRejected)?;

    let content_fingerprint =
        harness_store::canonical_json_fingerprint(&serde_json::json!({"content": content}));
    let payload = serde_json::json!({
        "session_id": session.id,
        "session_generation": session.runtime_generation,
        "delivery_id": source_record_id,
        "provider": session.provider_kind,
        "command_kind": command_kind,
        "permission_ceiling": session.effective_permission_ceiling,
        "content_fingerprint": content_fingerprint,
        "provider_attempt": provider_attempt,
    });
    let payload_fingerprint = harness_store::canonical_json_fingerprint(&payload);
    let source_fingerprint = harness_store::canonical_json_fingerprint(
        &serde_json::json!({"source_record_id": source_record_id}),
    );
    let idempotency_key = format!(
        "provider-effect:{}:{}:{command_kind:?}:{}:{}",
        session.id, session.runtime_generation, source_fingerprint, content_fingerprint
    );
    let command_id = format!("runtime-command:{idempotency_key}");
    let daemon_actor = ActorRef {
        kind: ActorKind::Service,
        id: lease.daemon_id.clone(),
    };
    let command = ControlCommandEnvelope {
        id: command_id.clone(),
        execution_space_id: execution_space_id.clone(),
        target_node_id: session.node_id.clone(),
        target_node_daemon_id: lease.daemon_id.clone(),
        target_node_daemon_generation: lease.generation,
        authenticated_actor: daemon_actor.clone(),
        command: command_kind,
        required_capability: required_capability.into(),
        idempotency_key: idempotency_key.clone(),
        expected_version: 0,
        // Exact replay must reproduce the same full envelope. Bind expiry to
        // the already-frozen daemon lease and use an idempotency-derived
        // observation marker instead of sampling a new wall clock.
        expires_unix_ms: lease.expires_unix_ms,
        binding: runtime_command_binding_for_member_session(&canonical_member, &session),
        precondition: harness_core::agentfirm_api::RuntimeCommandPrecondition {
            expected_session_version: Some(session.version),
            expected_residency: Some(session.control_state.runtime_residency),
            expected_activity: Some(session.control_state.activity),
            expected_execution_driver: Some(session.control_state.execution_driver),
            ..Default::default()
        },
        postcondition: runtime_command_postcondition_for(command_kind),
        payload,
        payload_fingerprint: payload_fingerprint.clone(),
        issued_at: format!("runtime-command:{idempotency_key}"),
    };
    let command_fingerprint = harness_store::runtime_command_envelope_fingerprint(&command)?;
    let admission_context = harness_core::agentfirm_api::MutationContext {
        execution_space_id: execution_space_id.clone(),
        authenticated_actor: daemon_actor.clone(),
        authority_actor: Some(daemon_actor.clone()),
        command_name: "node_daemon.provider_effect.prepare".into(),
        idempotency_key,
        expected_version: 0,
        request_fingerprint: Some(command_fingerprint),
    };
    let admission = ledger
        .store
        .prepare_runtime_command(
            &admission_context,
            &command,
            current_unix_ms_u64(),
            &now_string(),
        )
        .map_err(|error| CliError::ProviderAdmissionRejected(error.to_string()))?;
    if admission.replayed {
        let replay = match (
            admission.projection.status,
            admission.projection.effect_certainty,
        ) {
            (RuntimeCommandStatus::Applied, RuntimeEffectCertainty::Applied) => {
                Err(CliError::ProviderEffectAccepted(
                    admission.projection.id.clone(),
                ))
            }
            (RuntimeCommandStatus::Failed, RuntimeEffectCertainty::NotApplied) => {
                Err(CliError::Usage(format!(
                    "RUNTIME_COMMAND_REPLAY_FAILED: provider effect {} will not be repeated with the same attempt",
                    admission.projection.id
                )))
            }
            _ => Err(CliError::RuntimeRecoveryRequired(format!(
                "provider effect {} has unresolved certainty and will not be repeated",
                admission.projection.id
            ))),
        };
        return replay;
    }
    let current_lease =
        current_node_daemon_lease_after_admission(&ledger.store, &lease, &command_id)?;
    let fence = runtime_binding_fence_for_admission(
        ledger,
        &admission,
        &session,
        &canonical_member,
        &current_lease,
    )
    .map_err(|error| prepared_command_recovery(&command_id, error))?;
    Ok(ProviderEffectAdmission {
        command_id,
        control_plan: None,
        target_session: session,
        fence,
        settle_context: harness_core::agentfirm_api::MutationContext {
            execution_space_id: admission_context.execution_space_id,
            authenticated_actor: daemon_actor,
            authority_actor: None,
            command_name: "node_daemon.provider_effect.settle".into(),
            idempotency_key: format!("{}:settle", admission_context.idempotency_key),
            expected_version: admission.projection.version,
            request_fingerprint: None,
        },
    })
}

pub(crate) fn prepare_provider_effect(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    source_record_id: &str,
    content: &str,
    provider_attempt: u64,
) -> CliResult<ProviderEffectAdmission> {
    prepare_provider_effect_kind(
        ledger,
        member,
        source_record_id,
        content,
        harness_core::agentfirm_api::RuntimeCommandKind::StartCycle,
        "cycle.start",
        Some(provider_attempt),
    )
}

pub(crate) fn prepare_provider_process_effect(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    transport_attempt: u64,
) -> CliResult<ProviderEffectAdmission> {
    use harness_core::agentfirm_api::{
        ActorKind, ActorRef, ControlCommandEnvelope, RuntimeCommandKind,
    };
    let (execution_space_id, canonical_member, session) =
        provider_runtime_subject_for_member(ledger, member)
            .map_err(classify_pre_effect_provider_admission_error)?;
    let lease = ledger
        .store
        .latest_node_daemon_lease(&session.node_id)
        .map_err(|error| classify_pre_effect_provider_admission_error(error.into()))?
        .filter(|lease| {
            lease.daemon_id == session.node_daemon_id
                && lease.generation == session.node_daemon_generation
                && lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| {
            CliError::ProviderAdmissionRejected("NODE_DAEMON_GENERATION_FENCED".into())
        })?;
    crate::provider_adapter::map_permission(
        &session.provider_kind,
        session.effective_permission_ceiling,
    )
    .map_err(CliError::ProviderAdmissionRejected)?;
    let kind = if member.native_session.is_some() {
        RuntimeCommandKind::ResumeNativeSession
    } else {
        RuntimeCommandKind::OpenRuntime
    };
    let payload = serde_json::json!({
        "session_id": session.id,
        "session_generation": session.runtime_generation,
        "provider": session.provider_kind,
        "permission_ceiling": session.effective_permission_ceiling,
        "native_resume_ref": member.native_session.as_ref().map(|value| &value.native_session_id),
    });
    let fingerprint = harness_store::canonical_json_fingerprint(&payload);
    let idempotency_key = provider_process_idempotency_key(
        &session,
        canonical_member.runtime_generation,
        ledger.supervisor_generation,
        transport_attempt,
        kind,
    );
    let command_id = format!("runtime-command:{idempotency_key}");
    require_new_provider_process_command(&ledger.store, &execution_space_id, &command_id)?;
    let daemon_actor = ActorRef {
        kind: ActorKind::Service,
        id: lease.daemon_id.clone(),
    };
    let command = ControlCommandEnvelope {
        id: command_id.clone(),
        execution_space_id: execution_space_id.clone(),
        target_node_id: session.node_id.clone(),
        target_node_daemon_id: lease.daemon_id.clone(),
        target_node_daemon_generation: lease.generation,
        authenticated_actor: daemon_actor.clone(),
        command: kind,
        required_capability: if kind == RuntimeCommandKind::OpenRuntime {
            "runtime.open"
        } else {
            "runtime.native_session.resume"
        }
        .into(),
        idempotency_key: idempotency_key.clone(),
        expected_version: 0,
        // Exact retries reconcile by durable command identity above, before a
        // renewed lease can alter this first command's frozen envelope.
        expires_unix_ms: lease.expires_unix_ms,
        binding: runtime_command_binding_for_member_session(&canonical_member, &session),
        precondition: harness_core::agentfirm_api::RuntimeCommandPrecondition {
            expected_session_version: Some(session.version),
            expected_residency: Some(session.control_state.runtime_residency),
            expected_activity: Some(session.control_state.activity),
            expected_execution_driver: Some(session.control_state.execution_driver),
            ..Default::default()
        },
        postcondition: runtime_command_postcondition_for(kind),
        payload,
        payload_fingerprint: fingerprint.clone(),
        issued_at: format!("runtime-command:{idempotency_key}"),
    };
    let command_fingerprint = harness_store::runtime_command_envelope_fingerprint(&command)?;
    let context = harness_core::agentfirm_api::MutationContext {
        execution_space_id,
        authenticated_actor: daemon_actor.clone(),
        authority_actor: Some(daemon_actor.clone()),
        command_name: "node_daemon.provider_process.prepare".into(),
        idempotency_key: idempotency_key.clone(),
        expected_version: 0,
        request_fingerprint: Some(command_fingerprint),
    };
    let admission = ledger
        .store
        .prepare_runtime_command(&context, &command, current_unix_ms_u64(), &now_string())
        .map_err(|error| classify_pre_effect_provider_admission_error(error.into()))?;
    if admission.replayed {
        return Err(CliError::RuntimeRecoveryRequired(format!(
            "provider process command {} already exists as {:?}/{:?}; reconcile before spawn",
            admission.projection.id,
            admission.projection.status,
            admission.projection.effect_certainty
        )));
    }
    let current_lease =
        current_node_daemon_lease_after_admission(&ledger.store, &lease, &command_id)?;
    let fence = runtime_binding_fence_for_admission(
        ledger,
        &admission,
        &session,
        &canonical_member,
        &current_lease,
    )
    .map_err(|error| prepared_command_recovery(&command_id, error))?;
    Ok(ProviderEffectAdmission {
        command_id,
        control_plan: None,
        target_session: session,
        fence,
        settle_context: harness_core::agentfirm_api::MutationContext {
            execution_space_id: context.execution_space_id,
            authenticated_actor: daemon_actor,
            authority_actor: None,
            command_name: "node_daemon.provider_process.settle".into(),
            idempotency_key: format!("{idempotency_key}:settle"),
            expected_version: admission.projection.version,
            request_fingerprint: None,
        },
    })
}
