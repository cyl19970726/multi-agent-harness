#![allow(clippy::result_large_err)]

#[path = "../src/remote_fabric.rs"]
mod remote_fabric;

use harness_core::agentfirm_api::{
    ActorKind as RuntimeActorKind, ActorRef, ControlCommandEnvelope, RuntimeCommandKind,
};
use harness_fabric::{
    json_digest, ActorKind, AuthenticatedActor, OperationPriority, RoutedOperation,
    RUNTIME_COMMAND_REFERENCE_KIND, RUNTIME_COMMAND_REFERENCE_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn envelope() -> ControlCommandEnvelope {
    let payload = json!({"session_id":"session-b", "session_generation":3});
    ControlCommandEnvelope {
        id: "runtime-command:remote-1".into(),
        execution_space_id: "space-b".into(),
        target_node_id: "node-b".into(),
        target_node_daemon_id: "node-daemon:node-b".into(),
        target_node_daemon_generation: 7,
        authenticated_actor: ActorRef {
            kind: RuntimeActorKind::AgentMember,
            id: "agent-a".into(),
        },
        command: RuntimeCommandKind::ResumeSession,
        required_capability: "agent_session.resume".into(),
        idempotency_key: "remote-runtime-1".into(),
        expected_version: 4,
        expires_unix_ms: 90_000,
        payload_fingerprint: harness_store::canonical_json_fingerprint(&payload),
        payload,
        issued_at: "unix-ms:100".into(),
    }
}

fn operation(envelope: &ControlCommandEnvelope) -> RoutedOperation {
    let command_fingerprint =
        harness_store::runtime_command_envelope_fingerprint(envelope).expect("fingerprint");
    let body = json!({
        "runtime_command_id": envelope.id,
        "command_fingerprint": command_fingerprint,
        "target_execution_space_id": envelope.execution_space_id,
        "target_node_daemon_id": envelope.target_node_daemon_id,
        "target_node_daemon_generation": envelope.target_node_daemon_generation,
    });
    RoutedOperation {
        id: "operation-runtime-1".into(),
        company_id: "company-a".into(),
        kind: RUNTIME_COMMAND_REFERENCE_KIND.into(),
        source_authority: harness_fabric::OperationSourceAuthority::Node,
        source_node_id: Some("node-a".into()),
        target_node_id: envelope.target_node_id.clone(),
        source_gateway_generation: Some(2),
        source_node_daemon_id: Some("node-daemon:node-a".into()),
        source_node_daemon_generation: Some(2),
        control_plane_generation: 3,
        source_execution_space_id: Some("space-a".into()),
        target_execution_space_id: Some(envelope.execution_space_id.clone()),
        actor: AuthenticatedActor {
            company_id: "company-a".into(),
            actor_id: envelope.authenticated_actor.id.clone(),
            actor_kind: ActorKind::AgentMember,
            role_bindings: BTreeSet::from(["fabric_submit".into()]),
            session_id: "session-agent-a".into(),
            issued_at_unix_ms: 10,
            expires_at_unix_ms: 100_000,
        },
        actor_runtime_generation: Some(2),
        authorization_context: BTreeMap::from([("capability".into(), "remote-runtime".into())]),
        idempotency_key: envelope.idempotency_key.clone(),
        ordering_key: "runtime:session-b".into(),
        correlation_id: "correlation-1".into(),
        causation_id: None,
        expected_target_revision: Some(envelope.expected_version),
        body_schema: RUNTIME_COMMAND_REFERENCE_SCHEMA.into(),
        body_digest: json_digest(&body).expect("body digest"),
        body,
        priority: OperationPriority::Control,
        created_at_unix_ms: 100,
        expires_at_unix_ms: envelope.expires_unix_ms,
        protocol_version: 1,
        schema_version: "agentfirm.remote_fabric.v1".into(),
        canonicalization_version: harness_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
    }
}

#[test]
fn exact_runtime_reference_resolves_to_wave4c_node_daemon_contract() {
    let envelope = envelope();
    let operation = operation(&envelope);
    remote_fabric::validate_resolved_runtime_command(&operation, &envelope)
        .expect("exact immutable reference");
}

#[test]
fn hostile_runtime_resolution_fails_before_node_daemon_effect() {
    let envelope = envelope();
    let operation = operation(&envelope);
    for hostile in [
        {
            let mut value = envelope.clone();
            value.authenticated_actor.id = "sibling-agent".into();
            value
        },
        {
            let mut value = envelope.clone();
            value.target_node_daemon_generation += 1;
            value
        },
        {
            let mut value = envelope.clone();
            value.payload["session_id"] = json!("session-other");
            value.payload_fingerprint = harness_store::canonical_json_fingerprint(&value.payload);
            value
        },
    ] {
        assert!(remote_fabric::validate_resolved_runtime_command(&operation, &hostile).is_err());
    }
}
