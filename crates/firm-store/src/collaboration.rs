use crate::{canonical_json_fingerprint, HarnessStore, StoreError, StoreResult};
use firm_core::agentfirm_api::{ActorKind, ActorRef, CanonicalMessageDelivery, Message};
use firm_core::collaboration::{
    ArtifactImport, CancellationDecisionKind, CancellationRequestState,
    CrossNodeDeliveryProjection, DelegationCancellationDecision, DelegationCancellationRequest,
    DelegationDecision, DelegationDecisionKind, DelegationInboundMode, DelegationInboundPolicy,
    DelegationInboundPolicySnapshot, DelegationState, DelegationTerminalOutcome,
    FabricEffectCertainty, FabricError, FabricErrorCode, ImmutableMessageTransferPayload,
    RemoteFactPublication, RemoteMessageReplica, RemoteMessageTransferState, RemoteWorkRef,
    RoutedBusinessKind, RoutedBusinessOperation, RoutedBusinessReceipt,
    SourceRemoteMessageTransfer, SourceWorkAttestation, TargetPlacementRef, WorkDelegationV1,
    COLLABORATION_STORE_VERSION,
};
use firm_fabric::{ArtifactCapability, ArtifactCapabilityPurpose, RemoteArtifactManifest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const COLLABORATION_OPERATIONS_LEDGER: &str = "agentfirm_collaboration_operations.jsonl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationOperation {
    pub store_version: String,
    pub company_id: String,
    pub command_name: String,
    pub authenticated_actor: ActorRef,
    pub idempotency_key: String,
    pub request_fingerprint: String,
    pub aggregate_kind: String,
    pub aggregate_id: String,
    pub store_sequence: u64,
    pub resulting_revision: u64,
    pub resulting_projection: Value,
    pub immutable_side_records: Vec<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationMutationContext {
    pub company_id: String,
    pub authenticated_actor: ActorRef,
    pub command_name: String,
    pub idempotency_key: String,
    pub expected_revision: u64,
    pub occurred_at: String,
}

/// Server-resolved authority facts. Public callers never construct this from
/// request headers or bodies; the application boundary resolves it from the
/// authenticated session and canonical Team/Work projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCollaborationAuthority {
    pub source_host: ActorRef,
    pub source_work_owner: ActorRef,
    pub target_host: ActorRef,
    pub target_placement: TargetPlacementRef,
    pub source_work_application_service: ActorRef,
    pub source_gateway_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeDelegationRequest {
    pub delegation_id: String,
    pub source_work_attestation_id: String,
    pub target_placement: TargetPlacementRef,
    pub requested_outcome: String,
    pub outcome_class: String,
    pub acceptance_contract: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollaborationMutationResult<T> {
    pub projection: T,
    pub operation: CollaborationOperation,
    pub replayed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationDelegationFilter {
    pub source_team_id: Option<String>,
    pub target_team_id: Option<String>,
    pub node_id: Option<String>,
    pub state: Option<DelegationState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationCursor {
    pub as_of_store_sequence: u64,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationPage<T> {
    pub items: Vec<T>,
    pub as_of_store_sequence: u64,
    pub next_cursor: Option<CollaborationCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationScopedCursor {
    pub company_id: String,
    pub actor_digest: String,
    pub filter_digest: String,
    pub as_of_store_sequence: u64,
    pub raw_offset: usize,
    pub visible_progress: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationScopedPage<T> {
    pub items: Vec<T>,
    pub as_of_store_sequence: u64,
    pub next_cursor: Option<CollaborationScopedCursor>,
}

pub trait CollaborationFabricPort {
    fn dispatch(
        &self,
        operation: &RoutedBusinessOperation,
    ) -> Result<RoutedBusinessReceipt, FabricError>;
}

/// Target-node persistence seam for a remote replica of the existing Wave 4C
/// Message. The route journal cannot create delivery authority; callers must
/// receive a successfully persisted replica before invoking canonical delivery.
pub trait RemoteMessageReplicaPort {
    fn fetch_message_object(&self, message_object_ref: &str) -> Result<Vec<u8>, FabricError>;

    fn persist_remote_replica(
        &self,
        replica: &RemoteMessageReplica,
    ) -> Result<RemoteMessageReplica, FabricError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessageReplicaExpectation {
    pub source_execution_space_id: String,
    pub message_id: String,
    pub schema_version: u64,
    pub content_fingerprint: String,
    pub body_digest: String,
    pub persisted_at: String,
}

fn direct_fabric_error(
    code: FabricErrorCode,
    message: impl Into<String>,
    resource_kind: &str,
    resource_id: &str,
) -> FabricError {
    FabricError {
        code,
        message: message.into(),
        retryable: false,
        effect_certainty: FabricEffectCertainty::None,
        resource_kind: resource_kind.into(),
        resource_id: resource_id.into(),
        current_revision: None,
    }
}

fn immutable_message_fingerprint(message: &Message) -> String {
    canonical_json_fingerprint(&serde_json::json!({
        "sender_actor_ref": message.sender_actor_ref,
        "sender_agent_member_id": message.sender_agent_member_id,
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
    }))
}

pub fn validate_message_collaboration_scope(message: &Message) -> StoreResult<()> {
    let Some(scope) = message.collaboration_scope.as_ref() else {
        return Ok(());
    };
    let source_work_valid = scope.source_work_ref.as_ref().is_none_or(|source| {
        source.execution_space_id == message.source_execution_space_id
            && source.node_id == message.source_node_id
            && source.team_id == scope.source_team_id
            && source.placement_generation == 1
            && message.work_id.as_ref() == Some(&source.work_id)
    });
    let target_work_valid = scope.target_work_ref.as_ref().is_none_or(|target| {
        target.team_id == scope.target_team_id && target.placement_generation == 1
    });
    if scope.source_team_id.trim().is_empty()
        || scope.target_team_id.trim().is_empty()
        || scope.source_team_id == scope.target_team_id
        || message.team_id.as_ref() != Some(&scope.source_team_id)
        || !source_work_valid
        || !target_work_valid
        || (scope.expected_delegation_revision.is_some() && scope.delegation_id.is_none())
    {
        return Err(collaboration_error(
            FabricErrorCode::MessageRecipientUnauthorized,
            "Message CollaborationScope is outside the exact source Team/Work and target Team authority",
            "message",
            &message.id,
            None,
        ));
    }
    Ok(())
}

/// Resolve, authenticate and durably persist one immutable Message replica on
/// the target Node. A digest-only transfer is impossible by construction.
pub fn persist_verified_remote_message_replica<P: RemoteMessageReplicaPort>(
    port: &P,
    payload: &ImmutableMessageTransferPayload,
    expected: &RemoteMessageReplicaExpectation,
) -> Result<RemoteMessageReplica, FabricError> {
    let bytes = match payload {
        ImmutableMessageTransferPayload::CanonicalBytes {
            canonical_message_bytes,
        } => canonical_message_bytes.clone(),
        ImmutableMessageTransferPayload::MessageObjectRef {
            message_object_ref,
            authenticated_content_digest,
        } => {
            let bytes = port.fetch_message_object(message_object_ref)?;
            let value = serde_json::from_slice::<Value>(&bytes).map_err(|_| {
                direct_fabric_error(
                    FabricErrorCode::MessageReplicaMismatch,
                    "message object is not a valid immutable Message payload",
                    "message_object_ref",
                    message_object_ref,
                )
            })?;
            if canonical_json_fingerprint(&value) != *authenticated_content_digest {
                return Err(direct_fabric_error(
                    FabricErrorCode::MessageReplicaMismatch,
                    "message object content-addressed digest does not match",
                    "message_object_ref",
                    message_object_ref,
                ));
            }
            bytes
        }
    };
    let message = serde_json::from_slice::<Message>(&bytes).map_err(|_| {
        direct_fabric_error(
            FabricErrorCode::MessageReplicaMismatch,
            "cross-node payload is not the canonical Wave 4C Message schema",
            "message",
            &expected.message_id,
        )
    })?;
    let body_digest = format!(
        "sha256:{}",
        firm_fabric::sha256_hex(message.body.as_bytes())
    );
    if message.source_execution_space_id != expected.source_execution_space_id
        || message.id != expected.message_id
        || message.schema_version != expected.schema_version
        || message.content_fingerprint != expected.content_fingerprint
        || message.body_digest != expected.body_digest
        || message.content_fingerprint != immutable_message_fingerprint(&message)
        || message.body_digest != body_digest
    {
        return Err(direct_fabric_error(
            FabricErrorCode::MessageReplicaMismatch,
            "immutable Message identity, schema, body digest, or content fingerprint changed in transit",
            "message",
            &expected.message_id,
        ));
    }
    let replica = RemoteMessageReplica {
        source_execution_space_id: message.source_execution_space_id,
        message_id: message.id,
        schema_version: message.schema_version,
        content_fingerprint: message.content_fingerprint,
        body_digest: message.body_digest,
        canonical_message_bytes: bytes,
        persisted_at: expected.persisted_at.clone(),
    };
    port.persist_remote_replica(&replica)
}

/// Build the non-authoritative source outbox row allowed while the Control
/// Plane is offline. The Message already exists; this function cannot create a
/// Delegation, Decision, publication, or replacement Message identity.
pub fn queue_remote_message_transfer(
    message: &Message,
    target_placement: &TargetPlacementRef,
    payload: ImmutableMessageTransferPayload,
    queued_at: &str,
) -> Result<SourceRemoteMessageTransfer, FabricError> {
    let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
        direct_fabric_error(
            FabricErrorCode::MessageRecipientUnauthorized,
            "cross-node transfer requires the immutable Message CollaborationScope",
            "message",
            &message.id,
        )
    })?;
    if target_placement.placement_generation != 1
        || scope.target_team_id != target_placement.team_id
        || scope.source_team_id == scope.target_team_id
        || message.source_node_id == target_placement.node_id
        || message.content_fingerprint != immutable_message_fingerprint(message)
    {
        return Err(direct_fabric_error(
            FabricErrorCode::TargetTeamPlacementChanged,
            "remote Message transfer does not match immutable v1 Team placement and authored scope",
            "message",
            &message.id,
        ));
    }
    Ok(SourceRemoteMessageTransfer {
        id: format!("remote-message-transfer:{}", message.id),
        source_execution_space_id: message.source_execution_space_id.clone(),
        source_node_id: message.source_node_id.clone(),
        source_node_daemon_generation: message.source_authority_generation,
        message_id: message.id.clone(),
        message_schema_version: message.schema_version,
        content_fingerprint: message.content_fingerprint.clone(),
        body_digest: message.body_digest.clone(),
        target_placement: target_placement.clone(),
        payload,
        state: RemoteMessageTransferState::QueuedForControlPlane,
        queued_at: queued_at.into(),
    })
}

/// Transport-neutral business orchestration. The fabric may route/reconcile
/// the effect, but only this service folds an applied receipt into the central
/// Delegation relationship. Unknown/failed effects never fabricate Work or
/// Delegation outcomes.
pub struct CollaborationApplicationService<'a, F> {
    store: &'a HarnessStore,
    fabric: &'a F,
    control_plane_actor: &'a ActorRef,
}

impl<'a, F: CollaborationFabricPort> CollaborationApplicationService<'a, F> {
    pub fn new(store: &'a HarnessStore, fabric: &'a F, control_plane_actor: &'a ActorRef) -> Self {
        Self {
            store,
            fabric,
            control_plane_actor,
        }
    }

    pub fn provision_target_work(
        &self,
        fold_context: &CollaborationMutationContext,
        delegation_id: &str,
        observed_target_placement: &TargetPlacementRef,
    ) -> StoreResult<CollaborationMutationResult<WorkDelegationV1>> {
        let routed = self.store.target_work_create_operation(
            &fold_context.company_id,
            delegation_id,
            &fold_context.occurred_at,
        )?;
        let receipt = self.fabric.dispatch(&routed).map_err(|error| {
            StoreError::Conflict(
                serde_json::to_string(&error)
                    .unwrap_or_else(|_| "routed collaboration operation failed".into()),
            )
        })?;
        if receipt.operation_id != routed.id
            || receipt.kind != routed.kind
            || receipt.target_node_id != routed.target_placement.node_id
            || receipt.target_placement_generation != routed.target_placement.placement_generation
            || receipt.effect_certainty != FabricEffectCertainty::Applied
            || receipt.result_digest != canonical_json_fingerprint(&receipt.result)
        {
            return Err(collaboration_error(
                FabricErrorCode::RecoveryRequired,
                "fabric receipt is unknown, forged, or outside the exact routed operation",
                "routed_operation",
                &routed.id,
                Some(routed.expected_revision),
            ));
        }
        let target_work_ref = serde_json::from_value::<RemoteWorkRef>(
            receipt
                .result
                .get("target_work_ref")
                .cloned()
                .ok_or_else(|| {
                    collaboration_error(
                        FabricErrorCode::TargetWorkCreateFailed,
                        "applied target Work receipt lacks target_work_ref",
                        "routed_operation",
                        &routed.id,
                        Some(routed.expected_revision),
                    )
                })?,
        )?;
        self.store.apply_target_work_created(
            fold_context,
            delegation_id,
            &target_work_ref,
            observed_target_placement,
            &receipt.operation_id,
            self.control_plane_actor,
        )
    }
}

fn collaboration_error(
    code: FabricErrorCode,
    message: impl Into<String>,
    resource_kind: &str,
    resource_id: &str,
    current_revision: Option<u64>,
) -> StoreError {
    StoreError::Conflict(
        serde_json::to_string(&FabricError {
            code,
            message: message.into(),
            retryable: false,
            effect_certainty: FabricEffectCertainty::None,
            resource_kind: resource_kind.into(),
            resource_id: resource_id.into(),
            current_revision,
        })
        .unwrap_or_else(|_| "collaboration mutation rejected".into()),
    )
}

fn require_non_empty(value: &str, field: &str) -> StoreResult<()> {
    if value.trim().is_empty() {
        return Err(collaboration_error(
            FabricErrorCode::ProtocolMismatch,
            format!("{field} must not be empty"),
            "request",
            field,
            None,
        ));
    }
    Ok(())
}

fn policy_snapshot(
    policy: &DelegationInboundPolicy,
) -> StoreResult<DelegationInboundPolicySnapshot> {
    let value = serde_json::json!({
        "policy_id": policy.id,
        "policy_revision": policy.revision,
        "mode": policy.mode,
        "allowed_outcome_classes": policy.allowed_outcome_classes,
        "max_active_delegations": policy.max_active_delegations,
    });
    Ok(DelegationInboundPolicySnapshot {
        policy_id: policy.id.clone(),
        policy_revision: policy.revision,
        policy_digest: canonical_json_fingerprint(&value),
        mode: policy.mode,
        allowed_outcome_classes: policy.allowed_outcome_classes.clone(),
        max_active_delegations: policy.max_active_delegations,
    })
}

fn source_work_attestation_digest(attestation: &SourceWorkAttestation) -> StoreResult<String> {
    Ok(canonical_json_fingerprint(&serde_json::json!({
        "id": attestation.id,
        "company_id": attestation.company_id,
        "source_work_ref": attestation.source_work_ref,
        "source_owner_ref": attestation.source_owner_ref,
        "source_host_ref": attestation.source_host_ref,
        "work_application_service_ref": attestation.work_application_service_ref,
        "source_gateway_generation": attestation.source_gateway_generation,
        "issued_at": attestation.issued_at,
    })))
}

fn exact_actor(actual: &ActorRef, expected: &ActorRef) -> bool {
    actual == expected && !actual.id.trim().is_empty()
}

mod application;
mod delegation_mutations;
mod operation_builders;
mod policy_attestation;
mod read_models;

pub use application::project_cross_node_deliveries;
