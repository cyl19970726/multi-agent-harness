//! AgentFirm cross-machine collaboration v1 contracts.
//!
//! These records describe Company-level relationships and immutable remote
//! facts. They deliberately do not own Team Work, provider runtime, transport
//! receipts, or Message authorship. Wave 5 supplies the routed fabric; Wave 4C
//! remains the sole Message and per-recipient delivery authority.

use crate::agentfirm_api::{ActorRef, CanonicalMessageDeliveryStatus};
use serde::{Deserialize, Serialize};

pub const COLLABORATION_STORE_VERSION: &str = "agentfirm.collaboration.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlacementRef {
    pub team_id: String,
    pub team_revision: u64,
    pub node_id: String,
    pub placement_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteWorkRef {
    pub schema_version: String,
    pub execution_space_id: String,
    pub node_id: String,
    pub team_id: String,
    pub team_revision: u64,
    pub placement_generation: u64,
    pub work_id: String,
    pub work_revision: u64,
    pub work_event_id: String,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationState {
    Proposed,
    AwaitingTargetDecision,
    ProvisioningTargetWork,
    Active,
    ResultAvailable,
    CancellationRequested,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationTerminalOutcome {
    Completed,
    Rejected,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationInboundMode {
    HostApprovalRequired,
    AutoAccept,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationInboundPolicy {
    pub id: String,
    pub company_id: String,
    pub target_team_id: String,
    pub source_team_id: String,
    pub mode: DelegationInboundMode,
    pub allowed_outcome_classes: Vec<String>,
    pub max_active_delegations: u64,
    pub created_by_target_host: ActorRef,
    pub revision: u64,
    pub created_at: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationInboundPolicySnapshot {
    pub policy_id: String,
    pub policy_revision: u64,
    pub policy_digest: String,
    pub mode: DelegationInboundMode,
    pub allowed_outcome_classes: Vec<String>,
    pub max_active_delegations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDelegationV1 {
    pub id: String,
    pub company_id: String,
    pub source_work_ref: RemoteWorkRef,
    pub source_owner_ref: ActorRef,
    pub source_team_id: String,
    pub source_node_id: String,
    pub target_placement: TargetPlacementRef,
    pub requested_outcome: String,
    pub outcome_class: String,
    pub acceptance_contract: String,
    pub inbound_policy_snapshot: DelegationInboundPolicySnapshot,
    #[serde(default)]
    pub target_work_ref: Option<RemoteWorkRef>,
    pub state: DelegationState,
    #[serde(default)]
    pub terminal_outcome: Option<DelegationTerminalOutcome>,
    pub revision: u64,
    pub operation_id: String,
    pub idempotency_key: String,
    pub created_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationDecisionKind {
    Accept,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationDecision {
    pub id: String,
    pub delegation_id: String,
    pub expected_delegation_revision: u64,
    pub decision: DelegationDecisionKind,
    pub decided_by_target_host: ActorRef,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOperationalDecisionRef {
    pub decision_id: String,
    pub work_ref: RemoteWorkRef,
    pub decision_revision: u64,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationRequestState {
    Pending,
    Accepted,
    Rejected,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationCancellationRequest {
    pub id: String,
    pub delegation_id: String,
    pub expected_delegation_revision: u64,
    pub requested_by: ActorRef,
    pub reason: String,
    pub state: CancellationRequestState,
    #[serde(default)]
    pub target_host_decision_ref: Option<String>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationDecisionKind {
    Accept,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationCancellationDecision {
    pub id: String,
    pub cancellation_request_id: String,
    pub expected_request_revision: u64,
    pub decision: CancellationDecisionKind,
    pub decided_by_target_host: ActorRef,
    pub native_work_event_ref: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteFactKind {
    Report,
    Finding,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteFactSnapshot {
    pub publication_id: String,
    pub fact_schema: String,
    pub canonical_redacted_fact: serde_json::Value,
    pub canonical_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteFactPublication {
    pub id: String,
    pub company_id: String,
    pub delegation_id: String,
    pub origin_node_id: String,
    pub origin_team_id: String,
    pub fact_work_ref: RemoteWorkRef,
    pub delegation_source_work_ref: RemoteWorkRef,
    pub fact_kind: RemoteFactKind,
    pub fact_id: String,
    pub fact_revision: u64,
    pub fact_digest: String,
    pub summary: String,
    pub classification: String,
    pub snapshot: RemoteFactSnapshot,
    pub artifact_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub operational_decision_ref: Option<WorkOperationalDecisionRef>,
    pub created_by: ActorRef,
    pub created_at: String,
    pub retain_until: String,
}

/// Read-only Company projection of one target-owned canonical delivery.
/// It may never be folded back into target delivery authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossNodeDeliveryProjection {
    pub delivery_id: String,
    pub message_id: String,
    pub recipient_actor_ref: ActorRef,
    #[serde(default)]
    pub recipient_session_id: Option<String>,
    #[serde(default)]
    pub recipient_runtime_generation: Option<u64>,
    pub target_node_id: String,
    #[serde(default)]
    pub target_gateway_generation: Option<u64>,
    pub routed_operation_id: String,
    pub state: CanonicalMessageDeliveryStatus,
    pub attempt_refs: Vec<String>,
    pub receipt_refs: Vec<String>,
    pub target_observed_sequence: u64,
    pub observed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutedBusinessKind {
    DelegationPropose,
    DelegationDecide,
    TargetWorkCreate,
    DelegationCancelRequest,
    DelegationCancelDecide,
    TeamMessageDeliver,
    RemoteFactPublish,
    ArtifactGrant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedBusinessDescriptor {
    pub kind: RoutedBusinessKind,
    pub request_schema: String,
    pub result_schema: String,
    pub target_application_service: String,
    pub requires_expected_revision: bool,
    pub idempotency_components: Vec<String>,
    pub required_capability: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricEffectCertainty {
    None,
    NotApplied,
    Applied,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricErrorCode {
    UnauthorizedActor,
    IdempotencyConflict,
    RevisionConflict,
    DelegationTerminal,
    DelegationPolicyRejected,
    TargetTeamPlacementChanged,
    TargetTeamUnavailable,
    TargetWorkCreateFailed,
    CancellationDecisionRequired,
    MessageRecipientUnauthorized,
    MessageExpired,
    PublicationDigestMismatch,
    PublicationScopeMismatch,
    ArtifactScopeUnauthorized,
    ProtocolMismatch,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FabricError {
    pub code: FabricErrorCode,
    pub message: String,
    pub retryable: bool,
    pub effect_certainty: FabricEffectCertainty,
    pub resource_kind: String,
    pub resource_id: String,
    #[serde(default)]
    pub current_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedBusinessOperation {
    pub id: String,
    pub protocol_version: String,
    pub company_id: String,
    pub kind: RoutedBusinessKind,
    pub authenticated_actor: ActorRef,
    pub source_node_id: String,
    pub target_placement: TargetPlacementRef,
    pub expected_revision: u64,
    pub idempotency_key: String,
    pub payload: serde_json::Value,
    pub payload_digest: String,
    pub required_capability: String,
    pub ordering_key: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedBusinessReceipt {
    pub operation_id: String,
    pub kind: RoutedBusinessKind,
    pub target_node_id: String,
    pub target_placement_generation: u64,
    pub effect_certainty: FabricEffectCertainty,
    pub result: serde_json::Value,
    pub result_digest: String,
    pub applied_at: String,
}

pub fn collaboration_business_registry_v1() -> Vec<RoutedBusinessDescriptor> {
    use RoutedBusinessKind::*;
    [
        (
            DelegationPropose,
            "work-delegation-v1",
            "work-delegation-v1",
            "control-plane.collaboration",
        ),
        (
            DelegationDecide,
            "delegation-decision",
            "work-delegation-v1",
            "control-plane.collaboration",
        ),
        (
            TargetWorkCreate,
            "target-work-create",
            "remote-work-ref",
            "node.work",
        ),
        (
            DelegationCancelRequest,
            "delegation-cancellation-request",
            "work-delegation-v1",
            "control-plane.collaboration",
        ),
        (
            DelegationCancelDecide,
            "delegation-cancellation-decision",
            "work-delegation-v1",
            "node.work",
        ),
        (
            TeamMessageDeliver,
            "message",
            "canonical-message-delivery",
            "node.message",
        ),
        (
            RemoteFactPublish,
            "remote-fact-publication",
            "remote-fact-publication",
            "control-plane.collaboration",
        ),
        (
            ArtifactGrant,
            "remote-artifact-grant",
            "remote-artifact-manifest",
            "fabric.artifact",
        ),
    ]
    .into_iter()
    .map(
        |(kind, request_schema, result_schema, target_application_service)| {
            RoutedBusinessDescriptor {
                kind,
                request_schema: request_schema.into(),
                result_schema: result_schema.into(),
                target_application_service: target_application_service.into(),
                requires_expected_revision: true,
                idempotency_components: vec![
                    "company_id".into(),
                    "actor_session".into(),
                    "target_placement_generation".into(),
                    "expected_revision".into(),
                    "payload_digest".into(),
                ],
                required_capability: format!("collaboration.{kind:?}").to_ascii_lowercase(),
            }
        },
    )
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routed_business_registry_is_closed_and_complete() {
        let registry = collaboration_business_registry_v1();
        assert_eq!(registry.len(), 8);
        let kinds = registry
            .iter()
            .map(|entry| entry.kind)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(kinds.len(), 8);
        assert!(registry
            .iter()
            .all(|entry| entry.requires_expected_revision));
        assert!(registry
            .iter()
            .all(|entry| !entry.required_capability.is_empty()));
    }
}
