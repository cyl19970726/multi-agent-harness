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
    #[serde(deserialize_with = "deserialize_placement_generation_v1")]
    pub placement_generation: u64,
}

fn deserialize_placement_generation_v1<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let generation = u64::deserialize(deserializer)?;
    if generation == 1 {
        Ok(generation)
    } else {
        Err(serde::de::Error::custom(
            "Wave 6 v1 placement_generation must equal 1",
        ))
    }
}

/// Optional cross-Team context carried by the existing Wave 4C immutable
/// Message. This is metadata, not a second Message or delivery authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationScope {
    pub source_team_id: String,
    pub target_team_id: String,
    #[serde(default)]
    pub delegation_id: Option<String>,
    #[serde(default)]
    pub expected_delegation_revision: Option<u64>,
    #[serde(default)]
    pub source_work_ref: Option<RemoteWorkRef>,
    #[serde(default)]
    pub target_work_ref: Option<RemoteWorkRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteWorkRef {
    pub schema_version: String,
    pub execution_space_id: String,
    pub node_id: String,
    pub team_id: String,
    pub team_revision: u64,
    #[serde(deserialize_with = "deserialize_placement_generation_v1")]
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

/// Work authority resolved and signed into the collaboration Store by the
/// source Node WorkApplicationService. Public REST/MCP callers never author
/// Work or owner fields on a delegation proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceWorkAttestation {
    pub id: String,
    pub company_id: String,
    pub source_work_ref: RemoteWorkRef,
    #[serde(deserialize_with = "deserialize_person_actor_ref")]
    pub source_owner_ref: ActorRef,
    #[serde(deserialize_with = "deserialize_person_actor_ref")]
    pub source_host_ref: ActorRef,
    #[serde(deserialize_with = "deserialize_service_actor_ref")]
    pub work_application_service_ref: ActorRef,
    pub source_gateway_generation: u64,
    pub attestation_digest: String,
    pub issued_at: String,
}

fn deserialize_person_actor_ref<'de, D>(deserializer: D) -> Result<ActorRef, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let actor = ActorRef::deserialize(deserializer)?;
    if matches!(
        actor.kind,
        crate::agentfirm_api::ActorKind::Human | crate::agentfirm_api::ActorKind::AgentMember
    ) && !actor.id.trim().is_empty()
    {
        Ok(actor)
    } else {
        Err(serde::de::Error::custom(
            "source Work owner/Host must be a non-empty Human or AgentMember ActorRef",
        ))
    }
}

fn deserialize_service_actor_ref<'de, D>(deserializer: D) -> Result<ActorRef, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let actor = ActorRef::deserialize(deserializer)?;
    if actor.kind == crate::agentfirm_api::ActorKind::Service && !actor.id.trim().is_empty() {
        Ok(actor)
    } else {
        Err(serde::de::Error::custom(
            "source Work attestation author must be a non-empty Service ActorRef",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDelegationV1 {
    pub id: String,
    pub company_id: String,
    pub source_work_attestation_id: String,
    pub source_work_ref: RemoteWorkRef,
    pub source_owner_ref: ActorRef,
    pub source_team_id: String,
    pub source_node_id: String,
    pub target_placement: TargetPlacementRef,
    pub target_host_ref: ActorRef,
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

/// Frozen, digest-bound authority carried with a cross-node Message route.
/// The source server derives it from the current central Delegation; callers
/// cannot construct or widen it. The target revalidates this snapshot against
/// the immutable Message and its own canonical Team/Work placement before any
/// Message or Delivery mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationMessageAuthority {
    pub company_id: String,
    pub delegation_id: String,
    pub delegation_revision: u64,
    pub source_work_ref: RemoteWorkRef,
    pub target_work_ref: RemoteWorkRef,
    pub target_placement: TargetPlacementRef,
    pub source_owner_ref: ActorRef,
    pub source_host_ref: ActorRef,
    pub target_host_ref: ActorRef,
    pub inbound_policy_snapshot: DelegationInboundPolicySnapshot,
    pub authority_digest: String,
}

/// Frozen source-admission authority for an ordinary peer-Team Message.
///
/// This authority proves that one exact active TeamMembership and one exact
/// local AgentSession/NodeDaemon generation authored the Message. It does not
/// convey Work ownership and cannot authorize a WorkDelegation effect.
///
/// The default target is the peer Team's shared inbox (`team-inbox:` Team
/// subscription). When the three `target_membership_*`/`target_agent_member_id`
/// fields are present, the authority instead targets one exact peer
/// TeamMembership through its durable direct subscription, and the single
/// created delivery is already bound to that TeamMembership/AgentMember; all
/// three fields are set or all are absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerTeamMessageAdmissionAuthority {
    pub company_id: String,
    pub source_execution_space_id: String,
    pub source_team_id: String,
    pub source_team_revision: u64,
    pub source_membership_id: String,
    pub source_membership_generation: u64,
    pub source_agent_member_id: String,
    pub source_session_id: String,
    pub source_session_generation: u64,
    pub source_node_id: String,
    pub source_node_daemon_id: String,
    pub source_node_daemon_generation: u64,
    pub target_execution_space_id: String,
    pub target_team_id: String,
    pub target_team_revision: u64,
    pub target_node_id: String,
    #[serde(default)]
    pub target_membership_id: Option<String>,
    #[serde(default)]
    pub target_membership_generation: Option<u64>,
    #[serde(default)]
    pub target_agent_member_id: Option<String>,
    pub source_policy_ref: String,
    pub source_policy_revision: u64,
    pub source_policy_digest: String,
    pub source_required_capability: String,
    pub target_subscription_id: String,
    pub target_subscription_revision: u64,
    pub target_authorization_policy_ref: String,
    pub target_policy_revision: u64,
    pub target_policy_digest: String,
    pub target_required_capability: String,
    pub authority_digest: String,
}

/// Canonical Message admission authority. Delegation remains a distinct
/// responsibility-changing path; peer-Team admission grants conversation
/// authorship only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "authority_kind",
    content = "authority",
    rename_all = "snake_case"
)]
pub enum MessageAdmissionAuthority {
    PeerTeam(PeerTeamMessageAdmissionAuthority),
    WorkDelegation(CollaborationMessageAuthority),
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
    /// Immutable target Work reference frozen when the Delegation was accepted.
    /// This must remain byte-equal to `WorkDelegationV1.target_work_ref`.
    pub fact_work_ref: RemoteWorkRef,
    /// Exact native Work revision on which the fact was authored. A target Work
    /// may advance after Delegation acceptance, so this proof is deliberately
    /// separate from the relationship's immutable `fact_work_ref`.
    pub native_fact_work_ref: RemoteWorkRef,
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

/// Closed transfer choice for the source-authored immutable Message. Exactly
/// one payload form is present. A digest-only route is intentionally invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImmutableMessageTransferPayload {
    CanonicalBytes {
        canonical_message_bytes: Vec<u8>,
    },
    MessageObjectRef {
        message_object_ref: String,
        authenticated_content_digest: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteMessageReplica {
    pub source_execution_space_id: String,
    pub message_id: String,
    pub schema_version: u64,
    pub content_fingerprint: String,
    pub body_digest: String,
    pub canonical_message_bytes: Vec<u8>,
    pub persisted_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteMessageTransferState {
    QueuedForControlPlane,
    Routed,
    TargetPersisted,
    Terminal,
    Unknown,
}

/// Source-Node outbox intent. It carries the already-authored Message bytes or
/// object reference and never represents a local WorkDelegation/Decision fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRemoteMessageTransfer {
    pub id: String,
    pub source_execution_space_id: String,
    pub source_node_id: String,
    pub source_node_daemon_generation: u64,
    pub message_id: String,
    pub message_schema_version: u64,
    pub content_fingerprint: String,
    pub body_digest: String,
    pub target_placement: TargetPlacementRef,
    pub payload: ImmutableMessageTransferPayload,
    pub state: RemoteMessageTransferState,
    pub queued_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollaborationRetentionAnchor {
    #[serde(default)]
    pub terminal_transport_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub terminal_delegation_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub source_import_completed_at_unix_ms: Option<u64>,
}

/// Source-owned proof that a delegated artifact was actually consumed,
/// verified and durably imported. A transport/application receipt is not an
/// import and must never be used as the retention anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactImport {
    pub id: String,
    pub company_id: String,
    pub delegation_id: String,
    pub artifact_id: String,
    pub artifact_digest: String,
    pub size_bytes: u64,
    pub source_node_id: String,
    pub source_node_daemon_id: String,
    pub source_node_daemon_generation: u64,
    pub source_team_id: String,
    pub source_host_ref: ActorRef,
    pub source_work_ref: RemoteWorkRef,
    pub operation_id: String,
    pub imported_at_unix_ms: u64,
    pub revision: u64,
}

impl CollaborationRetentionAnchor {
    /// No deletion clock starts until transport, Delegation and durable source
    /// import are all terminal. Once complete, the latest boundary wins.
    pub fn safe_retention_start_unix_ms(&self) -> Option<u64> {
        if self.terminal_transport_at_unix_ms.is_none()
            || self.terminal_delegation_at_unix_ms.is_none()
            || self.source_import_completed_at_unix_ms.is_none()
        {
            return None;
        }
        [
            self.terminal_transport_at_unix_ms,
            self.terminal_delegation_at_unix_ms,
            self.source_import_completed_at_unix_ms,
        ]
        .into_iter()
        .flatten()
        .max()
    }

    pub fn retain_until_unix_ms(&self, retention_duration_ms: u64) -> Option<u64> {
        self.safe_retention_start_unix_ms()?
            .checked_add(retention_duration_ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutedBusinessKind {
    DelegationPropose,
    DelegationDecide,
    TargetWorkCreate,
    DelegationCancelRequest,
    DelegationCancelDecide,
    /// Canonical shared-Team-inbox delivery. This is the only current peer
    /// Message route kind and requires the durable target subscription fence.
    PeerMessageDeliver,
    /// Legacy WorkDelegation-scoped Message route, retained for read/replay
    /// compatibility. New ordinary peer-Team Messages never use this kind.
    TeamMessageDeliver,
    RemoteFactPublish,
    ArtifactGrant,
}

impl RoutedBusinessKind {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::DelegationPropose => "delegation_propose",
            Self::DelegationDecide => "delegation_decide",
            Self::TargetWorkCreate => "target_work_create",
            Self::DelegationCancelRequest => "delegation_cancel_request",
            Self::DelegationCancelDecide => "delegation_cancel_decide",
            Self::PeerMessageDeliver => "peer_message_deliver",
            Self::TeamMessageDeliver => "team_message_deliver",
            Self::RemoteFactPublish => "remote_fact_publish",
            Self::ArtifactGrant => "artifact_grant",
        }
    }

    pub fn required_capability(self) -> String {
        format!("collaboration.{}", self.wire_name())
    }
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
    SourceWorkAttestationInvalid,
    MessageReplicaMismatch,
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
            PeerMessageDeliver,
            "message-admission-authority",
            "canonical-message-delivery",
            "node.message",
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
                required_capability: kind.required_capability(),
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
        assert_eq!(registry.len(), 9);
        let kinds = registry
            .iter()
            .map(|entry| entry.kind)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(kinds.len(), 9);
        assert!(registry
            .iter()
            .all(|entry| entry.requires_expected_revision));
        assert!(registry
            .iter()
            .all(|entry| !entry.required_capability.is_empty()));
    }
}
