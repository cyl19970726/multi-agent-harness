use firm_core::agentfirm_api::WorkDeliveryStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentWorkDeliveryAuthority {
    CanonicalTrust,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentWorkDeliveryIntegrityAnnotation {
    RecipientMemberRunNotProvable,
    WorkExecutionBindingMissing,
    AgentSessionMissing,
    TeamMembershipMissing,
    CanonicalJoinConflict,
}

/// Non-persisted application read model for current Work delivery state.
///
/// Every row originates from one canonical trust `CanonicalWorkDelivery` and
/// is joined to its exact WorkExecutionBinding, Work, and AgentSession.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentWorkDeliveryView {
    pub authority: CurrentWorkDeliveryAuthority,
    pub read_only: bool,
    #[serde(default)]
    pub execution_space_id: Option<String>,
    pub team_run_id: String,
    pub work_id: String,
    pub work_revision: u64,
    #[serde(default)]
    pub work_execution_binding_id: Option<String>,
    pub delivery_id: String,
    #[serde(default)]
    pub recipient_agent_member_id: Option<String>,
    #[serde(default)]
    pub recipient_member_run_id: Option<String>,
    #[serde(default)]
    pub recipient_agent_session_id: Option<String>,
    #[serde(default)]
    pub recipient_agent_session_generation: Option<u64>,
    #[serde(default)]
    pub target_node_id: Option<String>,
    pub status: WorkDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_node_daemon_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_code: Option<String>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrity_annotations: Vec<CurrentWorkDeliveryIntegrityAnnotation>,
}
