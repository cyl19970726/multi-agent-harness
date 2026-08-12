#![allow(clippy::result_large_err)]

#[path = "../src/remote_fabric.rs"]
mod remote_fabric;

use harness_core::agentfirm_api::{
    ActorKind as RuntimeActorKind, ActorRef, ControlCommandEnvelope, RuntimeCommandKind,
};
use harness_core::{ExecutionNode, ExecutionNodeStatus};
use harness_fabric::{
    json_digest, ActorKind, AuthenticatedActor, CompanyNode, NodeAdministrativeStatus,
    OperationPriority, RoutedOperation, RuntimeCommandIntent, RuntimeCommandReference,
    RUNTIME_COMMAND_REFERENCE_KIND, RUNTIME_COMMAND_REFERENCE_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn envelope() -> ControlCommandEnvelope {
    let payload = json!({"session_id":"session-b", "session_generation":3});
    ControlCommandEnvelope {
        id: "runtime-command:remote-1".into(),
        execution_space_id: "space-b".into(),
        target_node_id: "22222222-2222-4222-8222-222222222222".into(),
        target_node_daemon_id: "node-daemon:node-b".into(),
        target_node_daemon_generation: 7,
        authenticated_actor: ActorRef {
            kind: RuntimeActorKind::Service,
            id: "node-daemon:node-b".into(),
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
    let intent = RuntimeCommandIntent {
        id: envelope.id.clone(),
        target_execution_space_id: envelope.execution_space_id.clone(),
        command: serde_json::to_value(envelope.command)
            .expect("command value")
            .as_str()
            .expect("command string")
            .into(),
        idempotency_key: envelope.idempotency_key.clone(),
        expected_version: envelope.expected_version,
        expires_unix_ms: envelope.expires_unix_ms,
        payload: envelope.payload.clone(),
        payload_fingerprint: envelope.payload_fingerprint.clone(),
        issued_at: envelope.issued_at.clone(),
    };
    let reference = RuntimeCommandReference {
        runtime_command_id: intent.id.clone(),
        intent_fingerprint: format!("sha256:{}", json_digest(&intent).expect("intent digest")),
        target_execution_space_id: intent.target_execution_space_id.clone(),
        canonical_command_intent: intent,
    };
    let body = serde_json::to_value(reference).expect("runtime intent reference");
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
            actor_id: "node-a".into(),
            actor_kind: ActorKind::Service,
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
    remote_fabric::validate_resolved_runtime_command(
        &operation,
        &envelope,
        &envelope.target_node_id,
        &envelope.target_node_daemon_id,
        envelope.target_node_daemon_generation,
    )
    .expect("exact immutable reference");
    assert_eq!(
        remote_fabric::resolved_runtime_command_from_operation(
            &operation,
            &envelope.target_node_id,
            &envelope.target_node_daemon_id,
            envelope.target_node_daemon_generation,
        )
        .expect("target resolves exact immutable intent under its own authority"),
        envelope
    );
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
        assert!(remote_fabric::validate_resolved_runtime_command(
            &operation,
            &hostile,
            &envelope.target_node_id,
            &envelope.target_node_daemon_id,
            envelope.target_node_daemon_generation,
        )
        .is_err());
    }
    let before = operation.clone();
    let mut rewritten_route = operation;
    rewritten_route.body["canonical_command_intent"]["payload"]["session_id"] =
        json!("session-other");
    rewritten_route.body_digest =
        harness_fabric::json_digest(&rewritten_route.body).expect("hostile body digest");
    assert_eq!(
        remote_fabric::resolved_runtime_command_from_operation(
            &rewritten_route,
            &envelope.target_node_id,
            &envelope.target_node_daemon_id,
            envelope.target_node_daemon_generation,
        )
        .expect_err("embedded command cannot diverge from its frozen fingerprint")
        .code,
        harness_fabric::FabricErrorCode::SourceMismatch
    );
    let mut forged_actor = before;
    forged_actor.body["canonical_command_intent"]["authenticated_actor"] = json!({
        "kind": "service",
        "id": envelope.target_node_daemon_id,
    });
    forged_actor.body_digest = json_digest(&forged_actor.body).expect("hostile body digest");
    assert_eq!(
        forged_actor
            .closed_body()
            .expect_err("source-authored target Operator identity must fail closed")
            .code,
        harness_fabric::FabricErrorCode::InvalidPayload
    );
}

#[test]
fn company_node_is_the_wave4c_execution_node_and_gateway_is_daemon_child() {
    let root =
        std::env::temp_dir().join(format!("agentfirm-node-authority-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove prior isolated test root");
    }
    std::fs::create_dir(&root).expect("create isolated Store root");
    let store = harness_store::HarnessStore::new(&root);
    let node_id = "11111111-1111-4111-8111-111111111111";
    store
        .insert_execution_node(&ExecutionNode {
            id: node_id.into(),
            display_name: "Node A".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert ExecutionNode");
    let lease = store
        .acquire_node_daemon_lease(node_id, "daemon-a", "instance-a", 10, 30_000)
        .expect("acquire NodeDaemon lease");
    let node = CompanyNode {
        id: node_id.into(),
        company_id: "company-a".into(),
        display_name: "Node A".into(),
        public_key_fingerprint: "fingerprint".into(),
        certificate_serial: "cert-a".into(),
        allowed_capabilities: BTreeSet::from(["durable-routing".into()]),
        administrative_status: NodeAdministrativeStatus::Active,
        node_revision: 1,
        enrolled_at_unix_ms: 1,
        last_seen_at_unix_ms: None,
        revoked_at_unix_ms: None,
        revoke_reason: None,
        protocol_min: 1,
        protocol_max: 1,
        schema_bundle_digest: "schema".into(),
        schema_version: "agentfirm.remote_fabric.v1".into(),
        created_at_unix_ms: 1,
        updated_at_unix_ms: 1,
    };
    remote_fabric::validate_wave4c_node_authority(&store, &node, &lease, 11)
        .expect("one exact machine and daemon authority");

    let before = std::fs::read(root.join("node_daemon_leases.jsonl")).expect("lease bytes");
    let mut wrong_node = node;
    wrong_node.id = "22222222-2222-4222-8222-222222222222".into();
    assert_eq!(
        remote_fabric::validate_wave4c_node_authority(&store, &wrong_node, &lease, 11)
            .expect_err("Fabric cannot invent a second machine identity")
            .code,
        harness_fabric::FabricErrorCode::SourceMismatch
    );
    assert_eq!(
        std::fs::read(root.join("node_daemon_leases.jsonl")).expect("lease bytes"),
        before
    );
    std::fs::remove_dir_all(root).expect("remove isolated Store root");
}

#[test]
fn runtime_route_cannot_be_reinterpreted_as_a_message() {
    let envelope = envelope();
    assert_eq!(
        remote_fabric::resolved_message_from_operation(&operation(&envelope))
            .expect_err("closed registry prevents route kind confusion")
            .code,
        harness_fabric::FabricErrorCode::InvalidPayload
    );
}
