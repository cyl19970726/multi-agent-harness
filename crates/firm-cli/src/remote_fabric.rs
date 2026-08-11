//! Narrow adapter between Remote Fabric references and the Wave 4C runtime.
//!
//! Fabric never constructs a RuntimeCommand or Message. It transports a
//! closed immutable reference; this adapter resolves the canonical command,
//! re-verifies every authority/fingerprint field, then calls the existing
//! NodeDaemon socket. A missing resolver or mismatch fails before the socket.

#![allow(clippy::result_large_err)]

use harness_core::agentfirm_api::{ActorKind as RuntimeActorKind, ControlCommandEnvelope};
use harness_fabric::{
    ActorKind as FabricActorKind, ClosedOperationBody, FabricError, FabricErrorCode,
    RoutedOperation, RuntimeCommandReference,
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

pub(crate) fn validate_resolved_runtime_command(
    operation: &RoutedOperation,
    envelope: &ControlCommandEnvelope,
) -> Result<(), FabricError> {
    let reference = runtime_reference_from_operation(operation)?;
    let fingerprint =
        harness_store::runtime_command_envelope_fingerprint(envelope).map_err(|error| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("resolved RuntimeCommand cannot be fingerprinted: {error}"),
            )
        })?;
    if reference.runtime_command_id != envelope.id
        || reference.command_fingerprint != fingerprint
        || reference.target_execution_space_id != envelope.execution_space_id
        || reference.target_node_daemon_id != envelope.target_node_daemon_id
        || reference.target_node_daemon_generation != envelope.target_node_daemon_generation
        || operation.target_node_id != envelope.target_node_id
        || operation.target_execution_space_id.as_deref()
            != Some(envelope.execution_space_id.as_str())
        || operation.expected_target_revision != Some(envelope.expected_version)
        || operation.expires_at_unix_ms != envelope.expires_unix_ms
        || operation.idempotency_key != envelope.idempotency_key
        || !actor_matches(operation, envelope)
    {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "resolved RuntimeCommand disagrees with its immutable routed authority/fingerprint",
        ));
    }
    Ok(())
}

#[cfg(not(test))]
#[allow(dead_code)]
pub(crate) fn dispatch_resolved_runtime_command(
    firm_home: &std::path::Path,
    operation: &RoutedOperation,
    envelope: &ControlCommandEnvelope,
) -> Result<serde_json::Value, FabricError> {
    validate_resolved_runtime_command(operation, envelope)?;
    crate::supervisor_daemon::runtime_command_via_socket(
        firm_home,
        &envelope.target_node_id,
        envelope,
    )
    .map_err(|error| {
        let mut failure = FabricError::unknown(
            operation.id.clone(),
            format!("NodeDaemon transport ended without a provable RuntimeCommand result: {error}"),
        );
        failure.details.insert(
            "reconciliation".into(),
            "resolve canonical RuntimeCommand record before retry".into(),
        );
        failure
    })
}

fn actor_matches(operation: &RoutedOperation, envelope: &ControlCommandEnvelope) -> bool {
    if operation.actor.actor_id != envelope.authenticated_actor.id {
        return false;
    }
    matches!(
        (
            operation.actor.actor_kind,
            envelope.authenticated_actor.kind
        ),
        (FabricActorKind::AgentMember, RuntimeActorKind::AgentMember)
            | (FabricActorKind::Service, RuntimeActorKind::Service)
            | (FabricActorKind::Human, RuntimeActorKind::Human)
    )
}
