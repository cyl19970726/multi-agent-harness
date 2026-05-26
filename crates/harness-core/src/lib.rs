use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMemberStatus {
    Idle,
    Busy,
    Paused,
    Stale,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMember {
    pub id: String,
    pub name: String,
    pub role: String,
    pub capabilities: Vec<String>,
    pub status: AgentMemberStatus,
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
    pub title: String,
    pub objective: String,
    pub owner_agent_id: String,
    pub assignee_agent_id: Option<String>,
    pub status: TaskStatus,
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
pub struct Message {
    pub id: String,
    pub task_id: Option<String>,
    pub from_agent_id: String,
    pub to_agent_id: Option<String>,
    pub channel: Option<String>,
    pub kind: MessageKind,
    pub content: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
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
        require_non_empty(&self.role, "AgentMember.role")
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
            title: "Inspect issue".to_string(),
            objective: "Find the root cause".to_string(),
            owner_agent_id: "leader-1".to_string(),
            assignee_agent_id: Some("agent-1".to_string()),
            status: TaskStatus::Assigned,
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
    fn validation_rejects_missing_required_id() {
        let member = AgentMember {
            id: "".to_string(),
            name: "Leader".to_string(),
            role: "leader".to_string(),
            capabilities: vec![],
            status: AgentMemberStatus::Idle,
        };

        assert_eq!(
            member.validate(),
            Err(ValidationError::Required {
                field: "AgentMember.id"
            })
        );
    }
}
