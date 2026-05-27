use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Blocked,
    Complete,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub owner_agent_id: String,
    pub status: GoalStatus,
    pub success_criteria: Vec<String>,
    pub priority: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMemberStatus {
    Creating,
    Idle,
    Assigned,
    Running,
    WaitingForInput,
    WaitingForApproval,
    Reviewing,
    Blocked,
    Closing,
    Closed,
    Error,
    Paused,
    Stale,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentProviderConfig {
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub collaboration_mode: Option<String>,
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub approvals_reviewer: Option<String>,
    #[serde(default)]
    pub sandbox_policy: Option<String>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<String>,
    #[serde(default)]
    pub environment_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMember {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub provider: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub provider_config: AgentProviderConfig,
    pub capabilities: Vec<String>,
    pub team_ids: Vec<String>,
    pub prompt_ref: Option<String>,
    pub skill_refs: Vec<String>,
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub worktree_ref: Option<String>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<String>,
    pub status: AgentMemberStatus,
    pub current_task_id: Option<String>,
    pub current_proposal_id: Option<String>,
    pub provider_runtime_id: Option<String>,
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_agent_path: Option<String>,
    #[serde(default)]
    pub provider_agent_nickname: Option<String>,
    #[serde(default)]
    pub provider_agent_role: Option<String>,
    pub control_endpoint: Option<String>,
    pub created_at: String,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTeam {
    pub id: String,
    pub name: String,
    pub description: String,
    pub owner_agent_id: String,
    pub member_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Planned,
    Assigned,
    Running,
    Blocked,
    Review,
    Done,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub goal_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub objective: String,
    pub owner_agent_id: String,
    pub assignee_agent_id: Option<String>,
    pub reviewer_agent_id: Option<String>,
    pub status: TaskStatus,
    pub depends_on_task_ids: Vec<String>,
    pub workspace_ref: Option<String>,
    pub branch_ref: Option<String>,
    pub pr_ref: Option<String>,
    pub owned_paths: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    Task,
    Report,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDeliveryStatus {
    Queued,
    Delivered,
    Acknowledged,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSessionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageTerminalSource {
    TurnCompleted,
    ThreadIdle,
    ThreadRead,
    HookStop,
    DryRun,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDelivery {
    #[serde(default)]
    pub provider_session_id: Option<String>,
    #[serde(default)]
    pub provider_request_id: Option<String>,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub terminal_source: Option<MessageTerminalSource>,
    #[serde(default)]
    pub delivered_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSession {
    pub id: String,
    pub provider: String,
    pub agent_member_id: String,
    pub task_id: Option<String>,
    pub workspace_ref: Option<String>,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub terminal_source: Option<MessageTerminalSource>,
    pub status: ProviderSessionStatus,
    pub command: String,
    pub args: Vec<String>,
    pub prompt_ref: Option<String>,
    pub prompt_summary: Option<String>,
    pub provider_session_ref: Option<String>,
    pub stdout_ref: Option<String>,
    pub jsonl_ref: Option<String>,
    pub transcript_ref: Option<String>,
    pub last_message_ref: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentRuntimeHealth {
    #[serde(default)]
    pub process_alive: bool,
    #[serde(default)]
    pub socket_exists: bool,
    #[serde(default)]
    pub protocol_probe: Option<String>,
    #[serde(default)]
    pub delivery_probe: Option<String>,
    #[serde(default)]
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntime {
    pub id: String,
    pub agent_member_id: String,
    pub provider: String,
    pub status: AgentRuntimeStatus,
    pub pid: Option<u32>,
    pub control_endpoint: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub health: AgentRuntimeHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub id: String,
    pub agent_member_id: String,
    pub provider_runtime_id: Option<String>,
    pub task_id: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub provider_child_thread_id: Option<String>,
    pub event_type: String,
    pub summary: String,
    pub payload_ref: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderChildThreadStatus {
    Open,
    Running,
    Completed,
    Interrupted,
    Errored,
    Closed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderChildThread {
    pub id: String,
    pub provider: String,
    pub agent_member_id: String,
    pub provider_runtime_id: Option<String>,
    pub task_id: Option<String>,
    pub parent_provider_thread_id: Option<String>,
    pub provider_thread_id: String,
    pub provider_agent_path: Option<String>,
    pub provider_agent_nickname: Option<String>,
    pub provider_agent_role: Option<String>,
    pub status: ProviderChildThreadStatus,
    pub last_message_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Submitted,
    Accepted,
    Rejected,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub task_id: String,
    pub agent_member_id: String,
    pub title: String,
    pub summary: String,
    pub status: ProposalStatus,
    pub changed_paths: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub task_id: Option<String>,
    pub from_agent_id: String,
    pub to_agent_id: Option<String>,
    pub channel: Option<String>,
    pub kind: MessageKind,
    pub delivery_status: MessageDeliveryStatus,
    pub content: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub delivery: Option<MessageDelivery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub task_id: Option<String>,
    pub source_type: String,
    pub source_ref: String,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub task_id: String,
    pub decision: String,
    pub rationale: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
}

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("{field} is required")]
    Required { field: &'static str },
}

fn require_non_empty(value: &str, field: &'static str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::Required { field })
    } else {
        Ok(())
    }
}

impl Validate for AgentMember {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentMember.id")?;
        require_non_empty(&self.name, "AgentMember.name")?;
        require_non_empty(&self.description, "AgentMember.description")?;
        require_non_empty(&self.role, "AgentMember.role")?;
        require_non_empty(&self.provider, "AgentMember.provider")?;
        require_non_empty(&self.created_at, "AgentMember.created_at")
    }
}

impl Validate for AgentTeam {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentTeam.id")?;
        require_non_empty(&self.name, "AgentTeam.name")?;
        require_non_empty(&self.description, "AgentTeam.description")?;
        require_non_empty(&self.owner_agent_id, "AgentTeam.owner_agent_id")?;
        require_non_empty(&self.created_at, "AgentTeam.created_at")?;
        require_non_empty(&self.updated_at, "AgentTeam.updated_at")
    }
}

impl Validate for Goal {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Goal.id")?;
        require_non_empty(&self.title, "Goal.title")?;
        require_non_empty(&self.objective, "Goal.objective")?;
        require_non_empty(&self.owner_agent_id, "Goal.owner_agent_id")?;
        require_non_empty(&self.priority, "Goal.priority")?;
        require_non_empty(&self.created_at, "Goal.created_at")?;
        require_non_empty(&self.updated_at, "Goal.updated_at")
    }
}

impl Validate for Task {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Task.id")?;
        require_non_empty(&self.title, "Task.title")?;
        require_non_empty(&self.objective, "Task.objective")?;
        require_non_empty(&self.owner_agent_id, "Task.owner_agent_id")?;
        require_non_empty(&self.created_at, "Task.created_at")?;
        require_non_empty(&self.updated_at, "Task.updated_at")
    }
}

impl Validate for Message {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Message.id")?;
        require_non_empty(&self.from_agent_id, "Message.from_agent_id")?;
        require_non_empty(&self.content, "Message.content")?;
        require_non_empty(&self.created_at, "Message.created_at")
    }
}

impl Validate for ProviderSession {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderSession.id")?;
        require_non_empty(&self.provider, "ProviderSession.provider")?;
        require_non_empty(&self.agent_member_id, "ProviderSession.agent_member_id")?;
        require_non_empty(&self.command, "ProviderSession.command")?;
        require_non_empty(&self.started_at, "ProviderSession.started_at")
    }
}

impl Validate for AgentRuntime {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentRuntime.id")?;
        require_non_empty(&self.agent_member_id, "AgentRuntime.agent_member_id")?;
        require_non_empty(&self.provider, "AgentRuntime.provider")?;
        require_non_empty(&self.command, "AgentRuntime.command")?;
        require_non_empty(&self.started_at, "AgentRuntime.started_at")
    }
}

impl Validate for AgentEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentEvent.id")?;
        require_non_empty(&self.agent_member_id, "AgentEvent.agent_member_id")?;
        require_non_empty(&self.provider, "AgentEvent.provider")?;
        require_non_empty(&self.event_type, "AgentEvent.event_type")?;
        require_non_empty(&self.summary, "AgentEvent.summary")?;
        require_non_empty(&self.created_at, "AgentEvent.created_at")
    }
}

impl Validate for Proposal {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Proposal.id")?;
        require_non_empty(&self.task_id, "Proposal.task_id")?;
        require_non_empty(&self.agent_member_id, "Proposal.agent_member_id")?;
        require_non_empty(&self.title, "Proposal.title")?;
        require_non_empty(&self.summary, "Proposal.summary")?;
        require_non_empty(&self.created_at, "Proposal.created_at")?;
        require_non_empty(&self.updated_at, "Proposal.updated_at")
    }
}

impl Validate for Evidence {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Evidence.id")?;
        require_non_empty(&self.source_type, "Evidence.source_type")?;
        require_non_empty(&self.source_ref, "Evidence.source_ref")?;
        require_non_empty(&self.summary, "Evidence.summary")?;
        require_non_empty(&self.created_at, "Evidence.created_at")
    }
}

impl Validate for Decision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Decision.id")?;
        require_non_empty(&self.task_id, "Decision.task_id")?;
        require_non_empty(&self.decision, "Decision.decision")?;
        require_non_empty(&self.rationale, "Decision.rationale")?;
        require_non_empty(&self.created_at, "Decision.created_at")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_round_trips_json() {
        let task = Task {
            id: "task-1".to_string(),
            goal_id: Some("goal-1".to_string()),
            parent_task_id: None,
            title: "Inspect issue".to_string(),
            objective: "Find the root cause".to_string(),
            owner_agent_id: "leader-1".to_string(),
            assignee_agent_id: Some("agent-1".to_string()),
            reviewer_agent_id: Some("reviewer-1".to_string()),
            status: TaskStatus::Assigned,
            depends_on_task_ids: vec!["task-0".to_string()],
            workspace_ref: Some("../worktrees/task-1".to_string()),
            branch_ref: Some("agent/task-1".to_string()),
            pr_ref: Some("https://github.com/cyl19970726/multi-agent-harness/pull/1".to_string()),
            owned_paths: vec!["crates/harness-core".to_string()],
            acceptance_criteria: vec!["Evidence is attached".to_string()],
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&task).expect("serialize task");
        let parsed: Task = serde_json::from_str(&json).expect("deserialize task");

        assert_eq!(parsed, task);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn goal_round_trips_json() {
        let goal = Goal {
            id: "goal-1".to_string(),
            title: "Self-host MVP".to_string(),
            objective: "Use the harness to manage its own development".to_string(),
            owner_agent_id: "leader-1".to_string(),
            status: GoalStatus::Active,
            success_criteria: vec!["Self-hosting task graph is visible".to_string()],
            priority: "p0".to_string(),
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&goal).expect("serialize goal");
        let parsed: Goal = serde_json::from_str(&json).expect("deserialize goal");

        assert_eq!(parsed, goal);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn provider_session_round_trips_json() {
        let session = ProviderSession {
            id: "session-1".to_string(),
            provider: "codex".to_string(),
            agent_member_id: "agent-1".to_string(),
            task_id: Some("task-1".to_string()),
            workspace_ref: Some("../worktrees/task-1".to_string()),
            provider_thread_id: Some("thread-1".to_string()),
            provider_turn_id: Some("turn-1".to_string()),
            terminal_source: Some(MessageTerminalSource::TurnCompleted),
            status: ProviderSessionStatus::Succeeded,
            command: "codex".to_string(),
            args: vec!["exec".to_string()],
            prompt_ref: Some(".harness/prompts/task-1.md".to_string()),
            prompt_summary: Some("Implement task-1".to_string()),
            provider_session_ref: None,
            stdout_ref: Some(".harness/provider-sessions/session-1/stdout.log".to_string()),
            jsonl_ref: Some(".harness/provider-sessions/session-1/events.jsonl".to_string()),
            transcript_ref: None,
            last_message_ref: Some(
                ".harness/provider-sessions/session-1/last-message.md".to_string(),
            ),
            exit_code: Some(0),
            started_at: "2026-05-26T00:00:00Z".to_string(),
            ended_at: Some("2026-05-26T00:05:00Z".to_string()),
            evidence_ids: vec!["evidence-1".to_string()],
        };

        let json = serde_json::to_string(&session).expect("serialize provider session");
        let parsed: ProviderSession =
            serde_json::from_str(&json).expect("deserialize provider session");

        assert_eq!(parsed, session);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn validation_rejects_missing_required_id() {
        let member = AgentMember {
            id: "".to_string(),
            name: "Leader".to_string(),
            description: "Lead agent".to_string(),
            role: "leader".to_string(),
            provider: "codex".to_string(),
            model: None,
            profile: None,
            provider_config: AgentProviderConfig::default(),
            capabilities: vec![],
            team_ids: vec![],
            prompt_ref: None,
            skill_refs: vec![],
            workspace_policy: None,
            worktree_ref: None,
            permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            status: AgentMemberStatus::Idle,
            current_task_id: None,
            current_proposal_id: None,
            provider_runtime_id: None,
            provider_thread_id: None,
            provider_agent_path: None,
            provider_agent_nickname: None,
            provider_agent_role: None,
            control_endpoint: None,
            created_at: "2026-05-26T00:00:00Z".to_string(),
            last_seen_at: None,
        };

        assert_eq!(
            member.validate(),
            Err(ValidationError::Required {
                field: "AgentMember.id"
            })
        );
    }
}
