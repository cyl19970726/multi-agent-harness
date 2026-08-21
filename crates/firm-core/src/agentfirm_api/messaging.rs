use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageKind {
    Message,
    Control,
    ProviderInteractionRequest,
    ProviderInteractionResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseIntent {
    Informational,
    ResponseRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMessage {
    pub id: String,
    pub team_run_id: String,
    #[serde(default)]
    pub work_id: Option<String>,
    pub sender: ActorRef,
    pub recipients: Vec<ActorRef>,
    pub kind: TeamMessageKind,
    pub body: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    pub response_intent: ResponseIntent,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub created_at: String,
}

/// Canonical message kind. Runtime control and Work delivery are intentionally
/// excluded so neither plane can smuggle executable authority through chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    Reply,
    RequestDecision,
    ProviderInteractionRequest,
    ProviderInteractionResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRecipientKind {
    #[serde(alias = "agent_identity")]
    AgentMember,
    Team,
    ControlPlaneActor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageAddressKind {
    DirectAgent,
    TeamChannel,
    Topic,
    AuthorizedBroadcast,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRecipientRef {
    pub kind: MessageRecipientKind,
    pub id: String,
}

/// Immutable source-authored message. `author_node_*` is frozen by the source
/// NodeDaemon and is never rewritten by the Company control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub source_execution_space_id: String,
    pub source_node_id: String,
    pub source_node_daemon_id: String,
    pub source_authority_generation: u64,
    pub sender_actor_ref: ActorRef,
    #[serde(default, alias = "sender_agent_id")]
    pub sender_agent_member_id: Option<String>,
    #[serde(default)]
    pub sender_session_id: Option<String>,
    pub address_kind: MessageAddressKind,
    pub target_ref: MessageRecipientRef,
    pub recipients: Vec<MessageRecipientRef>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub team_run_id: Option<String>,
    #[serde(default)]
    pub work_id: Option<String>,
    #[serde(default)]
    pub collaboration_scope: Option<crate::collaboration::CollaborationScope>,
    pub kind: MessageKind,
    pub body: String,
    pub body_digest: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    pub response_intent: ResponseIntent,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub content_fingerprint: String,
    pub schema_version: u64,
    pub idempotency_key: String,
    pub created_at: String,
}

/// Caller-visible message intent. Source Node/daemon/session identity,
/// timestamps, digests, and fingerprints are intentionally absent and are
/// resolved by the source NodeDaemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageDraft {
    pub address_kind: MessageAddressKind,
    pub target_ref: MessageRecipientRef,
    pub recipients: Vec<MessageRecipientRef>,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub team_run_id: Option<String>,
    #[serde(default)]
    pub work_id: Option<String>,
    #[serde(default)]
    pub collaboration_scope: Option<crate::collaboration::CollaborationScope>,
    pub kind: MessageKind,
    pub body: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    pub response_intent: ResponseIntent,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub schema_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSubscriptionKind {
    Agent,
    Team,
    Channel,
    AllAuthorized,
}

/// Subject that owns one subscription or canonical inbox delivery. A Team
/// subject remains unresolved until one exact active membership generation is
/// atomically claimed; it never fans out to every Team member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSubjectKind {
    AgentMember,
    Team,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSubscriptionStatus {
    Active,
    Paused,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageHistoryPolicy {
    FromJoin,
    Latest,
    AuthorizedHistory,
}

/// Durable routing policy. Consumption progress is held separately in
/// [`SubscriptionCursor`] so changing a policy cannot rewrite inbox history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageSubscription {
    pub id: String,
    pub subscriber_kind: MessageSubjectKind,
    pub subscriber_ref: String,
    pub execution_space_id: String,
    #[serde(default)]
    pub target_team_id: Option<String>,
    pub target_node_id: String,
    pub source_kind: MessageSubscriptionKind,
    pub source_ref: String,
    pub delivery_mode: RuntimeDispatchMode,
    pub history_policy: MessageHistoryPolicy,
    #[serde(default)]
    pub membership_ref: Option<String>,
    pub authorization_policy_ref: String,
    pub policy_revision: u64,
    pub policy_digest: String,
    pub status: MessageSubscriptionStatus,
    pub revision: u64,
    pub created_by: ActorRef,
    pub created_at: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionCursor {
    pub subscription_id: String,
    #[serde(alias = "recipient_agent_id")]
    pub recipient_agent_member_id: String,
    pub last_visible_store_sequence: u64,
    pub last_delivered_store_sequence: u64,
    pub last_read_store_sequence: u64,
    pub cursor_revision: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalMessageDeliveryStatus {
    Queued,
    Routed,
    Claimed,
    ProviderReceived,
    Acknowledged,
    Failed,
    Expired,
    Invalidated,
}

/// Per-recipient inbox/delivery truth, owned by the target NodeDaemon. The
/// recipient session remains absent while no unique current session exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalMessageDelivery {
    pub id: String,
    pub message_id: String,
    pub subscription_id: String,
    pub subscription_revision: u64,
    pub subscription_policy_digest: String,
    pub recipient_kind: MessageSubjectKind,
    pub recipient_ref: String,
    #[serde(default)]
    pub target_team_id: Option<String>,
    pub target_node_id: String,
    #[serde(default)]
    pub resolved_team_membership_id: Option<String>,
    #[serde(default)]
    pub recipient_agent_member_id: Option<String>,
    #[serde(default)]
    pub recipient_session_id: Option<String>,
    #[serde(default)]
    pub recipient_session_generation: Option<u64>,
    pub status: CanonicalMessageDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_node_daemon_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub failure_detail: Option<String>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteJournalStatus {
    Pending,
    Routed,
    Received,
    Failed,
}

/// Cross-node route metadata only. It contains no provider/session ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRouteJournal {
    pub id: String,
    pub message_id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub target_execution_space_id: String,
    pub attempt: u32,
    pub status: RouteJournalStatus,
    #[serde(default)]
    pub receipt_id: Option<String>,
    pub version: u64,
    pub updated_at: String,
}
