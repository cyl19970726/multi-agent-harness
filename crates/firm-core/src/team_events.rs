use super::*;

/// Status of a single [`MemberAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberActionStatus {
    Started,
    Progress,
    Succeeded,
    Failed,
    Cancelled,
}

/// One journaled action by a member inside an [`AgentTeamRun`]. `seq` is
/// monotonically increasing per team run and is assigned by the caller.
/// `action_type` is a free-form Harness coordination/outcome summary. Provider
/// tool, command, file, turn, chat, and reasoning streams stay exclusively in
/// the provider-native session and must not be converted into MemberActions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberAction {
    pub id: String,
    pub seq: u64,
    pub team_run_id: String,
    pub member_run_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    /// Provider-native call/item id for correlating start, progress, result,
    /// permission, and artifact frames without leaking provider semantics into
    /// the generic action id.
    #[serde(default)]
    pub provider_call_id: Option<String>,
    pub action_type: String,
    pub status: MemberActionStatus,
    /// Raw lifecycle status reported by the provider transport.
    #[serde(default)]
    pub provider_status: Option<String>,
    /// Harness interpretation after interaction/result semantics are known.
    /// `provider_status=completed` must not imply `semantic_status=succeeded`.
    #[serde(default)]
    pub semantic_status: Option<String>,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub started_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// How a [`DelegationRun`] is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationMode {
    ProviderNative,
    HarnessWorker,
    DynamicWorkflow,
}

/// Lifecycle of a [`DelegationRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationStatus {
    Planned,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

/// One delegation of work out of a [`ProviderRuntimeProjection`]: a provider-native child
/// thread, a harness worker, or a dynamic workflow run. Exactly one of
/// `provider_child_thread_id` / `workflow_run_id` is typically set, matching
/// `mode`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationRun {
    pub id: String,
    pub team_run_id: String,
    pub parent_member_run_id: String,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    pub mode: DelegationMode,
    pub provider: String,
    #[serde(default)]
    pub provider_child_thread_id: Option<String>,
    #[serde(default)]
    pub workflow_run_id: Option<String>,
    pub objective: String,
    pub status: DelegationStatus,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Where a [`TeamRunEvent`] originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunEventSourceKind {
    Host,
    Member,
    Operator,
    Service,
    Delegation,
}

/// One folded event in an [`AgentTeamRun`]'s per-run event log. `seq` is
/// monotonically increasing per team run and is assigned by the caller.
/// `entity_type` (team_run|member_run|assignment|action|message|delegation|
/// artifact) + `entity_id` + `operation` (created|updated|completed) reference
/// the ledger row this event summarizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRunEvent {
    pub id: String,
    pub seq: u64,
    pub team_run_id: String,
    pub source_kind: TeamRunEventSourceKind,
    #[serde(default)]
    pub member_run_id: Option<String>,
    #[serde(default)]
    pub delegation_run_id: Option<String>,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub summary: String,
    pub occurred_at: String,
}
