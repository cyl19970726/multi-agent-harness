//! Narrow adapter between Remote Fabric references and the Wave 4C runtime.
//!
//! Fabric never constructs a RuntimeCommand or Message. It transports a
//! closed immutable reference; this adapter resolves the canonical command,
//! re-verifies every authority/fingerprint field, then calls the existing
//! NodeDaemon socket. A missing resolver or mismatch fails before the socket.

#![allow(clippy::result_large_err, dead_code)]

use harness_core::agentfirm_api::{
    ActorKind as RuntimeActorKind, ActorRef, ControlCommandEnvelope, Message, RuntimeCommandKind,
};
use harness_core::{ExecutionNodeStatus, NodeDaemonLease, NodeDaemonLeaseStatus};
use harness_fabric::{
    ActorKind as FabricActorKind, ClosedOperationBody, CompanyNode, FabricError, FabricErrorCode,
    MessageReference, NodeAdministrativeStatus, RoutedOperation, RuntimeCommandIntent,
    RuntimeCommandReference,
};

pub(crate) fn runtime_reference_from_operation(
    operation: &RoutedOperation,
) -> Result<RuntimeCommandReference, FabricError> {
    match operation.closed_body()? {
        ClosedOperationBody::RuntimeCommand(reference) => Ok(reference),
        _ => Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "operation is not a RuntimeCommand reference",
        )),
    }
}

pub(crate) fn runtime_intent_from_operation(
    operation: &RoutedOperation,
) -> Result<RuntimeCommandIntent, FabricError> {
    let reference = runtime_reference_from_operation(operation)?;
    let intent = reference.canonical_command_intent;
    let intent_fingerprint = format!(
        "sha256:{}",
        harness_fabric::json_digest(&intent).map_err(|error| FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("RuntimeCommand intent cannot be fingerprinted: {error}"),
        ))?
    );
    if reference.runtime_command_id != intent.id
        || reference.intent_fingerprint != intent_fingerprint
        || reference.target_execution_space_id != intent.target_execution_space_id
        || operation.target_execution_space_id.as_deref()
            != Some(intent.target_execution_space_id.as_str())
        || operation.expected_target_revision != Some(intent.expected_version)
        || operation.expires_at_unix_ms != intent.expires_unix_ms
        || operation.idempotency_key != intent.idempotency_key
        || intent.payload_fingerprint != harness_store::canonical_json_fingerprint(&intent.payload)
        || !runtime_source_authority_matches(operation)
    {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "RuntimeCommand intent disagrees with its immutable route, payload, or source authority",
        ));
    }
    Ok(intent)
}

pub(crate) fn resolved_runtime_command_from_operation(
    operation: &RoutedOperation,
    target_node_id: &str,
    target_node_daemon_id: &str,
    target_node_daemon_generation: u64,
) -> Result<ControlCommandEnvelope, FabricError> {
    let intent = runtime_intent_from_operation(operation)?;
    if operation.target_node_id != target_node_id
        || target_node_daemon_id.trim().is_empty()
        || target_node_daemon_generation == 0
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "target RuntimeCommand resolution requires the exact authenticated NodeDaemon generation",
        ));
    }
    let command: RuntimeCommandKind = serde_json::from_value(serde_json::Value::String(
        intent.command.clone(),
    ))
    .map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("RuntimeCommand intent command is unsupported: {error}"),
        )
    })?;
    let required_capability = match command {
        RuntimeCommandKind::AuthorMessage => "message.author",
        RuntimeCommandKind::StartSession => "agent_session.start",
        RuntimeCommandKind::StopSession => "agent_session.stop",
        RuntimeCommandKind::ResumeSession => "agent_session.resume",
        RuntimeCommandKind::DispatchProvider => "provider.dispatch",
        RuntimeCommandKind::CancelProviderTurn => "provider.cancel",
        RuntimeCommandKind::OpenRuntime => "runtime.open",
        RuntimeCommandKind::ResumeNativeSession => "runtime.native_session.resume",
        RuntimeCommandKind::ReleaseRuntime => "runtime.release",
        RuntimeCommandKind::CloseMember => "member.close",
        RuntimeCommandKind::ReopenMember => "member.reopen",
        RuntimeCommandKind::RetireMember => "member.retire",
        RuntimeCommandKind::DeleteNativeSession => "runtime.native_session.delete",
        RuntimeCommandKind::StartCycle => "cycle.start",
        RuntimeCommandKind::InjectCurrentCycle => "cycle.inject_current",
        RuntimeCommandKind::QueueAtNativeBoundary => "cycle.queue_native_boundary",
        RuntimeCommandKind::InterruptCurrentCycle => "cycle.interrupt_current",
        RuntimeCommandKind::CancelPendingInput => "cycle.pending_input.cancel",
        RuntimeCommandKind::InspectContinuation => "continuation.inspect",
        RuntimeCommandKind::ActivateContinuation => "continuation.activate",
        RuntimeCommandKind::InhibitContinuation => "continuation.inhibit",
        RuntimeCommandKind::ResumeContinuation => "continuation.resume",
        RuntimeCommandKind::ReplaceContinuationCondition => "continuation.condition.replace",
        RuntimeCommandKind::ClearContinuation => "continuation.clear",
        RuntimeCommandKind::QuiesceExecutionLane => "execution_lane.quiesce",
        RuntimeCommandKind::DrainRuntime => "runtime.drain",
        RuntimeCommandKind::StopBackgroundTask => "background_task.stop",
        RuntimeCommandKind::TransferExecutionDriver => "driver.transfer",
        RuntimeCommandKind::InspectCommandEffect => "command_effect.inspect",
        RuntimeCommandKind::ReconcileUnknownEffect => "command_effect.reconcile",
        RuntimeCommandKind::ReattachLiveRuntime => "runtime.reattach",
        RuntimeCommandKind::AbortIfNotApplied => "command_effect.abort_if_not_applied",
    };
    Ok(ControlCommandEnvelope {
        id: intent.id,
        execution_space_id: intent.target_execution_space_id,
        target_node_id: target_node_id.into(),
        target_node_daemon_id: target_node_daemon_id.into(),
        target_node_daemon_generation,
        authenticated_actor: ActorRef {
            kind: RuntimeActorKind::Service,
            id: target_node_daemon_id.into(),
        },
        command,
        required_capability: required_capability.into(),
        idempotency_key: intent.idempotency_key,
        expected_version: intent.expected_version,
        expires_unix_ms: intent.expires_unix_ms,
        binding: Default::default(),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload: intent.payload,
        payload_fingerprint: intent.payload_fingerprint,
        issued_at: intent.issued_at,
    })
}

pub(crate) fn validate_resolved_runtime_command(
    operation: &RoutedOperation,
    envelope: &ControlCommandEnvelope,
    target_node_id: &str,
    target_node_daemon_id: &str,
    target_node_daemon_generation: u64,
) -> Result<(), FabricError> {
    let intent = runtime_intent_from_operation(operation)?;
    let expected: ControlCommandEnvelope = resolved_runtime_command_from_operation(
        operation,
        target_node_id,
        target_node_daemon_id,
        target_node_daemon_generation,
    )?;
    if *envelope != expected
        || envelope.id != intent.id
        || envelope.execution_space_id != intent.target_execution_space_id
        || envelope.authenticated_actor
            != (ActorRef {
                kind: RuntimeActorKind::Service,
                id: envelope.target_node_daemon_id.clone(),
            })
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "resolved RuntimeCommand authority was not constructed by the target NodeDaemon",
        ));
    }
    harness_store::runtime_command_envelope_fingerprint(envelope).map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("resolved RuntimeCommand cannot be fingerprinted: {error}"),
        )
    })?;
    Ok(())
}

/// Join the Company directory Node to the one Wave 4C machine identity and
/// exact current NodeDaemon generation. The Fabric directory never creates a
/// parallel machine identity or daemon authority.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn validate_wave4c_node_authority(
    store: &harness_store::HarnessStore,
    company_node: &CompanyNode,
    daemon_lease: &NodeDaemonLease,
    now_unix_ms: u64,
) -> Result<(), FabricError> {
    let execution_node = store
        .latest_execution_nodes()
        .map_err(store_error)?
        .into_iter()
        .find(|node| node.id == company_node.id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::SourceMismatch,
                "CompanyNode.id must resolve to the exact Wave4C ExecutionNode.id",
            )
        })?;
    if daemon_lease.node_id != company_node.id
        || daemon_lease.status != NodeDaemonLeaseStatus::Active
        || daemon_lease.expires_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "NodeGateway requires the exact current active NodeDaemonLease parent",
        ));
    }
    let current = store
        .latest_node_daemon_lease(&company_node.id)
        .map_err(store_error)?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "ExecutionNode has no current NodeDaemonLease",
            )
        })?;
    if current != *daemon_lease {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "provided NodeDaemonLease is not the latest generation",
        ));
    }
    let status_aligned = match company_node.administrative_status {
        NodeAdministrativeStatus::Active => execution_node.status == ExecutionNodeStatus::Active,
        NodeAdministrativeStatus::Draining => {
            matches!(
                execution_node.status,
                ExecutionNodeStatus::Active | ExecutionNodeStatus::Draining
            )
        }
        NodeAdministrativeStatus::Revoked => false,
    };
    if !status_aligned {
        return Err(FabricError::none(
            FabricErrorCode::NodeRevoked,
            "CompanyNode administrative authority forbids this Wave4C Node runtime",
        ));
    }
    Ok(())
}

/// Decode and independently verify the canonical Wave 4C Message carried by
/// a cross-node route. Fabric identity fields alone never synthesize Message
/// content, and MessageRouteJournal is not written here.
pub(crate) fn resolved_message_from_operation(
    operation: &RoutedOperation,
) -> Result<Message, FabricError> {
    let reference = match operation.closed_body()? {
        ClosedOperationBody::Message(reference) => reference,
        ClosedOperationBody::CollaborationBusiness(reference)
            if reference.business_kind == "team_message_deliver"
                && reference.required_capability == "collaboration.team_message_deliver" =>
        {
            serde_json::from_value::<MessageReference>(
                reference
                    .payload
                    .get("message_reference")
                    .cloned()
                    .ok_or_else(|| {
                        FabricError::none(
                            FabricErrorCode::InvalidPayload,
                            "team_message_deliver lacks server-frozen message_reference",
                        )
                    })?,
            )
            .map_err(|error| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    format!("team_message_deliver payload is not a MessageReference: {error}"),
                )
            })?
        }
        _ => {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "operation is not a canonical Message or team_message_deliver envelope",
            ))
        }
    };
    decode_embedded_message(operation, &reference)
}

fn decode_embedded_message(
    operation: &RoutedOperation,
    reference: &MessageReference,
) -> Result<Message, FabricError> {
    let envelope = reference
        .canonical_message_envelope
        .as_ref()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                "content-addressed Message references must be resolved before target apply",
            )
        })?;
    let message: Message = serde_json::from_value(envelope.clone()).map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("canonical Message envelope is invalid: {error}"),
        )
    })?;
    let expected_fingerprint = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "sender_actor_ref": message.sender_actor_ref,
        "sender_agent_id": message.sender_agent_id,
        "sender_session_id": message.sender_session_id,
        "address_kind": message.address_kind,
        "target_ref": message.target_ref,
        "recipients": message.recipients,
        "team_id": message.team_id,
        "team_run_id": message.team_run_id,
        "work_id": message.work_id,
        "collaboration_scope": message.collaboration_scope,
        "kind": message.kind,
        "body": message.body,
        "body_digest": message.body_digest,
        "correlation_id": message.correlation_id,
        "causation_id": message.causation_id,
        "response_intent": message.response_intent,
        "evidence_refs": message.evidence_refs,
        "schema_version": message.schema_version,
        "idempotency_key": message.idempotency_key,
    }));
    if message.id != reference.message_id
        || message.body_digest != reference.body_digest
        || message.body_digest
            != format!(
                "sha256:{}",
                harness_fabric::sha256_hex(message.body.as_bytes())
            )
        || message.content_fingerprint != expected_fingerprint
        || operation.source_execution_space_id.as_deref()
            != Some(message.source_execution_space_id.as_str())
        || operation.source_node_id.as_deref() != Some(message.source_node_id.as_str())
        || operation.source_node_daemon_id.as_deref()
            != Some(message.source_node_daemon_id.as_str())
        || operation.source_node_daemon_generation != Some(message.source_authority_generation)
        || operation.target_node_id == message.source_node_id
    {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "Message envelope disagrees with immutable route/source authority",
        ));
    }
    Ok(message)
}

#[cfg_attr(not(test), allow(dead_code))]
fn store_error(error: harness_store::StoreError) -> FabricError {
    FabricError::none(
        FabricErrorCode::StoreUnavailable,
        format!("Wave4C authority Store failed: {error}"),
    )
}

fn runtime_source_authority_matches(operation: &RoutedOperation) -> bool {
    operation.source_authority == harness_fabric::OperationSourceAuthority::Node
        && operation.actor.actor_kind == FabricActorKind::Service
        && operation.source_node_id.as_deref() == Some(operation.actor.actor_id.as_str())
        && operation.source_node_daemon_id.is_some()
        && operation.source_node_daemon_generation.is_some()
}
